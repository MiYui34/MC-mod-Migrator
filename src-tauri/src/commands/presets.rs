use std::path::PathBuf;

use crate::db::{load_migration_presets, save_migration_presets};
use crate::models::MigrationPreset;

fn new_preset_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("preset-{nanos}")
}

pub fn list_presets(data_dir: &PathBuf) -> Vec<MigrationPreset> {
    load_migration_presets(data_dir)
}

pub fn upsert_preset(data_dir: &PathBuf, mut preset: MigrationPreset) -> anyhow::Result<MigrationPreset> {
    if preset.id.is_empty() {
        preset.id = new_preset_id();
    }
    if preset.created_at.is_empty() {
        preset.created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_default();
    }
    let mut presets = load_migration_presets(data_dir);
    if let Some(existing) = presets.iter_mut().find(|p| p.id == preset.id) {
        *existing = preset.clone();
    } else {
        presets.push(preset.clone());
    }
    save_migration_presets(data_dir, &presets)?;
    Ok(preset)
}

pub fn delete_preset(data_dir: &PathBuf, id: &str) -> anyhow::Result<()> {
    let mut presets = load_migration_presets(data_dir);
    presets.retain(|p| p.id != id);
    save_migration_presets(data_dir, &presets)
}
