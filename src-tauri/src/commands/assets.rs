use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::stream::{self, StreamExt};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};
use tokio::time::timeout;

use crate::cancellation::CancelToken;
use crate::commands::backup::{create_migration_backup, finalize_migration_backup};
use crate::commands::history::record_migration;
use crate::commands::progress::MonotonicEmitter;
use crate::concurrency::api_concurrency;
use crate::http::{build_http_client, download_zip_file_validated};
use crate::instance::{game_settings_files, is_schematic_file_name, resolve_instance_paths, resolve_schematic_roots, shader_pack_txt_settings, InstancePaths};
use crate::models::{
    AppSettings, ConflictPolicy, ConfigScanMode, FileAsset, FileAssetCategory,
    FileAssetScanResult, FileAssetStatus, FileAssetTransferItem, InstanceInfo, MigrationRecord,
    TransferProgress, TransferResult,
};
use crate::providers::packs::{lookup_online_pack, PackKind};

pub fn category_root(paths: &InstancePaths, category: FileAssetCategory) -> PathBuf {
    match category {
        FileAssetCategory::ShaderPack => paths.shaderpacks.clone(),
        FileAssetCategory::ResourcePack => paths.resourcepacks.clone(),
        FileAssetCategory::Datapack => paths.datapacks.clone(),
        FileAssetCategory::Litematica => paths.schematics.clone(),
        FileAssetCategory::ModConfig => paths.config.clone(),
        FileAssetCategory::GameSettings => paths.game_dir.clone(),
    }
}

/// Resolve on-disk destination for a scanned asset.
pub fn asset_dest_path(paths: &InstancePaths, category: FileAssetCategory, asset: &FileAsset) -> PathBuf {
    match category {
        FileAssetCategory::GameSettings => paths.game_dir.join(&asset.relative_path),
        _ => category_root(paths, category).join(&asset.relative_path),
    }
}

pub fn category_label(category: FileAssetCategory) -> &'static str {
    match category {
        FileAssetCategory::ShaderPack => "光影包",
        FileAssetCategory::ResourcePack => "材质包",
        FileAssetCategory::Datapack => "数据包",
        FileAssetCategory::Litematica => "投影文件",
        FileAssetCategory::ModConfig => "Mod 配置",
        FileAssetCategory::GameSettings => "游戏设置",
    }
}

pub fn category_key(category: FileAssetCategory) -> &'static str {
    match category {
        FileAssetCategory::ShaderPack => "shader_pack",
        FileAssetCategory::ResourcePack => "resource_pack",
        FileAssetCategory::Datapack => "datapack",
        FileAssetCategory::Litematica => "litematica",
        FileAssetCategory::ModConfig => "mod_config",
        FileAssetCategory::GameSettings => "game_settings",
    }
}

fn emit_asset_progress(app: &AppHandle, event: &str, current: u32, total: u32, file_name: &str, message: &str) {
    let _ = app.emit(
        event,
        TransferProgress {
            current,
            total,
            file_name: file_name.to_string(),
            message: message.to_string(),
        },
    );
}

fn is_hidden(name: &str) -> bool {
    name.starts_with('.')
}

fn entry_size(path: &Path, is_dir: bool) -> u64 {
    if is_dir {
        dir_total_size(path).unwrap_or(0)
    } else {
        fs::metadata(path).map(|m| m.len()).unwrap_or(0)
    }
}

fn dir_total_size(dir: &Path) -> anyhow::Result<u64> {
    let mut total = 0u64;
    if !dir.is_dir() {
        return Ok(0);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            total += dir_total_size(&path)?;
        } else if path.is_file() {
            total += entry.metadata()?.len();
        }
    }
    Ok(total)
}

fn quick_fingerprint(path: &Path) -> Option<(u64, String)> {
    let meta = fs::metadata(path).ok()?;
    let size = meta.len();
    if size == 0 {
        return Some((0, String::new()));
    }
    let mut file = fs::File::open(path).ok()?;
    let read_len = size.min(65536) as usize;
    let mut buf = vec![0u8; read_len];
    file.read_exact(&mut buf).ok()?;
    let hash = hex::encode(Sha256::digest(&buf));
    Some((size, hash))
}

