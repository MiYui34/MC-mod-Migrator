use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use std::time::Duration;

use futures::stream::{self, StreamExt};
use tauri::{AppHandle, Emitter};
use tokio::time::timeout;

use crate::cancellation::CancelToken;
use crate::concurrency::{api_concurrency, check_concurrency};
use crate::commands::conflicts::{build_cross_version_guide, detect_migration_warnings};
use crate::commands::backup::{create_migration_backup, finalize_migration_backup};
use crate::commands::deps::{resolve_for_transfer, sort_download_order};
use crate::commands::history::record_migration;
use crate::commands::progress::MonotonicEmitter;
use crate::commands::target_mods::{
    build_target_mod_id_index, is_fabric_api_mod, remove_conflicting_target_jars_with_index,
    scan_target_mods_folder, target_jar_cached_supports,
};
use crate::db::AppDatabase;
use crate::models::{
    AppSettings, CompatibilityCheckResponse, IdentifiedMod, InstanceInfo, MigrationRecord,
    ModFile, ModSource, ModTransferItem, TargetEnv, TransferProgress, TransferResult,
    TransferStatus,
};
use crate::compat::{
    compat_lookup_targets, local_mod_file, mod_locally_compatible, CompatTarget, ModVersionPolicy,
};
use crate::instance::infer_from_mods_path;
use crate::jar::parse_jar_metadata;
use crate::providers::curseforge::find_compatible_with_mirrors as cf_find_compatible;
use crate::providers::github::GithubProvider;
use crate::providers::mcmod::{extract_curseforge_slug, extract_modrinth_slug, McModProvider};
use crate::http::{build_http_client, copy_zip_file_validated, download_zip_file_validated};
use crate::providers::endpoints::rewrite_cf_download_url;
use crate::providers::endpoints::mirrors_with_official_fallback;
use crate::providers::modrinth::{find_compatible_with_mirrors, prefetch_project_versions, ProjectVersionCache};
use crate::version::{
    effective_game_versions, game_version_meets_constraint, is_known_loader, loader_matches_target,
    normalize_loader, normalize_mc_version,
};

fn rewrite_transfer_download_url(url: &str, settings: &AppSettings) -> String {
    let mut out = url.to_string();
    for endpoints in mirrors_with_official_fallback(&settings.mod_api_mirror) {
        out = endpoints.rewrite_download_url(&out);
    }
    rewrite_cf_download_url(&out, settings)
}

/// Controls how aggressively `find_compatible_file` searches external sources.
#[derive(Debug, Clone, Copy)]
pub struct CompatLookupOptions {
    /// Compatibility check: skip MCMod / GitHub scrapers (Modrinth + CurseForge still run).
    pub fast_check: bool,
    pub version_policy: ModVersionPolicy,
}

impl CompatLookupOptions {
    pub fn for_transfer(settings: &AppSettings) -> Self {
        Self {
            fast_check: false,
            version_policy: ModVersionPolicy::from_setting(&settings.mod_version_policy),
        }
    }

    pub fn for_check(settings: &AppSettings) -> Self {
        Self {
            fast_check: true,
            version_policy: ModVersionPolicy::from_setting(&settings.mod_version_policy),
        }
    }

    /// Dependency resolution during transfer: Modrinth + CurseForge only, no scrapers.
    pub fn for_dep_resolve(settings: &AppSettings) -> Self {
        Self::for_check(settings)
    }
}

/// Fill in target loader / MC version when instance detection returns unknown.
pub fn resolve_target_env(target: TargetEnv, mods: &[IdentifiedMod]) -> TargetEnv {
    let (path_mc, path_loader, path_loader_version) = infer_from_mods_path(&target.mods_path);

    TargetEnv {
        mods_path: target.mods_path,
        mc_version: normalize_mc_version(&if target.mc_version.is_empty() || target.mc_version == "unknown" {
            if path_mc != "unknown" {
                path_mc
            } else {
                infer_mc_version(mods)
            }
        } else {
            target.mc_version
        }),
        loader: if target.loader.is_empty() || target.loader == "unknown" {
            if path_loader != "unknown" {
                normalize_loader(&path_loader)
            } else {
                reconcile_loader(&normalize_loader(&infer_loader(mods)), mods)
            }
        } else {
            reconcile_loader(&normalize_loader(&target.loader), mods)
        },
        loader_version: if target.loader_version.is_empty() {
            path_loader_version
        } else {
            target.loader_version
        },
    }
}

