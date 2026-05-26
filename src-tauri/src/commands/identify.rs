use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use futures::stream::{self, StreamExt};
use tauri::{AppHandle, Emitter};

use crate::cancellation::CancelToken;
use crate::concurrency::api_concurrency;
use crate::db::AppDatabase;
use crate::hash::compute_file_hashes_parallel;
use crate::models::FileHash;
use crate::jar::parse_jar_metadata;
use crate::models::{AppSettings, IdentifiedMod, ModSource, TransferProgress};
use crate::providers::curseforge::CurseForgeProvider;
use crate::providers::mcmod::{extract_curseforge_slug, extract_modrinth_slug, McModProvider};
use crate::providers::modrinth::ModrinthProvider;

fn emit_scan_progress(app: &AppHandle, current: u32, total: u32, file_name: &str, message: &str) {
    let _ = app.emit(
        "scan-progress",
        TransferProgress {
            current,
            total,
            file_name: file_name.to_string(),
            message: message.to_string(),
        },
    );
}

fn is_mod_jar(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("jar"))
        .unwrap_or(false)
}

enum ScanWork {
    Cached(IdentifiedMod),
    Fresh(FileHash, IdentifiedMod),
}

pub async fn scan_mods_folder(
    app: AppHandle,
    mods_path: &str,
    settings: &AppSettings,
    db: &Arc<AppDatabase>,
    cancel: &CancelToken,
) -> anyhow::Result<Vec<IdentifiedMod>> {
    let path = PathBuf::from(mods_path);
    if !path.is_dir() {
        anyhow::bail!("实例路径不存在: {mods_path}");
    }

    let jar_paths: Vec<PathBuf> = std::fs::read_dir(&path)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_mod_jar(p))
        .collect();

    if jar_paths.is_empty() {
        anyhow::bail!(
            "在 {mods_path} 未发现 .jar 文件。请选择 Minecraft 版本文件夹（如 .minecraft/versions/1.21.x-Fabric），\
             或该版本下的 mods 文件夹"
        );
    }

    let total = jar_paths.len() as u32;
    let concurrency = api_concurrency(settings);

    emit_scan_progress(&app, 0, total, "", "正在并行计算文件哈希...");

    let hashes = tokio::task::spawn_blocking(move || compute_file_hashes_parallel(&jar_paths)).await?;

    cancel.ensure_running()?;

    emit_scan_progress(&app, total, total, "", "正在批量查询 Modrinth...");

    let modrinth = Arc::new(ModrinthProvider::for_settings(settings));
    let mcmod = Arc::new(McModProvider::new());

    let modrinth_map =
        ModrinthProvider::identify_by_hashes_with_mirrors(settings, &hashes).await;

    let cf_map = CurseForgeProvider::identify_by_fingerprints_with_mirrors(settings, &hashes).await;

    emit_scan_progress(&app, 0, total, "", "正在识别 Mod...");

    let mut works: Vec<(usize, ScanWork)> = Vec::new();
    for (idx, hash) in hashes.into_iter().enumerate() {
        if let Some(cached) = db.get_cached_mod(&hash.sha512)? {
            let mut mod_info = cached;
            mod_info.file_path = hash.path.clone();
            mod_info.file_name = hash.file_name.clone();
            mod_info.sha1 = hash.sha1.clone();
            mod_info.fingerprint = hash.fingerprint;
            works.push((idx, ScanWork::Cached(mod_info)));
            continue;
        }

        let mod_info = if let Some(m) = modrinth_map.get(&hash.sha512) {
            m.clone()
        } else if let Some(m) = cf_map.get(&hash.fingerprint) {
            m.clone()
        } else {
            identify_from_metadata(&hash).await
        };

        works.push((idx, ScanWork::Fresh(hash, mod_info)));
    }

    let fresh_count = works
        .iter()
        .filter(|(_, w)| matches!(w, ScanWork::Fresh(_, _)))
        .count() as u32;

    let db = Arc::clone(db);
    let cancel = cancel.clone();
    let settings = settings.clone();
    let completed = Arc::new(AtomicU32::new(0));
    let processed = stream::iter(works)
        .map(|(idx, work)| {
            let app = app.clone();
            let modrinth = Arc::clone(&modrinth);
            let mcmod = Arc::clone(&mcmod);
            let db = Arc::clone(&db);
            let cancel = cancel.clone();
            let settings = settings.clone();
            let completed = Arc::clone(&completed);
            async move {
                if cancel.is_cancelled() {
                    return None;
                }
                match work {
                    ScanWork::Cached(mut mod_info) => {
                        if needs_project_enrichment(&mod_info) {
                            let hash = file_hash_from_mod(&mod_info);
                            enrich_mod_metadata(
                                &mut mod_info,
                                &hash,
                                &settings,
                                &modrinth,
                                &mcmod,
                            )
                            .await;
                            let _ = db.cache_mod(&mod_info);
                        }
                        Some((idx, mod_info))
                    }
                    ScanWork::Fresh(hash, mut mod_info) => {
                        let current = completed.fetch_add(1, Ordering::Relaxed) + 1;
                        emit_scan_progress(
                            &app,
                            current,
                            fresh_count.max(1),
                            &hash.file_name,
                            &format!("正在识别 {}", hash.file_name),
                        );
                        if cancel.is_cancelled() {
                            return None;
                        }
                        enrich_mod_metadata(&mut mod_info, &hash, &settings, &modrinth, &mcmod).await;
                        if cancel.is_cancelled() {
                            return None;
                        }
                        let _ = db.cache_mod(&mod_info);
                        Some((idx, mod_info))
                    }
                }
            }
        })
        .buffer_unordered(concurrency)
        .collect::<Vec<Option<(usize, IdentifiedMod)>>>()
        .await;

    cancel.ensure_running()?;

    let mut sorted: Vec<(usize, IdentifiedMod)> = processed.into_iter().flatten().collect();
    sorted.sort_by_key(|(idx, _)| *idx);
    let final_results: Vec<IdentifiedMod> = sorted.into_iter().map(|(_, m)| m).collect();

    emit_scan_progress(&app, total, total, "", "扫描完成");
    Ok(final_results)
}

