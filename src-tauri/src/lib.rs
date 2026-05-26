mod cancellation;
mod commands;
mod compat;
mod concurrency;
mod db;
mod hash;
mod http;
mod instance;
mod jar;
mod models;
mod pe_version;
mod providers;
mod version;

use std::sync::{Arc, Mutex};

use std::path::PathBuf;

use tauri::{AppHandle, Manager, State};

use crate::cancellation::CancelToken;
use crate::commands::assets::{scan_file_assets, transfer_file_assets};
use crate::commands::backup::{backup_folder, restore_from_backup};
use crate::commands::history::{delete_migration_record, list_migration_history};
use crate::commands::diff::compare_mod_lists;
use crate::commands::identify::scan_mods_folder;
use crate::commands::manifest::{export_manifest_to_path, import_manifest_from_path};
use crate::commands::presets::{delete_preset, list_presets, upsert_preset};
use crate::commands::market::{
    market_install_batch, market_list_versions, market_lookup_by_name, market_search,
};
use crate::commands::market_extra::{
    market_check_installed, market_get_project_detail,
    market_list_missing_deps, market_list_updatable_mods, market_preview_deps,
};
use crate::commands::market_undo::{list_recent_installs, undo_install};
use crate::commands::report::export_mod_report;
use crate::commands::transfer::{check_compatibility, transfer_mods};
use crate::commands::update::{
    app_version, check_for_update, download_update, effective_manifest_url, launch_installer,
    record_check_result, should_check_now, validate_manifest_url, DEFAULT_UPDATE_MANIFEST_URL,
};
use crate::commands::versions::list_mod_versions;
use crate::db::{
    load_settings, load_session, load_update_state, save_session, save_settings,
    save_update_state, AppDatabase,
};
use crate::instance::{detect_from_path, scan_launcher_instances};
use crate::models::{
    AppSession, AppSettings, CompatibilityCheckResponse, ConflictPolicy, ConfigScanMode, FileAssetCategory,
    FileAssetScanResult, FileAssetTransferItem, IdentifiedMod, ImportManifestResult,
    InstanceInfo, MarketCategory, MarketDepPreviewItem, MarketInstallBatchResult,
    MarketInstallJob, MarketInstallRecord, MarketInstallResult, MarketItemInstallStatus,
    MarketMissingDepsScan, MarketProjectDetail, MarketSearchResponse, MarketSort, MarketSourceFilter,
    MarketUpdatableMod, MigrationPreset, MigrationRecord, ModDiffResult, ModTransferItem, ModTransferResponse,
    ModVersionOption, TargetEnv, TransferResult, UpdateCheckResult, UpdateManifest,
    UpdateState,
};
use crate::providers::modrinth::{new_version_cache, ProjectVersionCache};

struct AppState {
    db: Arc<AppDatabase>,
    data_dir: PathBuf,
    cancel: CancelToken,
    update_cancel: CancelToken,
    version_cache: ProjectVersionCache,
}

#[tauri::command]
async fn pick_folder(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let folder = app.dialog().file().blocking_pick_folder();
    Ok(folder.and_then(|p| match p {
        tauri_plugin_dialog::FilePath::Path(path) => Some(path.to_string_lossy().to_string()),
        _ => None,
    }))
}

#[tauri::command]
fn detect_instance(path: String) -> Result<InstanceInfo, String> {
    detect_from_path(&path).map_err(|e| e.to_string())
}

