use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const META_FILE: &str = "backup_meta.json";

pub fn backups_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("backups")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupEntry {
    original_path: String,
    backup_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupMeta {
    target_name: String,
    created_at: String,
    entries: Vec<BackupEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    created_paths: Vec<String>,
}

pub fn backup_folder(data_dir: &Path, backup_id: &str) -> PathBuf {
    backups_dir(data_dir).join(backup_id)
}

/// Create a backup session before migration. Always writes metadata, even when nothing existed yet.
pub fn create_migration_backup(
    data_dir: &Path,
    target_name: &str,
    existing_paths: &[PathBuf],
) -> anyhow::Result<String> {
    let slug: String = target_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let id = format!(
        "{}-{}",
        timestamp_id(),
        if slug.is_empty() { "target" } else { slug.as_str() }
    );
    let backup_root = backups_dir(data_dir).join(&id);
    fs::create_dir_all(&backup_root)?;

    let mut entries = Vec::new();
    for (idx, src) in existing_paths.iter().enumerate() {
        if !src.exists() {
            continue;
        }
        let file_name = src
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".into());
        let backup_name = format!("{idx:04}-{file_name}");
        let dest = backup_root.join(&backup_name);
        if src.is_dir() {
            copy_dir_all(src, &dest)?;
        } else {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(src, &dest)?;
        }
        entries.push(BackupEntry {
            original_path: src.to_string_lossy().to_string(),
            backup_name,
        });
    }

    write_meta(
        &backup_root,
        BackupMeta {
            target_name: target_name.to_string(),
            created_at: timestamp_id(),
            entries,
            created_paths: Vec::new(),
        },
    )?;

    Ok(id)
}

pub fn backup_paths_before_transfer(
    data_dir: &Path,
    target_name: &str,
    paths: &[PathBuf],
) -> anyhow::Result<String> {
    create_migration_backup(data_dir, target_name, paths)
}

/// Start an incremental backup session (e.g. market install). Entries are added via [`backup_path_now`].
pub fn begin_incremental_backup(
    data_dir: &Path,
    backup_id: &str,
    target_name: &str,
) -> anyhow::Result<()> {
    let backup_root = backup_folder(data_dir, backup_id);
    fs::create_dir_all(&backup_root)?;
    let meta_path = backup_root.join(META_FILE);
    if meta_path.is_file() {
        return Ok(());
    }
    write_meta(
        &backup_root,
        BackupMeta {
            target_name: target_name.to_string(),
            created_at: timestamp_id(),
            entries: Vec::new(),
            created_paths: Vec::new(),
        },
    )
}

/// Copy `src` into an open backup session **before** it is overwritten.
pub fn backup_path_now(data_dir: &Path, backup_id: &str, src: &Path) -> anyhow::Result<()> {
    if !src.exists() {
        return Ok(());
    }
    let backup_root = backup_folder(data_dir, backup_id);
    let meta_path = backup_root.join(META_FILE);
    let mut meta: BackupMeta = if meta_path.is_file() {
        serde_json::from_str(&fs::read_to_string(&meta_path)?)?
    } else {
        anyhow::bail!("备份会话未初始化");
    };

    let original_key = normalize_path_key(src);
    if meta
        .entries
        .iter()
        .any(|e| normalize_path_key(Path::new(&e.original_path)) == original_key)
    {
        return Ok(());
    }

    let idx = meta.entries.len();
    let file_name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let backup_name = format!("{idx:04}-{file_name}");
    let dest = backup_root.join(&backup_name);
    if src.is_dir() {
        copy_dir_all(src, &dest)?;
    } else {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(src, &dest)?;
    }
    meta.entries.push(BackupEntry {
        original_path: src.to_string_lossy().to_string(),
        backup_name,
    });
    write_meta(&backup_root, meta)
}

/// Record files created by a migration so undo can remove them.
pub fn finalize_migration_backup(
    data_dir: &Path,
    backup_id: &str,
    created_paths: &[PathBuf],
) -> anyhow::Result<()> {
    if created_paths.is_empty() {
        return Ok(());
    }
    let backup_root = backup_folder(data_dir, backup_id);
    let meta_path = backup_root.join(META_FILE);
    let mut meta: BackupMeta = if meta_path.is_file() {
        serde_json::from_str(&fs::read_to_string(&meta_path)?)?
    } else {
        BackupMeta {
            target_name: backup_id.to_string(),
            created_at: timestamp_id(),
            entries: Vec::new(),
            created_paths: Vec::new(),
        }
    };

    let mut seen: HashSet<String> = meta.created_paths.iter().cloned().collect();
    for path in created_paths {
        let key = normalize_path_key(path);
        if seen.insert(key.clone()) {
            meta.created_paths.push(key);
        }
    }

    write_meta(&backup_root, meta)
}

fn write_meta(backup_root: &Path, meta: BackupMeta) -> anyhow::Result<()> {
    let meta_json = serde_json::to_string_pretty(&meta)?;
    fs::write(backup_root.join(META_FILE), meta_json)?;
    Ok(())
}

fn normalize_path_key(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    pub restored: u32,
    pub removed: u32,
    pub failed: u32,
    pub errors: Vec<String>,
}

pub fn restore_from_backup(data_dir: &Path, backup_id: &str) -> anyhow::Result<RestoreResult> {
    let backup_root = backup_folder(data_dir, backup_id);
    if !backup_root.is_dir() {
        anyhow::bail!("备份不存在: {}", backup_root.display());
    }
    let meta_path = backup_root.join(META_FILE);
    if !meta_path.is_file() {
        anyhow::bail!("此备份缺少元数据，无法自动撤销，请通过「打开备份」手动恢复");
    }
    let meta: BackupMeta = serde_json::from_str(&fs::read_to_string(&meta_path)?)?;

    let mut restored = 0u32;
    let mut removed = 0u32;
    let mut failed = 0u32;
    let mut errors = Vec::new();

    let mut touched_paths: HashSet<String> = HashSet::new();

    for entry in meta.entries {
        let src = backup_root.join(&entry.backup_name);
        let dst = PathBuf::from(&entry.original_path);
        touched_paths.insert(normalize_path_key(&dst));
        if !src.exists() {
            failed += 1;
            errors.push(format!("备份文件缺失: {}", entry.backup_name));
            continue;
        }
        match restore_entry(&src, &dst) {
            Ok(()) => restored += 1,
            Err(e) => {
                failed += 1;
                errors.push(format!("{}: {e}", entry.backup_name));
            }
        }
    }

    for path_str in meta.created_paths {
        if touched_paths.contains(&path_str) {
            continue;
        }
        let path = PathBuf::from(&path_str);
        match remove_created_path(&path) {
            Ok(true) => removed += 1,
            Ok(false) => {}
            Err(e) => {
                failed += 1;
                errors.push(format!(
                    "{}: {e}",
                    path.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or(path_str)
                ));
            }
        }
    }

    if restored == 0 && removed == 0 && failed == 0 {
        anyhow::bail!("此备份没有可撤销的内容");
    }

    Ok(RestoreResult {
        restored,
        removed,
        failed,
        errors,
    })
}

fn remove_created_path(path: &Path) -> anyhow::Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    if path.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(true)
}