fn dir_fingerprint(dir: &Path) -> Option<(u64, u32)> {
    let mut count = 0u32;
    let size = dir_total_size(dir).ok()?;
    fn walk(d: &Path, count: &mut u32) -> anyhow::Result<()> {
        for entry in fs::read_dir(d)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                walk(&path, count)?;
            } else if path.is_file() {
                *count += 1;
            }
        }
        Ok(())
    }
    walk(dir, &mut count).ok()?;
    Some((size, count))
}

fn compare_with_target(src: &Path, dst: &Path, is_dir: bool) -> FileAssetStatus {
    if !dst.exists() {
        return FileAssetStatus::Transferable;
    }
    if is_dir {
        if !dst.is_dir() {
            return FileAssetStatus::Conflict;
        }
        match (dir_fingerprint(src), dir_fingerprint(dst)) {
            (Some(a), Some(b)) if a == b => FileAssetStatus::UpToDate,
            _ => FileAssetStatus::Conflict,
        }
    } else if !dst.is_file() {
        FileAssetStatus::Conflict
    } else {
        match (quick_fingerprint(src), quick_fingerprint(dst)) {
            (Some(a), Some(b)) if a == b => FileAssetStatus::UpToDate,
            _ => FileAssetStatus::Conflict,
        }
    }
}

fn scan_folder_entries(root: &Path, include_dirs: bool) -> Vec<(String, PathBuf, bool)> {
    let mut out = Vec::new();
    if !root.is_dir() {
        return out;
    }
    let Ok(read) = fs::read_dir(root) else {
        return out;
    };
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if is_hidden(&name) {
            continue;
        }
        let path = entry.path();
        if path.is_file() && name.to_lowercase().ends_with(".zip") {
            out.push((name, path, false));
        } else if path.is_dir() && include_dirs {
            out.push((name, path, true));
        }
    }
    out.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    out
}

fn scan_litematica_roots(roots: &[PathBuf]) -> Vec<(String, PathBuf, bool)> {
    let mut out = Vec::new();
    let mut seen_files = HashSet::new();
    for root in roots {
        scan_litematica_dir(root, root, &mut out, &mut seen_files);
    }
    out.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    out
}

fn scan_litematica_dir(
    base: &Path,
    dir: &Path,
    out: &mut Vec<(String, PathBuf, bool)>,
    seen_files: &mut HashSet<String>,
) {
    if !dir.is_dir() {
        return;
    }
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if is_hidden(&name) {
            continue;
        }
        let path = entry.path();
        if path.is_file() && is_schematic_file_name(&name) {
            let key = path
                .canonicalize()
                .unwrap_or(path.clone())
                .to_string_lossy()
                .replace('\\', "/")
                .to_lowercase();
            if !seen_files.insert(key) {
                continue;
            }
            let relative = path
                .strip_prefix(base)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| name.clone());
            out.push((relative, path, false));
        } else if path.is_dir() {
            scan_litematica_dir(base, &path, out, seen_files);
        }
    }
}

fn mod_id_matches_config_name(mod_id: &str, name: &str) -> bool {
    if mod_id.eq_ignore_ascii_case(name) {
        return true;
    }
    let lower = name.to_lowercase();
    let stem = Path::new(name).file_stem().and_then(|s| s.to_str()).unwrap_or(name);
    mod_id.eq_ignore_ascii_case(stem)
        || lower == format!("{mod_id}.json")
        || lower == format!("{mod_id}.toml")
        || lower == format!("{mod_id}.cfg")
}

fn scan_mod_config(root: &Path, mode: ConfigScanMode, mod_ids: &[String]) -> Vec<(String, PathBuf, bool)> {
    let mut out = Vec::new();
    if !root.is_dir() {
        return out;
    }
    let Ok(read) = fs::read_dir(root) else {
        return out;
    };
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if is_hidden(&name) {
            continue;
        }
        let path = entry.path();
        let is_dir = path.is_dir();
        let include = match mode {
            ConfigScanMode::All => true,
            ConfigScanMode::Related => {
                if mod_ids.is_empty() {
                    false
                } else {
                    mod_ids.iter().any(|id| mod_id_matches_config_name(id, &name))
                }
            }
        };
        if include {
            out.push((name, path, is_dir));
        }
    }
    out.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    out
}