fn infer_loader(mods: &[IdentifiedMod]) -> String {
    let mut counts: HashMap<String, u32> = HashMap::new();
    for m in mods {
        for loader in &m.loaders {
            if is_known_loader(loader) {
                *counts.entry(normalize_loader(loader)).or_default() += 1;
            }
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(loader, _)| loader)
        .unwrap_or_else(|| "fabric".to_string())
}

fn reconcile_loader(target_loader: &str, mods: &[IdentifiedMod]) -> String {
    let target = normalize_loader(target_loader);
    if !is_known_loader(&target) {
        return infer_loader(mods);
    }
    let inferred = infer_loader(mods);
    if !is_known_loader(&inferred) || inferred == target {
        return target;
    }
    let matches_target = mods
        .iter()
        .filter(|m| loader_matches_target(&m.loaders, &target))
        .count();
    let matches_inferred = mods
        .iter()
        .filter(|m| loader_matches_target(&m.loaders, &inferred))
        .count();
    if matches_inferred > matches_target {
        inferred
    } else {
        target
    }
}

fn infer_mc_version(mods: &[IdentifiedMod]) -> String {
    let mut counts: HashMap<String, u32> = HashMap::new();
    for m in mods {
        for gv in &m.game_versions {
            if !gv.is_empty() && gv != "unknown" {
                *counts.entry(gv.clone()).or_default() += 1;
            }
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(version, _)| version)
        .unwrap_or_else(|| "unknown".to_string())
}

pub async fn check_compatibility(
    app: AppHandle,
    mods: Vec<IdentifiedMod>,
    target: TargetEnv,
    settings: &AppSettings,
    cancel: &CancelToken,
    version_cache: &ProjectVersionCache,
) -> anyhow::Result<CompatibilityCheckResponse> {
    let target = resolve_target_env(target, &mods);
    let github = GithubProvider::new();
    let mcmod = Arc::new(McModProvider::new());
    let check_parallelism = check_concurrency(settings);
    let total = mods.len() as u32;
    let progress = MonotonicEmitter::new(app.clone(), "check-progress", total);

    let mods_path = target.mods_path.clone();
    let mc_for_scan = target.mc_version.clone();
    let prefetch_settings = settings.clone();
    let prefetch_mods = mods.clone();
    let prefetch_target = CompatTarget::from_target(&target);
    let version_cache_prefetch = version_cache.clone();
    let cancel_prefetch = cancel.clone();
    tokio::spawn(async move {
        prefetch_project_versions(
            &prefetch_settings,
            &prefetch_mods,
            &prefetch_target,
            &version_cache_prefetch,
            check_parallelism,
            Some(&cancel_prefetch),
        )
        .await;
    });

    progress
        .emit(0, "", "正在扫描目标 mods 文件夹...")
        .await;
    let target_snapshot =
        tokio::task::spawn_blocking(move || scan_target_mods_folder(&mods_path, &mc_for_scan))
            .await?;
    let target_fabric_api = target_snapshot.fabric_api_file;
    let target_jar_cache = target_snapshot.jar_mc_support;

    cancel.ensure_running()?;

    progress
        .emit_status("", "正在检查兼容性...")
        .await;

    let cancel = cancel.clone();
    let lookup_options = CompatLookupOptions::for_check(settings);
    let checked = stream::iter(mods.iter().cloned().enumerate())
        .map(|(idx, mod_info)| {
            let target = target.clone();
            let settings = settings.clone();
            let github = &github;
            let mcmod = Arc::clone(&mcmod);
            let version_cache = version_cache.clone();
            let target_fabric_api = target_fabric_api.clone();
            let target_jar_cache = target_jar_cache.clone();
            let lookup_options = lookup_options;
            let cancel = cancel.clone();
            let progress = Arc::clone(&progress);
            async move {
                if cancel.is_cancelled() {
                    return None;
                }

                progress
                    .emit_status(
                        &mod_info.file_name,
                        &format!("正在检查 · {}", mod_info.name),
                    )
                    .await;

                let item = if mod_info.source == ModSource::Unknown && mod_info.project_id.is_none() {
                    ModTransferItem {
                        mod_info: mod_info.clone(),
                        status: TransferStatus::Unknown,
                        target_file_name: None,
                        target_version: None,
                        download_url: None,
                        download_source: None,
                        selected: false,
                        is_dependency: false,
                        required_by: None,
                    }
                } else {
                    if let Ok(meta) = parse_jar_metadata(std::path::Path::new(&mod_info.file_path)) {
                        if let Some(mc_dep) = meta.depend_versions.get("minecraft") {
                            if !game_version_meets_constraint(mc_dep, &target.mc_version) {
                                return Some((
                                    idx,
                                    ModTransferItem {
                                        mod_info: mod_info.clone(),
                                        status: TransferStatus::Incompatible,
                                        target_file_name: None,
                                        target_version: None,
                                        download_url: None,
                                        download_source: None,
                                        selected: false,
                                        is_dependency: false,
                                        required_by: None,
                                    },
                                ));
                            }
                        }
                    }

                    let compatible = timeout(
                        Duration::from_secs(45),
                        find_compatible_file(
                            &mod_info,
                            &target,
                            &settings,
                            github,
                            &mcmod,
                            Some(&version_cache),
                            lookup_options,
                        ),
                    )
                    .await
                    .ok()
                    .flatten();

                    let (status, target_file_name, target_version, download_url, download_source, selected) =
                        match compatible {
                            Some(file) => {
                                let reuse_target_fabric_api = is_fabric_api_mod(&mod_info)
                                    && target_fabric_api.is_some();
                                let chosen_name = if reuse_target_fabric_api {
                                    target_fabric_api.clone().unwrap()
                                } else {
                                    file.file_name.clone()
                                };
                                let up_to_date = if reuse_target_fabric_api {
                                    true
                                } else {
                                    target_jar_cached_supports(&target_jar_cache, &chosen_name)
                                };
                                let status = if up_to_date {
                                    TransferStatus::UpToDate
                                } else {
                                    TransferStatus::Transferable
                                };
                                (
                                    status,
                                    Some(chosen_name),
                                    Some(file.version),
                                    if up_to_date {
                                        None
                                    } else if file.download_url.is_empty() {
                                        None
                                    } else {
                                        Some(file.download_url)
                                    },
                                    Some(file.source),
                                    !up_to_date,
                                )
                            }
                            None => (
                                TransferStatus::Incompatible,
                                None,
                                None,
                                None,
                                None,
                                false,
                            ),
                        };

                    ModTransferItem {
                        mod_info,
                        status,
                        target_file_name,
                        target_version,
                        download_url,
                        download_source,
                        selected,
                        is_dependency: false,
                        required_by: None,
                    }
                };

                progress
                    .step(&item.mod_info.file_name, |current, total| {
                        format!(
                            "已检查 {}/{} · {}",
                            current, total, item.mod_info.name
                        )
                    })
                    .await;

                Some((idx, item))
            }
        })
        .buffer_unordered(check_parallelism)
        .collect::<Vec<Option<(usize, ModTransferItem)>>>()
        .await;

    cancel.ensure_running()?;

    let mut sorted: Vec<(usize, ModTransferItem)> = checked.into_iter().flatten().collect();
    sorted.sort_by_key(|(idx, _)| *idx);
    let items: Vec<ModTransferItem> = sorted.into_iter().map(|(_, item)| item).collect();

    progress.emit(total, "", "兼容性检查完成").await;

    let source_mc = infer_mc_version(&mods);
    let warnings = detect_migration_warnings(&mods, &target.mods_path, &target, &items);
    let cross_version_guide = build_cross_version_guide(&source_mc, &target.mc_version, &items);

    Ok(CompatibilityCheckResponse {
        items,
        warnings,
        cross_version_guide,
    })
}

pub(crate) async fn find_compatible_file(
    mod_info: &IdentifiedMod,
    target: &TargetEnv,
    settings: &AppSettings,
    github: &GithubProvider,
    mcmod: &McModProvider,
    version_cache: Option<&ProjectVersionCache>,
    options: CompatLookupOptions,
) -> Option<ModFile> {
    let source_mod_version = mod_info.current_version.as_deref();
    let source_gvs = effective_game_versions(mod_info);
    let lookup_targets = compat_lookup_targets(target, mod_info);
    let fast = options.fast_check;
    let version_policy = options.version_policy;

    for compat in &lookup_targets {
        if mod_locally_compatible(mod_info, compat) {
            return Some(local_mod_file(mod_info));
        }
    }

    let skip_scrapers = fast
        && mod_info.source != ModSource::Metadata
        && mod_info.source != ModSource::Unknown;
    let try_curseforge = !fast
        || mod_info.source == ModSource::Curseforge
        || mod_info.curseforge_id.is_some();
    let try_mcmod = !skip_scrapers || mod_info.source == ModSource::Metadata;

    if fast {
        let can_modrinth = mod_info
            .project_id
            .as_ref()
            .is_some_and(|id| !id.is_empty())
            || !mod_info.sha512.is_empty()
            || !mod_info.sha1.is_empty()
            || mod_info
                .mod_id
                .as_ref()
                .is_some_and(|id| !id.is_empty());
        if can_modrinth {
            for compat in &lookup_targets {
                if let Some(f) = find_compatible_with_mirrors(
                    settings,
                    mod_info,
                    compat,
                    version_cache,
                    true,
                    version_policy,
                )
                .await
                {
                    return Some(f);
                }
            }
        }
        if try_curseforge {
            for compat in &lookup_targets {
                if let Some(f) = cf_find_compatible(settings, mod_info, compat, version_policy).await {
                    return Some(f);
                }
            }
        }
        if skip_scrapers {
            if can_modrinth {
                for compat in &lookup_targets {
                    if let Some(f) = find_compatible_with_mirrors(
                        settings,
                        mod_info,
                        compat,
                        version_cache,
                        false,
                        version_policy,
                    )
                    .await
                    {
                        return Some(f);
                    }
                }
            }
            return None;
        }
    }

    for compat in &lookup_targets {
        for source in &settings.download_source_priority {
            match source.as_str() {
                "modrinth" => {
                    if let Some(f) = find_compatible_with_mirrors(
                        settings,
                        mod_info,
                        compat,
                        version_cache,
                        false,
                        version_policy,
                    )
                    .await
                    {
                        return Some(f);
                    }
                }
                "curseforge" if try_curseforge => {
                    if let Some(f) = cf_find_compatible(settings, mod_info, compat, version_policy).await {
                        return Some(f);
                    }
                }
                "mcmod" if try_mcmod => {
                    let query = if !mod_info.name.is_empty() && mod_info.name != mod_info.file_name {
                        mod_info.name.as_str()
                    } else if let Some(id) = &mod_info.mod_id {
                        id.as_str()
                    } else {
                        mod_info.file_name.trim_end_matches(".jar")
                    };
                    let mcmod = mcmod;
                    if let Ok(Some(result)) = mcmod.search(query).await {
                        if let Some(url) = &result.modrinth_url {
                            if let Some(slug) = extract_modrinth_slug(url) {
                                let mut enriched = mod_info.clone();
                                if enriched.project_id.is_none() {
                                    enriched.project_id = Some(slug);
                                }
                                if let Some(f) = find_compatible_with_mirrors(
                                    settings,
                                    &enriched,
                                    compat,
                                    version_cache,
                                    false,
                                    version_policy,
                                )
                                .await
                                {
                                    return Some(f);
                                }
                            }
                        }
                        if let Some(url) = &result.github_url {
                            let mut enriched = mod_info.clone();
                            if enriched.github_url.is_none() {
                                enriched.github_url = Some(url.clone());
                            }
                            if let Ok(Some(f)) = github
                                .get_compatible_release(
                                    url,
                                    compat,
                                    &source_gvs,
                                    source_mod_version,
                                )
                                .await
                            {
                                return Some(f);
                            }
                        }
                        if let Some(url) = &result.curseforge_url {
                            if let Some(slug) = extract_curseforge_slug(url) {
                                let mut enriched = mod_info.clone();
                                if enriched.mod_id.is_none() {
                                    enriched.mod_id = Some(slug);
                                }
                                if let Some(f) =
                                    cf_find_compatible(settings, &enriched, compat, version_policy).await
                                {
                                    return Some(f);
                                }
                            }
                        }
                    }
                }
                "github" if !skip_scrapers => {
                    if let Some(url) = &mod_info.github_url {
                        if let Ok(Some(f)) = github
                            .get_compatible_release(
                                url,
                                compat,
                                &source_gvs,
                                source_mod_version,
                            )
                            .await
                        {
                            return Some(f);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn emit_transfer_progress(app: &AppHandle, current: u32, total: u32, file_name: &str, message: &str) {
    let _ = app.emit(
        "transfer-progress",
        TransferProgress {
            current,
            total,
            file_name: file_name.to_string(),
            message: message.to_string(),
        },
    );
}

pub async fn transfer_mods(
    app: AppHandle,
    items: Vec<ModTransferItem>,
    source_mods: Vec<IdentifiedMod>,
    target: TargetEnv,
    settings: AppSettings,
    _db: &Arc<AppDatabase>,
    cancel: &CancelToken,
    version_cache: &ProjectVersionCache,
    data_dir: Option<&Path>,
    source_instance: Option<&InstanceInfo>,
    target_instance_name: Option<&str>,
) -> anyhow::Result<(TransferResult, Vec<String>)> {
    let target = resolve_target_env(target, &source_mods);
    if let Some(src) = source_instance {
        crate::instance::ensure_distinct_migration_source_target(src, &target.mods_path)?;
    }
    let target_path = target.mods_path.clone();
    std::fs::create_dir_all(&target_path)?;

    let concurrency = api_concurrency(&settings);

    let mut items = items;

    emit_transfer_progress(&app, 0, 0, "", "正在解析前置依赖...");
    resolve_for_transfer(
        Some(&app),
        &mut items,
        &source_mods,
        &target,
        &settings,
        Some(version_cache),
        cancel,
    )
    .await?;

    let selected: Vec<_> = items
        .into_iter()
        .filter(|i| i.selected && i.status == TransferStatus::Transferable)
        .collect();

    let sorted = sort_download_order(selected);
    let total = sorted.len() as u32;

    let mut backup_id = None;
    let mut pre_existing_dest: HashSet<PathBuf> = HashSet::new();
    if settings.backup_before_transfer {
        if let Some(data_dir) = data_dir {
            let paths: Vec<PathBuf> = sorted
                .iter()
                .filter_map(|item| {
                    let file_name = item
                        .target_file_name
                        .clone()
                        .unwrap_or_else(|| item.mod_info.file_name.clone());
                    let dest = PathBuf::from(&target_path).join(&file_name);
                    if dest.exists() {
                        Some(dest)
                    } else {
                        None
                    }
                })
                .collect();
            pre_existing_dest = paths.iter().cloned().collect();
            let name = target_instance_name.unwrap_or("target");
            backup_id = create_migration_backup(data_dir, name, &paths).ok();
        }
    }

    let client = Arc::new(build_http_client(crate::http::APP_USER_AGENT));
    let progress = MonotonicEmitter::new(app.clone(), "transfer-progress", total);
    let settings = settings.clone();

    let success = Arc::new(AtomicU32::new(0));
    let failed = Arc::new(AtomicU32::new(0));
    let errors = Arc::new(Mutex::new(Vec::<String>::new()));
    let transferred = Arc::new(Mutex::new(Vec::<String>::new()));
    let target_path = Arc::new(target_path);
    let target_mod_index = Arc::new(Mutex::new(build_target_mod_id_index(&target_path)));
    let cancel = cancel.clone();

    if total > 0 {
        progress
            .emit_status("", &format!("开始下载/复制 (0/{total})..."))
            .await;
    }

    stream::iter(sorted.into_iter())
        .map(|item| {
            let client = Arc::clone(&client);
            let target_path = Arc::clone(&target_path);
            let target_mod_index = Arc::clone(&target_mod_index);
            let progress = Arc::clone(&progress);
            let cancel = cancel.clone();
            let success = Arc::clone(&success);
            let failed = Arc::clone(&failed);
            let errors = Arc::clone(&errors);
            let transferred = Arc::clone(&transferred);
            let settings = settings.clone();
            async move {
                if cancel.is_cancelled() {
                    return;
                }

                let file_name = item
                    .target_file_name
                    .clone()
                    .unwrap_or_else(|| item.mod_info.file_name.clone());
                let label = if item.is_dependency {
                    format!("前置 {file_name}")
                } else {
                    file_name.clone()
                };

                progress
                    .emit_status(&label, &format!("正在处理 · {}", item.mod_info.name))
                    .await;

                let dest = PathBuf::from(target_path.as_str()).join(&file_name);

                if let Ok(index) = target_mod_index.lock() {
                    remove_conflicting_target_jars_with_index(
                        target_path.as_str(),
                        &index,
                        &item.mod_info,
                        &file_name,
                    );
                }

                let download_url = rewrite_transfer_download_url(
                    item.download_url.as_ref().unwrap(),
                    &settings,
                );
                let result = if item.download_url.as_deref().is_some_and(|u| !u.is_empty()) {
                    timeout(
                        Duration::from_secs(180),
                        download_zip_file_validated(
                            &client,
                            &download_url,
                            &dest,
                            &cancel,
                        ),
                    )
                    .await
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("下载超时: {file_name}")))
                } else {
                    timeout(
                        Duration::from_secs(60),
                        copy_zip_file_validated(Path::new(&item.mod_info.file_path), &dest),
                    )
                    .await
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("复制超时: {file_name}")))
                };

                match result {
                    Ok(()) => {
                        success.fetch_add(1, Ordering::Relaxed);
                        if let Ok(mut t) = transferred.lock() {
                            t.push(file_name.clone());
                        }
                    }
                    Err(e) => {
                        failed.fetch_add(1, Ordering::Relaxed);
                        if let Ok(mut guard) = errors.lock() {
                            guard.push(format!("{}: {e}", item.mod_info.name));
                        }
                    }
                }

                progress
                    .step(&label, |current, total| {
                        if item.download_url.as_deref().is_some_and(|u| !u.is_empty()) {
                            format!("已下载 {current}/{total} · {}", item.mod_info.name)
                        } else {
                            format!("已复制 {current}/{total} · {}", item.mod_info.name)
                        }
                    })
                    .await;
            }
        })
        .buffer_unordered(concurrency)
        .collect::<()>()
        .await;

    cancel.ensure_running()?;

    progress.emit(total, "", "转移完成").await;

    let result = TransferResult {
        success: success.load(Ordering::Relaxed),
        failed: failed.load(Ordering::Relaxed),
        skipped: 0,
        errors: errors.lock().map(|g| g.clone()).unwrap_or_default(),
    };

    let transferred_names = transferred.lock().map(|g| g.clone()).unwrap_or_default();

    if let (Some(data_dir), Some(ref id)) = (data_dir, &backup_id) {
        let created: Vec<PathBuf> = transferred_names
            .iter()
            .map(|name| PathBuf::from(target_path.as_str()).join(name))
            .filter(|dest| !pre_existing_dest.contains(dest))
            .collect();
        let _ = finalize_migration_backup(data_dir, id, &created);
    }

    if let Some(data_dir) = data_dir {
        let _ = record_migration(
            data_dir,
            MigrationRecord {
                id: format!("mod-{}", timestamp_secs()),
                timestamp: timestamp_secs(),
                source_name: source_instance
                    .map(|s| s.name.clone())
                    .unwrap_or_else(|| "未知".into()),
                target_name: target_instance_name.unwrap_or("目标").into(),
                source_mc: source_instance
                    .map(|s| s.mc_version.clone())
                    .unwrap_or_default(),
                target_mc: target.mc_version.clone(),
                category: "mod".into(),
                success: result.success,
                failed: result.failed,
                skipped: result.skipped,
                backup_id,
                manifest_path: None,
                report_path: None,
            },
        );
    }

    Ok((result, transferred_names))
}

fn timestamp_secs() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}
