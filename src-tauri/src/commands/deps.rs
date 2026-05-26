use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use std::time::Duration;

use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use tokio::time::timeout;

use crate::cancellation::CancelToken;
use crate::commands::target_mods::{
    build_target_mod_id_index, find_usable_fabric_api_in_target, is_fabric_api_project_ref,
    target_jar_supports_mc, FABRIC_API_PROJECT_ID,
};
use crate::commands::transfer::{find_compatible_file, CompatLookupOptions};
use crate::compat::CompatTarget;
use crate::models::{
    AppSettings, IdentifiedMod, ModFile, ModSource, ModTransferItem, TargetEnv, TransferProgress,
    TransferStatus,
};
use crate::providers::github::GithubProvider;
use crate::providers::mcmod::McModProvider;
use crate::providers::modrinth::{ModrinthProvider, ProjectVersionCache};

const LOADER_DEPS: &[&str] = &[
    "minecraft",
    "java",
    "fabricloader",
    "fabric-loader",
    "forge",
    "neoforge",
    "quilt-loader",
    "quilt_loader",
    "fabric-api",
    "fabric",
];

/// Per-request cap so a slow mirror cannot block transfer for minutes.
const DEP_LOOKUP_TIMEOUT: Duration = Duration::from_secs(15);
const DEP_COMPAT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_DEP_QUEUE: usize = 64;

pub fn is_loader_or_game_dep(dep_id: &str) -> bool {
    let lower = dep_id.to_lowercase();
    LOADER_DEPS
        .iter()
        .any(|d| lower == *d || lower.starts_with(&format!("{d}-")))
}

fn emit_resolve_progress(app: Option<&AppHandle>, current: u32, total: u32, message: &str) {
    if let Some(app) = app {
        let _ = app.emit(
            "transfer-progress",
            TransferProgress {
                current,
                total,
                file_name: String::new(),
                message: message.to_string(),
            },
        );
    }
}

/// Resolve missing dependencies for selected mods before download.
/// Skips deps already present in the mod list or target folder.
pub async fn resolve_for_transfer(
    app: Option<&AppHandle>,
    items: &mut Vec<ModTransferItem>,
    source_mods: &[IdentifiedMod],
    target: &TargetEnv,
    settings: &AppSettings,
    version_cache: Option<&ProjectVersionCache>,
    cancel: &CancelToken,
) -> anyhow::Result<()> {
    let modrinth = ModrinthProvider::for_settings(settings);
    let github = GithubProvider::new();
    let mcmod = McModProvider::new();
    let project_id_cache = Arc::new(Mutex::new(HashMap::<String, String>::new()));

    let mod_id_to_project: HashMap<String, String> = source_mods
        .iter()
        .filter_map(|m| {
            m.mod_id.as_ref().and_then(|id| {
                m.project_id
                    .as_ref()
                    .map(|pid| (id.clone(), pid.clone()))
            })
        })
        .collect();

    let mut queue: VecDeque<(String, String)> = VecDeque::new();
    let mut queued: HashSet<String> = HashSet::new();
    let mut resolved_keys: HashSet<String> = HashSet::new();
    let mut failed_keys: HashSet<String> = HashSet::new();

    let compat = CompatTarget::from_target(target);

    let selected: Vec<ModTransferItem> = items
        .iter()
        .filter(|i| {
            i.selected
                && i.status != TransferStatus::Incompatible
                && i.status != TransferStatus::Unknown
        })
        .cloned()
        .collect();

    let target_mod_index = build_target_mod_id_index(&target.mods_path);
    let collect_total = selected.len().max(1) as u32;

    emit_resolve_progress(app, 0, collect_total, "正在收集前置依赖...");

    for (idx, item) in selected.iter().enumerate() {
        cancel.ensure_running()?;
        emit_resolve_progress(
            app,
            idx as u32,
            collect_total,
            &format!(
                "正在收集前置依赖 ({}/{}) · {}",
                idx + 1,
                selected.len(),
                item.mod_info.name
            ),
        );
        collect_deps_for_item(
            &item,
            &mod_id_to_project,
            &modrinth,
            &mut queue,
            &mut queued,
        )
        .await;
    }

    let existing_files = list_target_files(&target.mods_path);
    let target_fabric_api =
        find_usable_fabric_api_in_target(&target.mods_path, &target.mc_version);

    cancel.ensure_running()?;

    let resolve_total = queue.len().max(1) as u32;
    let mut resolved = 0u32;

    while let Some((dep_ref, required_by)) = queue.pop_front() {
        cancel.ensure_running()?;

        let key = dep_ref.to_lowercase();
        if resolved_keys.contains(&key) || failed_keys.contains(&key) {
            continue;
        }

        if queue.len() + resolved as usize >= MAX_DEP_QUEUE {
            failed_keys.insert(key);
            continue;
        }

        resolved += 1;
        emit_resolve_progress(
            app,
            resolved.min(resolve_total),
            resolve_total,
            &format!("正在解析前置依赖 ({resolved}/{resolve_total}) · {dep_ref}"),
        );

        let outcome = resolve_one_dep(
            dep_ref.clone(),
            required_by,
            &modrinth,
            &github,
            &mcmod,
            target,
            settings,
            &compat,
            version_cache,
            &project_id_cache,
            items,
            &existing_files,
            target_fabric_api.as_deref(),
            &target_mod_index,
        )
        .await;

        match outcome {
            Some(outcome) => {
                resolved_keys.insert(key);

                if items.iter().any(|i| {
                    i.mod_info.project_id.as_deref() == Some(&outcome.project_id)
                        || i.target_file_name.as_deref() == Some(&outcome.file_name)
                }) {
                    continue;
                }

                items.push(outcome.item);
                for (dep, by) in outcome.nested {
                    enqueue_dep(&mut queue, &mut queued, dep, &by);
                }
            }
            None => {
                resolved_keys.insert(key);
            }
        }
    }

    emit_resolve_progress(
        app,
        resolve_total,
        resolve_total,
        "前置依赖解析完成",
    );
    Ok(())
}