fn scan_shader_pack(paths: &InstancePaths, include_settings: bool) -> Vec<(String, String, PathBuf, bool, bool)> {
    let mut out: Vec<(String, String, PathBuf, bool, bool)> = scan_folder_entries(&paths.shaderpacks, true)
        .into_iter()
        .map(|(name, path, is_dir)| (name.clone(), name, path, is_dir, false))
        .collect();

    if include_settings {
        for (relative, path, is_dir) in shader_pack_txt_settings(&paths.shaderpacks) {
            let display = Path::new(&relative)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(relative.as_str())
                .to_string();
            out.push((display, relative, path, is_dir, true));
        }
    }

    out.sort_by(|a, b| {
        a.4.cmp(&b.4)
            .then(a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });
    out
}

fn scan_game_settings(game_dir: &Path) -> Vec<(String, PathBuf, bool)> {
    game_settings_files(game_dir)
        .into_iter()
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            (name, p, false)
        })
        .collect()
}

fn build_asset(
    name: &str,
    path: &Path,
    relative: &str,
    is_dir: bool,
    related_mod_id: Option<String>,
    settings_file: bool,
) -> FileAsset {
    FileAsset {
        name: name.to_string(),
        relative_path: relative.to_string(),
        file_path: path.to_string_lossy().to_string(),
        is_directory: is_dir,
        size: entry_size(path, is_dir),
        related_mod_id,
        settings_file,
    }
}

fn default_selected(status: &FileAssetStatus) -> bool {
    matches!(
        status,
        FileAssetStatus::Transferable
            | FileAssetStatus::Conflict
            | FileAssetStatus::OnlineAvailable
    )
}