fn restore_entry(src: &Path, dst: &Path) -> anyhow::Result<()> {
    if src.is_dir() {
        if dst.exists() {
            if dst.is_dir() {
                fs::remove_dir_all(dst)?;
            } else {
                fs::remove_file(dst)?;
            }
        }
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        copy_dir_all(src, dst)?;
    } else {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(src, dst)?;
    }
    Ok(())
}

fn copy_dir_all(src: &Path, dst: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

fn timestamp_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

pub struct BackupContext {
    pub backup_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn backup_and_restore_roundtrip() {
        let data_dir = std::env::temp_dir().join(format!(
            "mc-mod-migrator-backup-test-{}",
            timestamp_id()
        ));
        let target = data_dir.join("instance").join("shaderpacks");
        fs::create_dir_all(&target).unwrap();
        let original = target.join("pack.zip");
        fs::File::create(&original)
            .unwrap()
            .write_all(b"old content")
            .unwrap();

        let id = create_migration_backup(&data_dir, "Test", &[original.clone()]).unwrap();
        fs::write(&original, b"new content").unwrap();

        let result = restore_from_backup(&data_dir, &id).unwrap();
        assert_eq!(result.restored, 1);
        assert_eq!(result.removed, 0);
        assert_eq!(result.failed, 0);
        assert_eq!(fs::read_to_string(&original).unwrap(), "old content");

        let _ = fs::remove_dir_all(&data_dir);
    }

    #[test]
    fn undo_removes_created_files() {
        let data_dir = std::env::temp_dir().join(format!(
            "mc-mod-migrator-backup-created-{}",
            timestamp_id()
        ));
        let mods_dir = data_dir.join("instance").join("mods");
        fs::create_dir_all(&mods_dir).unwrap();
        let new_mod = mods_dir.join("new-mod.jar");
        fs::File::create(&new_mod)
            .unwrap()
            .write_all(b"fake jar")
            .unwrap();

        let id = create_migration_backup(&data_dir, "Test", &[]).unwrap();
        finalize_migration_backup(&data_dir, &id, &[new_mod.clone()]).unwrap();

        let result = restore_from_backup(&data_dir, &id).unwrap();
        assert_eq!(result.removed, 1);
        assert!(!new_mod.exists());

        let _ = fs::remove_dir_all(&data_dir);
    }
}