/// Scan installed mods on the target instance and return transferable dependency items
/// that are not yet present in the mods folder.
pub async fn resolve_missing_deps_for_installed(
    app: Option<&AppHandle>,
    target: &TargetEnv,
    installed_mods: &[IdentifiedMod],
    settings: &AppSettings,
    version_cache: Option<&ProjectVersionCache>,
    cancel: &CancelToken,
) -> anyhow::Result<Vec<ModTransferItem>> {
    if installed_mods.is_empty() {
        return Ok(vec![]);
    }

    let mut items: Vec<ModTransferItem> = installed_mods
        .iter()
        .map(|m| ModTransferItem {
            mod_info: m.clone(),
            status: TransferStatus::UpToDate,
            target_file_name: Some(m.file_name.clone()),
            target_version: m.current_version.clone(),
            download_url: None,
            download_source: None,
            selected: true,
            is_dependency: false,
            required_by: None,
        })
        .collect();

    resolve_for_transfer(
        app,
        &mut items,
        installed_mods,
        target,
        settings,
        version_cache,
        cancel,
    )
    .await?;

    let installed_projects: HashSet<String> = installed_mods
        .iter()
        .filter_map(|m| m.project_id.as_ref())
        .map(|p| p.to_lowercase())
        .collect();
    let installed_files: HashSet<String> = installed_mods
        .iter()
        .map(|m| m.file_name.to_lowercase())
        .collect();

    Ok(items
        .into_iter()
        .filter(|i| {
            i.is_dependency
                && i.status == TransferStatus::Transferable
                && i.download_url.is_some()
        })
        .filter(|i| {
            if let Some(pid) = &i.mod_info.project_id {
                !installed_projects.contains(&pid.to_lowercase())
            } else {
                true
            }
        })
        .filter(|i| {
            let fname = i
                .target_file_name
                .as_deref()
                .unwrap_or(i.mod_info.file_name.as_str());
            !installed_files.contains(&fname.to_lowercase())
        })
        .collect())
}

struct DepResolveOutcome {
    project_id: String,
    file_name: String,
    item: ModTransferItem,
    nested: Vec<(String, String)>,
}