pub async fn scan_file_assets(
    app: AppHandle,
    category: FileAssetCategory,
    source: InstanceInfo,
    target: Option<InstanceInfo>,
    config_mode: ConfigScanMode,
    known_mod_ids: Vec<String>,
    auto_check_online: bool,
    include_shader_settings: bool,
    settings: &AppSettings,
    cancel: &CancelToken,
) -> anyhow::Result<FileAssetScanResult> {
    cancel.ensure_running()?;
    if let Some(ref tgt) = target {
        crate::instance::ensure_distinct_migration_instances(&source, tgt)?;
    }
    let src_paths = resolve_instance_paths(&source);
    let src_root = category_root(&src_paths, category);
    let tgt_paths = target.as_ref().map(|t| resolve_instance_paths(t));

    let schematic_roots = if category == FileAssetCategory::Litematica {
        Some(resolve_schematic_roots(&source))
    } else {
        None
    };

    enum ScanRow {
        Plain(String, PathBuf, bool),
        Shader(String, String, PathBuf, bool, bool),
    }

    let rows: Vec<ScanRow> = match category {
        FileAssetCategory::ShaderPack => scan_shader_pack(&src_paths, include_shader_settings)
            .into_iter()
            .map(|(display, relative, path, is_dir, settings_file)| {
                ScanRow::Shader(display, relative, path, is_dir, settings_file)
            })
            .collect(),
        FileAssetCategory::ResourcePack | FileAssetCategory::Datapack => scan_folder_entries(&src_root, true)
            .into_iter()
            .map(|(name, path, is_dir)| ScanRow::Plain(name, path, is_dir))
            .collect(),
        FileAssetCategory::Litematica => scan_litematica_roots(schematic_roots.as_deref().unwrap_or(&[]))
            .into_iter()
            .map(|(name, path, is_dir)| ScanRow::Plain(name, path, is_dir))
            .collect(),
        FileAssetCategory::ModConfig => scan_mod_config(&src_root, config_mode, &known_mod_ids)
            .into_iter()
            .map(|(name, path, is_dir)| ScanRow::Plain(name, path, is_dir))
            .collect(),
        FileAssetCategory::GameSettings => scan_game_settings(&src_paths.game_dir)
            .into_iter()
            .map(|(name, path, is_dir)| ScanRow::Plain(name, path, is_dir))
            .collect(),
    };

    let hint = if rows.is_empty() {
        if category == FileAssetCategory::Litematica {
            let dirs = schematic_roots
                .as_ref()
                .map(|roots| {
                    roots
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect::<Vec<_>>()
                        .join("、")
                })
                .unwrap_or_default();
            Some(format!(
                "未在以下目录找到 .litematic / .schem / .schematic 文件：{dirs}"
            ))
        } else {
            Some(format!(
                "源实例未找到 {} 目录或其中没有可迁移项",
                category_label(category)
            ))
        }
    } else {
        None
    };

    let total = rows.len().max(1) as u32;
    let mut items = Vec::with_capacity(rows.len());

    for (idx, row) in rows.into_iter().enumerate() {
        cancel.ensure_running()?;
        let (name, relative, path, is_dir, settings_file) = match row {
            ScanRow::Plain(name, path, is_dir) => (name.clone(), name, path, is_dir, false),
            ScanRow::Shader(display, relative, path, is_dir, settings_file) => {
                (display, relative, path, is_dir, settings_file)
            }
        };

        emit_asset_progress(
            &app,
            "asset-scan-progress",
            idx as u32 + 1,
            total,
            &name,
            &format!("正在扫描 {}", name),
        );

        let related_mod_id = if category == FileAssetCategory::ModConfig {
            known_mod_ids
                .iter()
                .find(|id| mod_id_matches_config_name(id, &name))
                .cloned()
        } else {
            None
        };

        let asset = build_asset(&name, &path, &relative, is_dir, related_mod_id, settings_file);
        let status = if let Some(ref tgt_paths) = tgt_paths {
            let dst = asset_dest_path(tgt_paths, category, &asset);
            compare_with_target(&path, &dst, is_dir)
        } else {
            FileAssetStatus::Transferable
        };

        let selected = default_selected(&status);
        items.push(FileAssetTransferItem {
            asset,
            status,
            selected,
            download_url: None,
            online_version: None,
            online_source: None,
        });
    }

    if auto_check_online
        && settings.auto_check_online_packs
        && matches!(
            category,
            FileAssetCategory::ShaderPack | FileAssetCategory::ResourcePack
        )
    {
        if let Some(ref tgt_inst) = target {
            let pack_kind = match category {
                FileAssetCategory::ShaderPack => PackKind::Shader,
                FileAssetCategory::ResourcePack => PackKind::ResourcePack,
                _ => unreachable!(),
            };
            let mc = tgt_inst.mc_version.clone();
            for item in items.iter_mut() {
                if item.asset.settings_file || item.status == FileAssetStatus::UpToDate {
                    continue;
                }
                cancel.ensure_running()?;
                if let Some(hit) = lookup_online_pack(&item.asset.name, pack_kind, &mc, settings).await {
                    item.download_url = Some(hit.download_url);
                    item.online_version = Some(hit.version);
                    item.online_source = Some(hit.source);
                    if item.status != FileAssetStatus::Transferable && item.status != FileAssetStatus::Conflict {
                        item.status = FileAssetStatus::OnlineAvailable;
                        item.selected = true;
                    }
                }
            }
        }
    }

    emit_asset_progress(
        &app,
        "asset-scan-progress",
        total,
        total,
        "",
        "扫描完成",
    );

    Ok(FileAssetScanResult { items, hint })
}

async fn download_file(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    cancel: &CancelToken,
) -> anyhow::Result<()> {
    download_zip_file_validated(client, url, dest, cancel).await
}