#[tauri::command]
async fn scan_instances() -> Result<Vec<InstanceInfo>, String> {
    tokio::task::spawn_blocking(scan_launcher_instances)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_session(state: State<'_, Mutex<AppState>>) -> Result<AppSession, String> {
    let data_dir = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir.clone()
    };
    tokio::task::spawn_blocking(move || load_session(&data_dir))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn save_app_session(
    session: AppSession,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let data_dir = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir.clone()
    };
    tokio::task::spawn_blocking(move || save_session(&data_dir, &session))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn cancel_task(state: State<'_, Mutex<AppState>>) -> Result<(), String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    s.cancel.cancel();
    Ok(())
}

#[tauri::command]
async fn scan_and_identify(
    app: AppHandle,
    mods_path: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<IdentifiedMod>, String> {
    let (settings, db, cancel) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.cancel.reset();
        (load_settings(&s.data_dir), Arc::clone(&s.db), s.cancel.clone())
    };
    scan_mods_folder(app, &mods_path, &settings, &db, &cancel)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn check_mods_compatibility(
    app: AppHandle,
    mods: Vec<IdentifiedMod>,
    target: TargetEnv,
    state: State<'_, Mutex<AppState>>,
) -> Result<CompatibilityCheckResponse, String> {
    let settings = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.cancel.reset();
        load_settings(&s.data_dir)
    };
    let cancel = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.cancel.clone()
    };
    let version_cache = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.version_cache.clone()
    };
    check_compatibility(app, mods, target, &settings, &cancel, &version_cache)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn compare_mod_diff_cmd(
    app: AppHandle,
    source_mods: Vec<IdentifiedMod>,
    target_mods_path: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<ModDiffResult, String> {
    let (settings, db, cancel) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.cancel.reset();
        (
            load_settings(&s.data_dir),
            Arc::clone(&s.db),
            s.cancel.clone(),
        )
    };
    let target_mods = scan_mods_folder(app, &target_mods_path, &settings, &db, &cancel)
        .await
        .map_err(|e| e.to_string())?;
    Ok(compare_mod_lists(&source_mods, &target_mods))
}

#[tauri::command]
fn list_migration_presets_cmd(state: State<'_, Mutex<AppState>>) -> Result<Vec<MigrationPreset>, String> {
    let data_dir = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir.clone()
    };
    Ok(list_presets(&data_dir))
}

