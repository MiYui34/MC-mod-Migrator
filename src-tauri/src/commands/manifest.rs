use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::commands::assets::category_key;
use crate::models::{
    AppSession, ConfigScanMode, FileAssetCategory, FileAssetTransferItem, ImportManifestResult,
    ManifestAssetEntry, ManifestModEntry, MigrationManifest, ModTransferItem,
};

pub fn build_manifest(session: &AppSession) -> MigrationManifest {
    let mods: Vec<ManifestModEntry> = session
        .transfer_items
        .iter()
        .map(|item| ManifestModEntry {
            file_name: item.mod_info.file_name.clone(),
            mod_id: item.mod_info.mod_id.clone(),
            selected: item.selected,
            status: item.status.clone(),
            target_version: item.target_version.clone(),
            target_file_name: item.target_file_name.clone(),
        })
        .collect();

    let mut file_assets = HashMap::new();
    for (key, items) in &session.file_assets {
        file_assets.insert(
            key.clone(),
            items
                .iter()
                .map(|i| ManifestAssetEntry {
                    relative_path: i.asset.relative_path.clone(),
                    selected: i.selected,
                    status: i.status.clone(),
                })
                .collect(),
        );
    }

    MigrationManifest {
        schema_version: 1,
        exported_at: export_timestamp(),
        source_instance: session.source_instance.clone(),
        target_instance: session.target_instance.clone(),
        mods,
        file_assets,
        config_scan_mode: session.config_scan_mode,
    }
}

pub fn export_manifest_to_path(session: &AppSession, path: &str) -> anyhow::Result<()> {
    let manifest = build_manifest(session);
    let content = serde_json::to_string_pretty(&manifest)?;
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

pub fn import_manifest_from_path(path: &str, current: &AppSession) -> anyhow::Result<ImportManifestResult> {
    let content = fs::read_to_string(path)?;
    let manifest: MigrationManifest = serde_json::from_str(&content)?;
    let mut warnings = Vec::new();

    if manifest.schema_version != 1 {
        warnings.push(format!("清单 schema 版本 {} 可能不完全兼容", manifest.schema_version));
    }

    let mut session = current.clone();
    session.source_instance = manifest.source_instance;
    session.target_instance = manifest.target_instance;
    session.config_scan_mode = manifest.config_scan_mode;

    if let Some(ref src) = session.source_instance {
        if !Path::new(&src.mods_path).is_dir() {
            warnings.push(format!("源实例 mods 路径不存在: {}", src.mods_path));
        }
    }
    if let Some(ref tgt) = session.target_instance {
        if !Path::new(&tgt.mods_path).is_dir() {
            warnings.push(format!("目标实例 mods 路径不存在: {}", tgt.mods_path));
        }
    }

    if !manifest.mods.is_empty() && !session.transfer_items.is_empty() {
        let sel: HashMap<_, _> = manifest
            .mods
            .iter()
            .map(|m| (m.file_name.as_str(), m))
            .collect();
        for item in session.transfer_items.iter_mut() {
            if let Some(entry) = sel.get(item.mod_info.file_name.as_str()) {
                item.selected = entry.selected;
                item.target_version = entry.target_version.clone();
                item.target_file_name = entry.target_file_name.clone();
            }
        }
    } else if !manifest.mods.is_empty() {
        warnings.push("请先扫描 Mod 后再导入清单以恢复选中状态".into());
    }

    let mut restored_assets: HashMap<String, Vec<FileAssetTransferItem>> = HashMap::new();
    for (key, entries) in manifest.file_assets {
        if let Some(existing) = session.file_assets.get(&key) {
            let sel: HashMap<_, _> = entries
                .iter()
                .map(|e| (e.relative_path.as_str(), e))
                .collect();
            let mut updated = existing.clone();
            for item in updated.iter_mut() {
                if let Some(entry) = sel.get(item.asset.relative_path.as_str()) {
                    item.selected = entry.selected;
                }
            }
            restored_assets.insert(key, updated);
        } else {
            warnings.push(format!("资源分类 {key} 尚未扫描，请先扫描对应 Tab"));
        }
    }
    for (k, v) in restored_assets {
        session.file_assets.insert(k, v);
    }

    Ok(ImportManifestResult { session, warnings })
}

pub fn file_asset_category_from_key(key: &str) -> Option<FileAssetCategory> {
    match key {
        "shader_pack" => Some(FileAssetCategory::ShaderPack),
        "resource_pack" => Some(FileAssetCategory::ResourcePack),
        "datapack" => Some(FileAssetCategory::Datapack),
        "litematica" => Some(FileAssetCategory::Litematica),
        "mod_config" => Some(FileAssetCategory::ModConfig),
        "game_settings" => Some(FileAssetCategory::GameSettings),
        _ => None,
    }
}

pub fn merge_mod_selection(items: &mut [ModTransferItem], manifest_mods: &[ManifestModEntry]) {
    let sel: HashMap<_, _> = manifest_mods
        .iter()
        .map(|m| (m.file_name.as_str(), m))
        .collect();
    for item in items.iter_mut() {
        if let Some(entry) = sel.get(item.mod_info.file_name.as_str()) {
            item.selected = entry.selected;
            item.target_version = entry.target_version.clone();
            item.target_file_name = entry.target_file_name.clone();
        }
    }
}

fn export_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

pub fn category_key_for_export(category: FileAssetCategory) -> String {
    category_key(category).to_string()
}
