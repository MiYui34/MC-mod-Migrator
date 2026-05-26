use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::backup::{
    backup_path_now, begin_incremental_backup, finalize_migration_backup, restore_from_backup,
};
use crate::models::MarketInstallRecord;

const LOG_FILE: &str = "market_installs.json";
const MAX_RECORDS: usize = 50;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarketInstallLog {
    records: Vec<MarketInstallRecord>,
}

fn log_path(data_dir: &Path) -> PathBuf {
    data_dir.join(LOG_FILE)
}

fn read_log(data_dir: &Path) -> Result<MarketInstallLog> {
    let path = log_path(data_dir);
    if !path.is_file() {
        return Ok(MarketInstallLog::default());
    }
    let text = fs::read_to_string(&path).context("read market install log")?;
    Ok(serde_json::from_str(&text).unwrap_or_default())
}

fn write_log(data_dir: &Path, log: &MarketInstallLog) -> Result<()> {
    let path = log_path(data_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(log)?;
    fs::write(path, json)?;
    Ok(())
}

pub fn list_recent_installs(data_dir: &Path, limit: usize) -> Result<Vec<MarketInstallRecord>> {
    let log = read_log(data_dir)?;
    Ok(log
        .records
        .into_iter()
        .filter(|r| !r.undone)
        .take(limit)
        .collect())
}

pub struct MarketInstallSession {
    pub record_id: String,
    data_dir: Option<PathBuf>,
    backup_id: Option<String>,
    backed_up_keys: HashSet<String>,
    created_paths: Vec<PathBuf>,
}

impl MarketInstallSession {
    pub fn new(
        record_id: String,
        data_dir: Option<&Path>,
        target_name: &str,
    ) -> Result<Self> {
        let backup_id = if let Some(dir) = data_dir {
            let id = format!("market-{}", record_id);
            begin_incremental_backup(dir, &id, target_name)?;
            Some(id)
        } else {
            None
        };

        Ok(Self {
            record_id,
            data_dir: data_dir.map(Path::to_path_buf),
            backup_id,
            backed_up_keys: HashSet::new(),
            created_paths: Vec::new(),
        })
    }

    pub fn has_changes(&self) -> bool {
        !self.created_paths.is_empty() || !self.backed_up_keys.is_empty()
    }

    /// Back up existing file/dir immediately, before it is overwritten.
    pub fn track_overwrite(&mut self, path: PathBuf) -> Result<()> {
        if !path.exists() {
            return Ok(());
        }
        let key = normalize_path_key(&path);
        if !self.backed_up_keys.insert(key) {
            return Ok(());
        }
        if let (Some(ref data_dir), Some(ref backup_id)) = (&self.data_dir, &self.backup_id) {
            backup_path_now(data_dir, backup_id, &path)?;
        }
        Ok(())
    }

    pub fn track_created(&mut self, path: PathBuf) {
        if !self.created_paths.iter().any(|p| p == &path) {
            self.created_paths.push(path);
        }
    }

    pub fn finalize(self, target_name: &str, summary: &str) -> Result<Option<MarketInstallRecord>> {
        if !self.has_changes() {
            return Ok(None);
        }

        let backup_id = if let (Some(ref data_dir), Some(ref id)) = (&self.data_dir, &self.backup_id)
        {
            finalize_migration_backup(data_dir, id, &self.created_paths)?;
            Some(id.clone())
        } else {
            None
        };

        let record = MarketInstallRecord {
            id: self.record_id.clone(),
            timestamp: chrono_like_timestamp(),
            target_name: target_name.to_string(),
            summary: summary.to_string(),
            files_created: self
                .created_paths
                .iter()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .collect(),
            backup_id,
            undone: false,
        };

        if let Some(ref data_dir) = self.data_dir {
            append_record(data_dir, record.clone())?;
        }
        Ok(Some(record))
    }
}

fn normalize_path_key(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn chrono_like_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

fn append_record(data_dir: &Path, record: MarketInstallRecord) -> Result<()> {
    let mut log = read_log(data_dir)?;
    log.records.insert(0, record);
    log.records.truncate(MAX_RECORDS);
    write_log(data_dir, &log)
}

pub fn undo_install(data_dir: &Path, record_id: &str) -> Result<super::backup::RestoreResult> {
    let mut log = read_log(data_dir)?;
    let record = log
        .records
        .iter_mut()
        .find(|r| r.id == record_id && !r.undone)
        .context("安装记录不存在或已撤销")?;

    let backup_id = record
        .backup_id
        .clone()
        .context("此安装无备份，无法自动撤销")?;

    let result = restore_from_backup(data_dir, &backup_id)?;
    record.undone = true;
    write_log(data_dir, &log)?;
    Ok(result)
}

pub fn new_record_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{ms}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn track_overwrite_backs_up_before_overwrite() {
        let data_dir = std::env::temp_dir().join(format!(
            "mc-market-undo-{}",
            new_record_id()
        ));
        let target_dir = data_dir.join("instance").join("mods");
        fs::create_dir_all(&target_dir).unwrap();
        let jar = target_dir.join("fabric-api.jar");
        fs::File::create(&jar)
            .unwrap()
            .write_all(b"old jar")
            .unwrap();

        let record_id = new_record_id();
        let mut session =
            MarketInstallSession::new(record_id.clone(), Some(&data_dir), "Test").unwrap();
        session.track_overwrite(jar.clone()).unwrap();
        fs::write(&jar, b"new jar").unwrap();
        session.track_created(jar.clone());
        let record = session
            .finalize("Test", "fabric-api.jar")
            .unwrap()
            .expect("record");

        let result = undo_install(&data_dir, &record.id).unwrap();
        assert_eq!(result.restored, 1);
        // 同一路径既被覆盖又被记为新建时，恢复优先于删除
        assert_eq!(result.removed, 0);
        assert_eq!(fs::read(&jar).unwrap(), b"old jar");

        let _ = fs::remove_dir_all(&data_dir);
    }
}