fn copy_recursive(src: &Path, dst: &Path) -> anyhow::Result<()> {
    if src.is_file() {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(src, dst)?;
        return Ok(());
    }
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if is_hidden(&name.to_string_lossy()) {
            continue;
        }
        let from = entry.path();
        let to = dst.join(name);
        if from.is_dir() {
            copy_recursive(&from, &to)?;
        } else {
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

async fn copy_asset(src: &Path, dst: &Path, is_dir: bool, cancel: &CancelToken) -> anyhow::Result<()> {
    cancel.ensure_running()?;
    let src = src.to_path_buf();
    let dst = dst.to_path_buf();
    tokio::task::spawn_blocking(move || {
        if is_dir {
            copy_recursive(&src, &dst)
        } else {
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&src, &dst)?;
            Ok(())
        }
    })
    .await?
}

pub async fn transfer_file_assets(
    app: AppHandle,
    category: FileAssetCategory,
    items: Vec<FileAssetTransferItem>,
    target: InstanceInfo,
    source: Option<InstanceInfo>,
    conflict_policy: ConflictPolicy,
    settings: AppSettings,
    data_dir: &Path,
    backup_enabled: bool,
    cancel: &CancelToken,
) -> anyhow::Result<TransferResult> {
    cancel.ensure_running()?;
    if let Some(ref src) = source {
        crate::instance::ensure_distinct_migration_instances(src, &target)?;
    }
    let tgt_paths = resolve_instance_paths(&target);
    let root = category_root(&tgt_paths, category);
    fs::create_dir_all(&root)?;

    let to_transfer: Vec<_> = items
        .into_iter()
        .filter(|i| {
            i.selected
                && !matches!(i.status, FileAssetStatus::UpToDate | FileAssetStatus::Incompatible)
        })
        .collect();

    let mut backup_id = None;
    let mut pre_existing_dest: HashSet<PathBuf> = HashSet::new();
    if backup_enabled && settings.backup_before_transfer {
        let paths: Vec<PathBuf> = to_transfer
            .iter()
            .filter_map(|item| {
                let dst = asset_dest_path(&tgt_paths, category, &item.asset);
                if dst.exists() {
                    Some(dst)
                } else {
                    None
                }
            })
            .collect();
        pre_existing_dest = paths.iter().cloned().collect();
        backup_id = Some(create_migration_backup(data_dir, &target.name, &paths)?);
    }

    let total = to_transfer.len() as u32;
    let client = Arc::new(build_http_client(crate::http::APP_USER_AGENT));
    let concurrency = api_concurrency(&settings);
    let progress = MonotonicEmitter::new(app.clone(), "asset-transfer-progress", total);
    let success = Arc::new(AtomicU32::new(0));
    let failed = Arc::new(AtomicU32::new(0));
    let skipped = Arc::new(AtomicU32::new(0));
    let errors = Arc::new(Mutex::new(Vec::<String>::new()));
    let created_paths = Arc::new(Mutex::new(Vec::<PathBuf>::new()));

    if total > 0 {
        progress
            .emit_status("", &format!("开始迁移 {} (0/{total})...", category_label(category)))
            .await;
    }

    stream::iter(to_transfer.into_iter())
        .map(|item| {
            let client = Arc::clone(&client);
            let tgt_paths = tgt_paths.clone();
            let progress = Arc::clone(&progress);
            let cancel = cancel.clone();
            let success = Arc::clone(&success);
            let failed = Arc::clone(&failed);
            let skipped = Arc::clone(&skipped);
            let errors = Arc::clone(&errors);
            let created_paths = Arc::clone(&created_paths);
            async move {
                if cancel.is_cancelled() {
                    return;
                }
                let label = item.asset.name.clone();
                progress.emit_status(&label, &format!("正在处理 {}", label)).await;

                let dest = asset_dest_path(&tgt_paths, category, &item.asset);
                if dest.exists() && item.status == FileAssetStatus::Conflict && conflict_policy == ConflictPolicy::Skip {
                    skipped.fetch_add(1, Ordering::Relaxed);
                    progress.step(&label, |c, t| format!("已跳过 {c}/{t}")).await;
                    return;
                }

                let result = if item.download_url.as_deref().is_some_and(|u| !u.is_empty()) {
                    timeout(
                        Duration::from_secs(120),
                        download_file(&client, item.download_url.as_ref().unwrap(), &dest, &cancel),
                    )
                    .await
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("下载超时: {label}")))
                } else {
                    timeout(
                        Duration::from_secs(120),
                        copy_asset(
                            Path::new(&item.asset.file_path),
                            &dest,
                            item.asset.is_directory,
                            &cancel,
                        ),
                    )
                    .await
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("复制超时: {label}")))
                };

                match result {
                    Ok(()) => {
                        success.fetch_add(1, Ordering::Relaxed);
                        if let Ok(mut paths) = created_paths.lock() {
                            paths.push(dest.clone());
                        }
                    }
                    Err(e) => {
                        failed.fetch_add(1, Ordering::Relaxed);
                        if let Ok(mut g) = errors.lock() {
                            g.push(format!("{}: {e}", label));
                        }
                    }
                }
                progress.step(&label, |c, t| format!("已完成 {c}/{t}")).await;
            }
        })
        .buffer_unordered(concurrency)
        .collect::<()>()
        .await;

    cancel.ensure_running()?;
    progress.emit(total, "", "迁移完成").await;

    let result = TransferResult {
        success: success.load(Ordering::Relaxed),
        failed: failed.load(Ordering::Relaxed),
        skipped: skipped.load(Ordering::Relaxed),
        errors: errors.lock().map(|g| g.clone()).unwrap_or_default(),
    };

    if let Some(ref id) = backup_id {
        let all_created = created_paths.lock().map(|g| g.clone()).unwrap_or_default();
        let new_only: Vec<PathBuf> = all_created
            .into_iter()
            .filter(|p| !pre_existing_dest.contains(p))
            .collect();
        let _ = finalize_migration_backup(data_dir, id, &new_only);
    }

    let src = source.as_ref();
    record_migration(
        data_dir,
        MigrationRecord {
            id: format!("{}", chrono_lite_timestamp()),
            timestamp: chrono_lite_timestamp(),
            source_name: src.map(|s| s.name.clone()).unwrap_or_else(|| "未知".into()),
            target_name: target.name.clone(),
            source_mc: src.map(|s| s.mc_version.clone()).unwrap_or_default(),
            target_mc: target.mc_version.clone(),
            category: category_key(category).into(),
            success: result.success,
            failed: result.failed,
            skipped: result.skipped,
            backup_id,
            manifest_path: None,
            report_path: None,
        },
    )?;

    Ok(result)
}