async fn resolve_one_dep(
    dep_ref: String,
    required_by: String,
    modrinth: &ModrinthProvider,
    github: &GithubProvider,
    mcmod: &McModProvider,
    target: &TargetEnv,
    settings: &AppSettings,
    _compat: &CompatTarget,
    version_cache: Option<&ProjectVersionCache>,
    project_id_cache: &Mutex<HashMap<String, String>>,
    items: &[ModTransferItem],
    existing_files: &HashSet<String>,
    target_fabric_api: Option<&str>,
    target_mod_index: &HashMap<String, Vec<String>>,
) -> Option<DepResolveOutcome> {
    let project_id =
        resolve_project_id_cached(modrinth, project_id_cache, &dep_ref).await?;

    if is_fabric_api_project_ref(&project_id) || is_fabric_api_project_ref(&dep_ref) {
        if target_fabric_api.is_some() {
            return None;
        }
    }

    let stub = IdentifiedMod {
        file_name: String::new(),
        file_path: String::new(),
        sha512: String::new(),
        sha1: String::new(),
        fingerprint: 0,
        source: ModSource::Modrinth,
        project_id: Some(project_id.clone()),
        curseforge_id: None,
        name: dep_ref.clone(),
        name_zh: None,
        mod_id: Some(dep_ref.clone()),
        current_version: None,
        loaders: vec![],
        game_versions: vec![],
        icon_url: None,
        github_url: None,
        depends: vec![],
    };

    let compatible = timeout(
        DEP_COMPAT_TIMEOUT,
        find_compatible_file(
            &stub,
            target,
            settings,
            github,
            mcmod,
            version_cache,
            CompatLookupOptions::for_dep_resolve(settings),
        ),
    )
    .await
    .ok()
    .flatten()?;

    if is_dep_satisfied(
        &project_id,
        &dep_ref,
        &compatible,
        items,
        existing_files,
        target_fabric_api,
        target_mod_index,
    ) {
        return None;
    }

    if target_jar_supports_mc(&target.mods_path, &compatible.file_name, &target.mc_version) {
        return None;
    }

    Some(DepResolveOutcome {
        file_name: compatible.file_name.clone(),
        project_id,
        item: ModTransferItem {
            mod_info: IdentifiedMod {
                file_name: compatible.file_name.clone(),
                ..stub
            },
            status: TransferStatus::Transferable,
            target_file_name: Some(compatible.file_name),
            target_version: Some(compatible.version),
            download_url: Some(compatible.download_url),
            download_source: Some(compatible.source),
            selected: true,
            is_dependency: true,
            required_by: Some(required_by),
        },
        nested: Vec::new(),
    })
}

async fn resolve_project_id_cached(
    modrinth: &ModrinthProvider,
    cache: &Mutex<HashMap<String, String>>,
    dep_ref: &str,
) -> Option<String> {
    let key = dep_ref.to_lowercase();
    {
        let guard = cache.lock().await;
        if let Some(id) = guard.get(&key) {
            return Some(id.clone());
        }
    }
    let id = timeout(
        DEP_LOOKUP_TIMEOUT,
        modrinth.resolve_project_id(dep_ref),
    )
    .await
    .ok()
    .and_then(|r| r.ok())
    .flatten()?;
    cache.lock().await.insert(key, id.clone());
    Some(id)
}

async fn collect_deps_for_item(
    item: &ModTransferItem,
    mod_id_to_project: &HashMap<String, String>,
    modrinth: &ModrinthProvider,
    queue: &mut VecDeque<(String, String)>,
    queued: &mut HashSet<String>,
) {
    let mut collected = false;

    for dep_mod_id in &item.mod_info.depends {
        if is_loader_or_game_dep(dep_mod_id) {
            continue;
        }
        collected = true;
        if let Some(pid) = mod_id_to_project.get(dep_mod_id) {
            enqueue_dep(queue, queued, pid.clone(), &item.mod_info.name);
        } else {
            enqueue_dep(queue, queued, dep_mod_id.clone(), &item.mod_info.name);
        }
    }

    if collected || item.mod_info.depends.is_empty() {
        return;
    }

    if !item.mod_info.sha512.is_empty() {
        if let Ok(deps) = timeout(
            DEP_LOOKUP_TIMEOUT,
            modrinth.get_version_dependencies_by_hash(&item.mod_info.sha512),
        )
        .await
        {
            if let Ok(deps) = deps {
                for dep in deps {
                    if dep.dependency_type != "required" {
                        continue;
                    }
                    if let Some(dep_pid) = dep.project_id {
                        enqueue_dep(queue, queued, dep_pid, &item.mod_info.name);
                    }
                }
            }
        }
    }
}