fn needs_project_enrichment(mod_info: &IdentifiedMod) -> bool {
    if mod_info
        .project_id
        .as_ref()
        .is_some_and(|p| !p.is_empty())
    {
        return false;
    }
    mod_info.source == ModSource::Metadata
        || mod_info.source == ModSource::Unknown
        || mod_info
            .mod_id
            .as_ref()
            .is_some_and(|id| !id.is_empty())
}

fn file_hash_from_mod(mod_info: &IdentifiedMod) -> FileHash {
    FileHash {
        path: mod_info.file_path.clone(),
        file_name: mod_info.file_name.clone(),
        sha512: mod_info.sha512.clone(),
        sha1: mod_info.sha1.clone(),
        fingerprint: mod_info.fingerprint,
    }
}

async fn enrich_mod_metadata(
    mod_info: &mut IdentifiedMod,
    hash: &FileHash,
    settings: &AppSettings,
    modrinth: &ModrinthProvider,
    mcmod: &McModProvider,
) {
    if mod_info.mod_id.is_none() {
        if let Ok(meta) = parse_jar_metadata(std::path::Path::new(&hash.path)) {
            mod_info.mod_id = meta.mod_id;
        }
    }

    if mod_info.depends.is_empty() {
        if let Ok(meta) = parse_jar_metadata(std::path::Path::new(&hash.path)) {
            mod_info.depends = meta.depends;
        }
    }

    if mod_info.depends.is_empty() && !hash.sha512.is_empty() {
        if let Ok(deps) = modrinth.get_version_dependencies_by_hash(&hash.sha512).await {
            mod_info.depends = deps
                .into_iter()
                .filter(|d| d.dependency_type == "required")
                .filter_map(|d| d.project_id)
                .collect();
        }
    }

    let needs_provider_lookup = mod_info.source == ModSource::Metadata
        || mod_info.source == ModSource::Unknown
        || mod_info
            .project_id
            .as_ref()
            .map_or(true, |p| p.is_empty());

    if needs_provider_lookup {
        let search_query = mod_info
            .mod_id
            .clone()
            .unwrap_or_else(|| mod_info.name.clone());
        if let Ok(Some(mcmod_result)) = mcmod.search(&search_query).await {
            mod_info.name_zh = mcmod_result.name_zh;
            if mod_info.github_url.is_none() {
                mod_info.github_url = mcmod_result.github_url;
            }
            if mod_info.project_id.is_none() {
                if let Some(url) = mcmod_result.modrinth_url {
                    if let Some(slug) = extract_modrinth_slug(&url) {
                        mod_info.project_id = Some(slug);
                        if mod_info.source == ModSource::Metadata
                            || mod_info.source == ModSource::Unknown
                        {
                            mod_info.source = ModSource::Modrinth;
                        }
                    }
                } else if let Some(url) = mcmod_result.curseforge_url {
                    if let Some(slug) = extract_curseforge_slug(&url) {
                        for endpoints in crate::providers::endpoints::cf_usable_mirrors(settings) {
                            let cf = CurseForgeProvider::with_endpoints(
                                settings.curseforge_api_key.clone(),
                                endpoints,
                            );
                            if let Ok(Some(cf_id)) = cf.search_mod_id(&slug).await {
                                mod_info.curseforge_id = Some(cf_id);
                                if mod_info.source == ModSource::Metadata
                                    || mod_info.source == ModSource::Unknown
                                {
                                    mod_info.source = ModSource::Curseforge;
                                }
                                break;
                            }
                        }
                    }
                }
            }
        }

        if mod_info.project_id.is_none() {
            if let Some(mod_id) = mod_info.mod_id.as_deref().filter(|id| !id.is_empty()) {
                if let Ok(Some(pid)) = modrinth.resolve_project_id(mod_id).await {
                    mod_info.project_id = Some(pid);
                    if mod_info.source == ModSource::Metadata
                        || mod_info.source == ModSource::Unknown
                    {
                        mod_info.source = ModSource::Modrinth;
                    }
                }
            }
        }
    }
}

async fn identify_from_metadata(hash: &FileHash) -> IdentifiedMod {
    let path = Path::new(&hash.path);
    let meta = parse_jar_metadata(path).unwrap_or_default();
    IdentifiedMod {
        file_name: hash.file_name.clone(),
        file_path: hash.path.clone(),
        sha512: hash.sha512.clone(),
        sha1: hash.sha1.clone(),
        fingerprint: hash.fingerprint,
        source: if meta.name.is_some() {
            ModSource::Metadata
        } else {
            ModSource::Unknown
        },
        project_id: None,
        curseforge_id: None,
        name: meta
            .name
            .unwrap_or_else(|| hash.file_name.trim_end_matches(".jar").to_string()),
        name_zh: None,
        mod_id: meta.mod_id,
        current_version: meta.version,
        loaders: meta.loader_hint.map(|l| vec![l]).unwrap_or_default(),
        game_versions: vec![],
        icon_url: None,
        github_url: None,
        depends: meta.depends,
    }
}