#[tauri::command]
fn save_migration_preset_cmd(
    preset: MigrationPreset,
    state: State<'_, Mutex<AppState>>,
) -> Result<MigrationPreset, String> {
    let data_dir = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir.clone()
    };
    upsert_preset(&data_dir, preset).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_migration_preset_cmd(
    id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let data_dir = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir.clone()
    };
    delete_preset(&data_dir, &id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_mod_version_options(
    mod_info: IdentifiedMod,
    target: TargetEnv,
    source_mods: Vec<IdentifiedMod>,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<ModVersionOption>, String> {
    let (settings, version_cache) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        (load_settings(&s.data_dir), s.version_cache.clone())
    };
    list_mod_versions(mod_info, target, &source_mods, &settings, &version_cache)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn execute_transfer(
    app: AppHandle,
    items: Vec<ModTransferItem>,
    source_mods: Vec<IdentifiedMod>,
    target: TargetEnv,
    source_instance: Option<InstanceInfo>,
    target_instance_name: Option<String>,
    state: State<'_, Mutex<AppState>>,
) -> Result<ModTransferResponse, String> {
    let (settings, db, cancel, version_cache, data_dir) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.cancel.reset();
        (
            load_settings(&s.data_dir),
            Arc::clone(&s.db),
            s.cancel.clone(),
            s.version_cache.clone(),
            s.data_dir.clone(),
        )
    };
    let (result, transferred_names) = transfer_mods(
        app,
        items,
        source_mods,
        target,
        settings,
        &db,
        &cancel,
        &version_cache,
        Some(&data_dir),
        source_instance.as_ref(),
        target_instance_name.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(ModTransferResponse {
        result,
        transferred_names,
    })
}

#[tauri::command]
async fn scan_file_assets_cmd(
    app: AppHandle,
    category: FileAssetCategory,
    source: InstanceInfo,
    target: Option<InstanceInfo>,
    config_mode: ConfigScanMode,
    known_mod_ids: Vec<String>,
    auto_check_online: bool,
    include_shader_settings: bool,
    state: State<'_, Mutex<AppState>>,
) -> Result<FileAssetScanResult, String> {
    let (settings, cancel) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.cancel.reset();
        (load_settings(&s.data_dir), s.cancel.clone())
    };
    scan_file_assets(
        app,
        category,
        source,
        target,
        config_mode,
        known_mod_ids,
        auto_check_online,
        include_shader_settings,
        &settings,
        &cancel,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn transfer_file_assets_cmd(
    app: AppHandle,
    category: FileAssetCategory,
    items: Vec<FileAssetTransferItem>,
    target: InstanceInfo,
    source: Option<InstanceInfo>,
    conflict_policy: ConflictPolicy,
    backup_enabled: bool,
    state: State<'_, Mutex<AppState>>,
) -> Result<TransferResult, String> {
    let (settings, cancel, data_dir) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.cancel.reset();
        (
            load_settings(&s.data_dir),
            s.cancel.clone(),
            s.data_dir.clone(),
        )
    };
    transfer_file_assets(
        app,
        category,
        items,
        target,
        source,
        conflict_policy,
        settings,
        &data_dir,
        backup_enabled,
        &cancel,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn export_migration_manifest(
    session: AppSession,
    path: String,
) -> Result<(), String> {
    export_manifest_to_path(&session, &path).map_err(|e| e.to_string())
}

#[tauri::command]
async fn import_migration_manifest(
    path: String,
    session: AppSession,
) -> Result<ImportManifestResult, String> {
    import_manifest_from_path(&path, &session).map_err(|e| e.to_string())
}

#[tauri::command]
async fn export_mod_report_cmd(
    path: String,
    format: String,
    items: Vec<ModTransferItem>,
    result: Option<TransferResult>,
    source_label: String,
    target_label: String,
    transferred_names: Vec<String>,
) -> Result<(), String> {
    export_mod_report(
        &path,
        &format,
        &items,
        result.as_ref(),
        &source_label,
        &target_label,
        &transferred_names,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_migration_history_cmd(
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<MigrationRecord>, String> {
    let data_dir = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir.clone()
    };
    list_migration_history(&data_dir).map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_migration_record_cmd(id: String, state: State<'_, Mutex<AppState>>) -> Result<(), String> {
    let data_dir = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir.clone()
    };
    delete_migration_record(&data_dir, &id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn restore_from_backup_cmd(
    backup_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<crate::commands::backup::RestoreResult, String> {
    let data_dir = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir.clone()
    };
    restore_from_backup(&data_dir, &backup_id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn open_backup_folder(backup_id: String, state: State<'_, Mutex<AppState>>) -> Result<(), String> {
    let data_dir = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir.clone()
    };
    let path = backup_folder(&data_dir, &backup_id);
    if !path.is_dir() {
        return Err(format!("备份不存在: {}", path.display()));
    }
    // opener plugin on AppHandle - need app from state... use shell
    std::process::Command::new("explorer")
        .arg(&path)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn pick_save_file(app: AppHandle, default_name: String) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let file = app
        .dialog()
        .file()
        .set_file_name(&default_name)
        .blocking_save_file();
    Ok(file.and_then(|p| match p {
        tauri_plugin_dialog::FilePath::Path(path) => Some(path.to_string_lossy().to_string()),
        _ => None,
    }))
}

#[tauri::command]
async fn pick_open_file(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let file = app.dialog().file().blocking_pick_file();
    Ok(file.and_then(|p| match p {
        tauri_plugin_dialog::FilePath::Path(path) => Some(path.to_string_lossy().to_string()),
        _ => None,
    }))
}

#[tauri::command]
fn clear_cache(state: State<'_, Mutex<AppState>>) -> Result<(), String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    s.db.clear_cache().map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_settings(state: State<'_, Mutex<AppState>>) -> Result<AppSettings, String> {
    let data_dir = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir.clone()
    };
    tokio::task::spawn_blocking(move || load_settings(&data_dir))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn save_app_settings(
    settings: AppSettings,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    if !settings.update_manifest_url.trim().is_empty() {
        validate_manifest_url(&settings.update_manifest_url).map_err(|e| e.to_string())?;
    }
    let data_dir = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir.clone()
    };
    tokio::task::spawn_blocking(move || save_settings(&data_dir, &settings))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn market_search_cmd(
    category: MarketCategory,
    query: String,
    mc_version: String,
    loader: String,
    page: u32,
    source_filter: Option<MarketSourceFilter>,
    sort: Option<MarketSort>,
    mc_version_override: Option<String>,
    loader_override: Option<String>,
    relax_filters: Option<bool>,
    compatible_only: Option<bool>,
    state: State<'_, Mutex<AppState>>,
) -> Result<MarketSearchResponse, String> {
    let settings = {
        let s = state.lock().map_err(|e| e.to_string())?;
        load_settings(&s.data_dir)
    };
    let mc = mc_version_override
        .filter(|s| !s.is_empty())
        .unwrap_or(mc_version);
    let ld = loader_override
        .filter(|s| !s.is_empty())
        .unwrap_or(loader);
    market_search(
        category,
        query,
        mc,
        ld,
        page,
        source_filter.unwrap_or_default(),
        sort.unwrap_or_default(),
        relax_filters.unwrap_or(true),
        compatible_only.unwrap_or(false),
        &settings,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn market_get_project_detail_cmd(
    category: MarketCategory,
    source: crate::models::ModSource,
    project_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<MarketProjectDetail, String> {
    let settings = {
        let s = state.lock().map_err(|e| e.to_string())?;
        load_settings(&s.data_dir)
    };
    market_get_project_detail(category, source, project_id, &settings)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn market_check_installed_cmd(
    category: MarketCategory,
    items: Vec<crate::models::MarketSearchItem>,
    target: InstanceInfo,
    quick_check: Option<bool>,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<MarketItemInstallStatus>, String> {
    let (settings, version_cache, db) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        (
            load_settings(&s.data_dir),
            s.version_cache.clone(),
            s.db.clone(),
        )
    };
    market_check_installed(
        category,
        items,
        target,
        &settings,
        &version_cache,
        Some(db.as_ref()),
        quick_check.unwrap_or(true),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn market_preview_deps_cmd(
    download_url: String,
    file_name: String,
    project_id: String,
    source: crate::models::ModSource,
    mod_name: String,
    target: InstanceInfo,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<MarketDepPreviewItem>, String> {
    let (settings, cancel, version_cache) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.cancel.reset();
        (
            load_settings(&s.data_dir),
            s.cancel.clone(),
            s.version_cache.clone(),
        )
    };
    market_preview_deps(
        download_url,
        file_name,
        project_id,
        source,
        mod_name,
        target,
        &settings,
        &version_cache,
        &cancel,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn market_list_updatable_mods_cmd(
    target: InstanceInfo,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<MarketUpdatableMod>, String> {
    let (settings, cancel, version_cache, db) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.cancel.reset();
        (
            load_settings(&s.data_dir),
            s.cancel.clone(),
            s.version_cache.clone(),
            s.db.clone(),
        )
    };
    market_list_updatable_mods(
        target,
        &settings,
        &version_cache,
        db.as_ref(),
        &cancel,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn market_list_missing_deps_cmd(
    target: InstanceInfo,
    state: State<'_, Mutex<AppState>>,
) -> Result<MarketMissingDepsScan, String> {
    let (settings, cancel, version_cache, db) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.cancel.reset();
        (
            load_settings(&s.data_dir),
            s.cancel.clone(),
            s.version_cache.clone(),
            s.db.clone(),
        )
    };
    market_list_missing_deps(
        target,
        &settings,
        &version_cache,
        db.as_ref(),
        &cancel,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn market_list_versions_cmd(
    category: MarketCategory,
    source: crate::models::ModSource,
    project_id: String,
    target: TargetEnv,
    expand: Option<bool>,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<ModVersionOption>, String> {
    let (settings, version_cache) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        (load_settings(&s.data_dir), s.version_cache.clone())
    };
    market_list_versions(
        category,
        source,
        project_id,
        target,
        &settings,
        &version_cache,
        expand.unwrap_or(false),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn market_install_cmd(
    category: MarketCategory,
    download_url: String,
    file_name: String,
    target: InstanceInfo,
    app: AppHandle,
    state: State<'_, Mutex<AppState>>,
) -> Result<MarketInstallResult, String> {
    let (settings, cancel, data_dir) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.cancel.reset();
        (
            load_settings(&s.data_dir),
            s.cancel.clone(),
            s.data_dir.clone(),
        )
    };
    let job = MarketInstallJob {
        category,
        download_url,
        file_name,
        is_dependency: false,
        project_id: None,
        source: None,
        mod_name: None,
    };
    let batch = market_install_batch(
        Some(app),
        vec![job],
        target,
        false,
        &settings,
        &cancel,
        None,
        Some(&data_dir),
    )
    .await
    .map_err(|e| e.to_string())?;
    batch
        .results
        .into_iter()
        .next()
        .ok_or_else(|| "安装失败".to_string())
}

#[tauri::command]
async fn market_install_batch_cmd(
    jobs: Vec<MarketInstallJob>,
    target: InstanceInfo,
    resolve_deps: bool,
    app: AppHandle,
    state: State<'_, Mutex<AppState>>,
) -> Result<MarketInstallBatchResult, String> {
    let (settings, cancel, data_dir, version_cache) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.cancel.reset();
        (
            load_settings(&s.data_dir),
            s.cancel.clone(),
            s.data_dir.clone(),
            s.version_cache.clone(),
        )
    };
    market_install_batch(
        Some(app),
        jobs,
        target,
        resolve_deps,
        &settings,
        &cancel,
        Some(&version_cache),
        Some(&data_dir),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn market_lookup_by_name_cmd(
    category: MarketCategory,
    file_name: String,
    mc_version: String,
    loader: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Option<crate::models::MarketSearchItem>, String> {
    let settings = {
        let s = state.lock().map_err(|e| e.to_string())?;
        load_settings(&s.data_dir)
    };
    market_lookup_by_name(category, file_name, mc_version, loader, &settings)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn market_install_from_asset_cmd(
    category: MarketCategory,
    download_url: String,
    file_name: String,
    target: InstanceInfo,
    app: AppHandle,
    state: State<'_, Mutex<AppState>>,
) -> Result<MarketInstallBatchResult, String> {
    let (settings, cancel, data_dir) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.cancel.reset();
        (
            load_settings(&s.data_dir),
            s.cancel.clone(),
            s.data_dir.clone(),
        )
    };
    let job = MarketInstallJob {
        category,
        download_url,
        file_name,
        is_dependency: false,
        project_id: None,
        source: None,
        mod_name: None,
    };
    market_install_batch(
        Some(app),
        vec![job],
        target,
        false,
        &settings,
        &cancel,
        None,
        Some(&data_dir),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn market_list_recent_installs_cmd(
    limit: Option<usize>,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<MarketInstallRecord>, String> {
    let data_dir = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir.clone()
    };
    list_recent_installs(&data_dir, limit.unwrap_or(10)).map_err(|e| e.to_string())
}

#[tauri::command]
fn market_undo_install_cmd(
    record_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<crate::commands::backup::RestoreResult, String> {
    let data_dir = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir.clone()
    };
    undo_install(&data_dir, &record_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_app_version_cmd() -> String {
    app_version()
}

#[tauri::command]
fn get_default_update_manifest_url_cmd() -> String {
    DEFAULT_UPDATE_MANIFEST_URL.to_string()
}

#[tauri::command]
async fn check_app_update_cmd(
    manifest_url: Option<String>,
    state: State<'_, Mutex<AppState>>,
) -> Result<UpdateCheckResult, String> {
    let (data_dir, url) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        let settings = load_settings(&s.data_dir);
        let url = manifest_url
            .filter(|u| !u.trim().is_empty())
            .or_else(|| effective_manifest_url(&settings))
            .unwrap_or_default();
        (s.data_dir.clone(), url)
    };
    if url.trim().is_empty() {
        return Ok(UpdateCheckResult {
            current_version: app_version(),
            update_available: false,
            manifest: None,
        });
    }
    let mut update_state = load_update_state(&data_dir);
    match check_for_update(&url).await {
        Ok(result) => {
            record_check_result(
                &mut update_state,
                true,
                None,
                result.manifest.as_ref().map(|m| m.version.clone()),
            );
            let _ = save_update_state(&data_dir, &update_state);
            Ok(result)
        }
        Err(e) => {
            record_check_result(&mut update_state, false, Some(e.to_string()), None);
            let _ = save_update_state(&data_dir, &update_state);
            Err(e.to_string())
        }
    }
}

#[tauri::command]
async fn download_app_update_cmd(
    manifest: UpdateManifest,
    app: AppHandle,
    state: State<'_, Mutex<AppState>>,
) -> Result<String, String> {
    let (data_dir, cancel) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.update_cancel.reset();
        (s.data_dir.clone(), s.update_cancel.clone())
    };
    let path = download_update(app, &data_dir, &cancel, &manifest)
        .await
        .map_err(|e| e.to_string())?;
    let mut update_state = load_update_state(&data_dir);
    update_state.downloaded_path = Some(path.to_string_lossy().to_string());
    update_state.downloaded_version = Some(manifest.version.clone());
    let _ = save_update_state(&data_dir, &update_state);
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
fn cancel_app_update_download_cmd(state: State<'_, Mutex<AppState>>) -> Result<(), String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    s.update_cancel.cancel();
    Ok(())
}

#[tauri::command]
fn install_app_update_cmd(
    path: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let data_dir = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir.clone()
    };
    let installer = std::path::Path::new(&path);
    if let Some(expected) = load_update_state(&data_dir).downloaded_version {
        crate::pe_version::assert_installer_version(installer, &expected)
            .map_err(|e| e.to_string())?;
    }
    launch_installer(installer).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_update_state_cmd(state: State<'_, Mutex<AppState>>) -> Result<UpdateState, String> {
    let data_dir = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir.clone()
    };
    Ok(load_update_state(&data_dir))
}

#[tauri::command]
fn save_update_state_cmd(
    update_state: UpdateState,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let data_dir = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.data_dir.clone()
    };
    save_update_state(&data_dir, &update_state).map_err(|e| e.to_string())
}

#[tauri::command]
fn should_check_app_update_cmd(state: State<'_, Mutex<AppState>>) -> Result<bool, String> {
    let (data_dir, settings) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        let settings = load_settings(&s.data_dir);
        (s.data_dir.clone(), settings)
    };
    if effective_manifest_url(&settings).is_none() {
        return Ok(false);
    }
    let update_state = load_update_state(&data_dir);
    Ok(should_check_now(
        &update_state,
        settings.update_check_interval_hours,
    ))
}

fn cancel_all_tasks(app: &AppHandle) {
    if let Some(state) = app.try_state::<Mutex<AppState>>() {
        if let Ok(s) = state.lock() {
            s.cancel.cancel();
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to get app data dir");
            let db = Arc::new(
                AppDatabase::new(data_dir.clone()).expect("failed to init database"),
            );
            app.manage(Mutex::new(AppState {
                db,
                data_dir,
                cancel: CancelToken::default(),
                update_cancel: CancelToken::default(),
                version_cache: new_version_cache(),
            }));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                let app = window.app_handle().clone();
                cancel_all_tasks(&app);
                // 若后台任务拖住进程，最多 1 秒后强制退出
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    app.exit(0);
                });
            }
        })
        .invoke_handler(tauri::generate_handler![
            pick_folder,
            pick_save_file,
            pick_open_file,
            detect_instance,
            scan_instances,
            scan_and_identify,
            check_mods_compatibility,
            compare_mod_diff_cmd,
            list_migration_presets_cmd,
            save_migration_preset_cmd,
            delete_migration_preset_cmd,
            list_mod_version_options,
            execute_transfer,
            scan_file_assets_cmd,
            transfer_file_assets_cmd,
            export_migration_manifest,
            import_migration_manifest,
            export_mod_report_cmd,
            list_migration_history_cmd,
            delete_migration_record_cmd,
            restore_from_backup_cmd,
            open_backup_folder,
            cancel_task,
            get_settings,
            save_app_settings,
            clear_cache,
            get_session,
            save_app_session,
            market_search_cmd,
            market_get_project_detail_cmd,
            market_check_installed_cmd,
            market_preview_deps_cmd,
            market_list_updatable_mods_cmd,
            market_list_missing_deps_cmd,
            market_list_versions_cmd,
            market_install_cmd,
            market_install_batch_cmd,
            market_lookup_by_name_cmd,
            market_install_from_asset_cmd,
            market_list_recent_installs_cmd,
            market_undo_install_cmd,
            get_app_version_cmd,
            get_default_update_manifest_url_cmd,
            check_app_update_cmd,
            download_app_update_cmd,
            cancel_app_update_download_cmd,
            install_app_update_cmd,
            get_update_state_cmd,
            save_update_state_cmd,
            should_check_app_update_cmd,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                cancel_all_tasks(app_handle);
            }
        });
}