fn chrono_lite_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn mod_id_matches_config() {
        assert!(mod_id_matches_config_name("carpet", "carpet"));
        assert!(mod_id_matches_config_name("carpet", "carpet.json"));
        assert!(!mod_id_matches_config_name("carpet", "sodium"));
    }

    #[test]
    fn asset_dest_path_for_shader_settings() {
        let inst = InstanceInfo {
            name: "test".into(),
            mods_path: r"C:\game\mods".into(),
            mc_version: "1.21.4".into(),
            loader: "fabric".into(),
            loader_version: String::new(),
            game_dir: r"C:\game".into(),
            launcher: None,
        };
        let paths = resolve_instance_paths(&inst);
        let asset = FileAsset {
            name: "Complementary.txt".into(),
            relative_path: "Complementary.txt".into(),
            file_path: r"C:\game\shaderpacks\Complementary.txt".into(),
            is_directory: false,
            size: 1,
            related_mod_id: None,
            settings_file: true,
        };
        assert_eq!(
            asset_dest_path(&paths, FileAssetCategory::ShaderPack, &asset),
            PathBuf::from(r"C:\game\shaderpacks\Complementary.txt")
        );
    }

    #[test]
    fn scan_litematica_supports_nested_and_schem() {
        let base = std::env::temp_dir().join(format!("litematica-scan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("sub")).unwrap();
        fs::File::create(base.join("a.litematic"))
            .unwrap()
            .write_all(b"a")
            .unwrap();
        fs::File::create(base.join("sub/b.schem"))
            .unwrap()
            .write_all(b"b")
            .unwrap();

        let found = scan_litematica_roots(&[base.clone()]);
        assert_eq!(found.len(), 2);
        assert!(found.iter().any(|(r, _, _)| r == "a.litematic"));
        assert!(found.iter().any(|(r, _, _)| r == "sub/b.schem"));

        let _ = fs::remove_dir_all(&base);
    }
}