/// Dep is satisfied when the recommended compatible file is already present.
fn is_dep_satisfied(
    project_id: &str,
    dep_ref: &str,
    compatible: &ModFile,
    items: &[ModTransferItem],
    existing_target_files: &HashSet<String>,
    target_fabric_api: Option<&str>,
    target_mod_index: &HashMap<String, Vec<String>>,
) -> bool {
    if project_id == FABRIC_API_PROJECT_ID || is_fabric_api_project_ref(project_id) {
        if target_fabric_api.is_some() {
            return true;
        }
    }

    if existing_target_files.contains(&compatible.file_name) {
        return true;
    }

    for item in items {
        if !item_matches_dep_project(item, project_id, dep_ref) {
            continue;
        }
        if item.target_file_name.as_deref() == Some(compatible.file_name.as_str()) {
            return true;
        }
        if let Some(name) = &item.target_file_name {
            if existing_target_files.contains(name) && name == &compatible.file_name {
                return true;
            }
            if existing_target_files.contains(name) && name != &compatible.file_name {
                return false;
            }
        }
    }

    let mod_id = dep_ref.to_lowercase();
    if let Some(jars) = target_mod_index.get(&mod_id) {
        if jars.iter().any(|j| j == &compatible.file_name) {
            return true;
        }
        if !jars.is_empty() {
            return false;
        }
    }

    for item in items {
        if item_matches_dep_project(item, project_id, dep_ref) {
            if item.status == TransferStatus::UpToDate || item.status == TransferStatus::Transferable
            {
                if item.target_file_name.as_deref() != Some(compatible.file_name.as_str()) {
                    return false;
                }
                return true;
            }
        }
    }

    false
}

fn item_matches_dep_project(item: &ModTransferItem, project_id: &str, dep_ref: &str) -> bool {
    if item.mod_info.project_id.as_deref() == Some(project_id) {
        return true;
    }
    item.mod_info
        .mod_id
        .as_deref()
        .is_some_and(|id| id.eq_ignore_ascii_case(dep_ref))
}

/// Sort download queue: dependencies first, then main mods.
pub fn sort_download_order(items: Vec<ModTransferItem>) -> Vec<ModTransferItem> {
    let mut deps: Vec<ModTransferItem> = items
        .iter()
        .filter(|i| i.is_dependency)
        .cloned()
        .collect();
    let mains: Vec<ModTransferItem> = items
        .into_iter()
        .filter(|i| !i.is_dependency)
        .collect();

    deps.sort_by(|a, b| {
        let a_is_dep_of_b = a.required_by.as_deref() == Some(b.mod_info.name.as_str());
        let b_is_dep_of_a = b.required_by.as_deref() == Some(a.mod_info.name.as_str());
        if a_is_dep_of_b {
            std::cmp::Ordering::Less
        } else if b_is_dep_of_a {
            std::cmp::Ordering::Greater
        } else {
            a.mod_info.name.cmp(&b.mod_info.name)
        }
    });

    deps.into_iter().chain(mains).collect()
}

fn enqueue_dep(
    queue: &mut VecDeque<(String, String)>,
    queued: &mut HashSet<String>,
    project_ref: String,
    required_by: &str,
) {
    if queue.len() >= MAX_DEP_QUEUE {
        return;
    }
    let key = project_ref.to_lowercase();
    if queued.insert(key) {
        queue.push_back((project_ref, required_by.to_string()));
    }
}

fn list_target_files(mods_path: &str) -> HashSet<String> {
    let path = std::path::Path::new(mods_path);
    let mut files = HashSet::new();
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                files.insert(name.to_string());
            }
        }
    }
    files
}
