use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::{params, Connection};
use serde_json;

use crate::models::{AppSettings, IdentifiedMod, ModSource};

pub struct AppDatabase {
    conn: Mutex<Connection>,
}

impl AppDatabase {
    pub fn new(data_dir: PathBuf) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&data_dir)?;
        let db_path = data_dir.join("cache.db");
        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS mod_cache (
                sha512 TEXT PRIMARY KEY,
                file_name TEXT NOT NULL,
                source TEXT NOT NULL,
                project_id TEXT,
                curseforge_id INTEGER,
                name TEXT NOT NULL,
                name_zh TEXT,
                mod_id TEXT,
                current_version TEXT,
                loaders TEXT NOT NULL,
                game_versions TEXT NOT NULL,
                icon_url TEXT,
                github_url TEXT,
                depends TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );
            ",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn get_cached_mod(&self, sha512: &str) -> anyhow::Result<Option<IdentifiedMod>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT file_name, source, project_id, curseforge_id, name, name_zh, mod_id,
                    current_version, loaders, game_versions, icon_url, github_url, depends
             FROM mod_cache WHERE sha512 = ?1",
        )?;
        let mut rows = stmt.query(params![sha512])?;
        if let Some(row) = rows.next()? {
            let source_str: String = row.get(1)?;
            let source = match source_str.as_str() {
                "modrinth" => ModSource::Modrinth,
                "curseforge" => ModSource::Curseforge,
                "metadata" => ModSource::Metadata,
                "github" => ModSource::Github,
                "sgu" => ModSource::Sgu,
                _ => ModSource::Unknown,
            };
            let loaders: Vec<String> = serde_json::from_str(&row.get::<_, String>(8)?)?;
            let game_versions: Vec<String> = serde_json::from_str(&row.get::<_, String>(9)?)?;
            let depends: Vec<String> = serde_json::from_str(&row.get::<_, String>(12)?)?;
            return Ok(Some(IdentifiedMod {
                file_name: row.get(0)?,
                file_path: String::new(),
                sha512: sha512.to_string(),
                sha1: String::new(),
                fingerprint: 0,
                source,
                project_id: row.get(2)?,
                curseforge_id: row.get(3)?,
                name: row.get(4)?,
                name_zh: row.get(5)?,
                mod_id: row.get(6)?,
                current_version: row.get(7)?,
                loaders,
                game_versions,
                icon_url: row.get(10)?,
                github_url: row.get(11)?,
                depends,
            }));
        }
        Ok(None)
    }

    pub fn cache_mod(&self, mod_info: &IdentifiedMod) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let source = match mod_info.source {
            ModSource::Modrinth => "modrinth",
            ModSource::Curseforge => "curseforge",
            ModSource::Metadata => "metadata",
            ModSource::Github => "github",
            ModSource::Sgu => "sgu",
            ModSource::Unknown => "unknown",
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;
        conn.execute(
            "INSERT OR REPLACE INTO mod_cache
             (sha512, file_name, source, project_id, curseforge_id, name, name_zh, mod_id,
              current_version, loaders, game_versions, icon_url, github_url, depends, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            params![
                mod_info.sha512,
                mod_info.file_name,
                source,
                mod_info.project_id,
                mod_info.curseforge_id,
                mod_info.name,
                mod_info.name_zh,
                mod_info.mod_id,
                mod_info.current_version,
                serde_json::to_string(&mod_info.loaders)?,
                serde_json::to_string(&mod_info.game_versions)?,
                mod_info.icon_url,
                mod_info.github_url,
                serde_json::to_string(&mod_info.depends)?,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn clear_cache(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM mod_cache", [])?;
        Ok(())
    }

    pub fn with_conn<F, T>(&self, f: F) -> anyhow::Result<T>
    where
        F: FnOnce(&Connection) -> anyhow::Result<T>,
    {
        let conn = self.conn.lock().unwrap();
        f(&conn)
    }
}

pub fn settings_path(data_dir: &PathBuf) -> PathBuf {
    data_dir.join("settings.json")
}

pub fn sanitize_settings(mut settings: AppSettings) -> AppSettings {
    if settings.download_source_priority.is_empty() {
        settings.download_source_priority = AppSettings::default().download_source_priority;
    }
    if !matches!(
        settings.mod_api_mirror.as_str(),
        "official" | "mcim" | "auto"
    ) {
        settings.mod_api_mirror = AppSettings::default().mod_api_mirror;
    }
    if !matches!(settings.mod_version_policy.as_str(), "auto" | "downgrade") {
        settings.mod_version_policy = AppSettings::default().mod_version_policy;
    }
    if !matches!(settings.mod_report_format.as_str(), "md" | "txt") {
        settings.mod_report_format = AppSettings::default().mod_report_format;
    }
    if !matches!(settings.update_mode.as_str(), "manual" | "auto") {
        settings.update_mode = AppSettings::default().update_mode;
    }
    if settings.update_check_interval_hours == 0 {
        settings.update_check_interval_hours = AppSettings::default().update_check_interval_hours;
    }
    settings
}

pub fn update_state_path(data_dir: &PathBuf) -> PathBuf {
    data_dir.join("update_state.json")
}

pub fn load_update_state(data_dir: &PathBuf) -> crate::models::UpdateState {
    let path = update_state_path(data_dir);
    if path.exists() {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(state) = serde_json::from_str(&content) {
                return state;
            }
        }
    }
    crate::models::UpdateState::default()
}

pub fn save_update_state(data_dir: &PathBuf, state: &crate::models::UpdateState) -> anyhow::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let content = serde_json::to_string_pretty(state)?;
    std::fs::write(update_state_path(data_dir), content)?;
    Ok(())
}

pub fn migration_presets_path(data_dir: &PathBuf) -> PathBuf {
    data_dir.join("migration_presets.json")
}

pub fn load_migration_presets(data_dir: &PathBuf) -> Vec<crate::models::MigrationPreset> {
    let path = migration_presets_path(data_dir);
    if path.exists() {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(presets) = serde_json::from_str(&content) {
                return presets;
            }
        }
    }
    Vec::new()
}

pub fn save_migration_presets(
    data_dir: &PathBuf,
    presets: &[crate::models::MigrationPreset],
) -> anyhow::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let content = serde_json::to_string_pretty(presets)?;
    std::fs::write(migration_presets_path(data_dir), content)?;
    Ok(())
}

pub fn load_settings(data_dir: &PathBuf) -> AppSettings {
    let path = settings_path(data_dir);
    if path.exists() {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(settings) = serde_json::from_str::<AppSettings>(&content) {
                return sanitize_settings(settings);
            }
        }
    }
    AppSettings::default()
}

pub fn save_settings(data_dir: &PathBuf, settings: &AppSettings) -> anyhow::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let content = serde_json::to_string_pretty(settings)?;
    std::fs::write(settings_path(data_dir), content)?;
    Ok(())
}

pub fn session_path(data_dir: &PathBuf) -> PathBuf {
    data_dir.join("session.json")
}

pub fn load_session(data_dir: &PathBuf) -> crate::models::AppSession {
    let path = session_path(data_dir);
    let mut session = if path.exists() {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default()
    } else {
        crate::models::AppSession::default()
    };
    refresh_session(&mut session);
    session
}

pub fn save_session(data_dir: &PathBuf, session: &crate::models::AppSession) -> anyhow::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let content = serde_json::to_string(session)?;
    std::fs::write(session_path(data_dir), content)?;
    Ok(())
}

fn refresh_session(session: &mut crate::models::AppSession) {
    if let Some(inst) = session.source_instance.as_mut() {
        refresh_instance_light(inst);
    }
    if let Some(inst) = session.target_instance.as_mut() {
        refresh_instance_light(inst);
    }
    if let Some(src) = session.source_instance.as_ref() {
        let mods_path = src.mods_path.clone();
        refresh_mod_file_paths(&mut session.mods, &mods_path);
        for item in &mut session.transfer_items {
            let m = &mut item.mod_info;
            refresh_one_mod_path(m, &mods_path);
        }
    }
}

/// Fast path refresh on startup — avoids re-detecting instances or parsing mod jars.
fn refresh_instance_light(inst: &mut crate::models::InstanceInfo) {
    if !std::path::Path::new(&inst.mods_path).is_dir() {
        return;
    }
    if let Some(game_dir) = std::path::Path::new(&inst.mods_path).parent() {
        if inst.game_dir.is_empty() || !std::path::Path::new(&inst.game_dir).is_dir() {
            inst.game_dir = game_dir.to_string_lossy().to_string();
        }
    }
    let (mc, loader, lv) = crate::instance::infer_from_mods_path(&inst.mods_path);
    if inst.mc_version.is_empty() || inst.mc_version == "unknown" {
        if mc != "unknown" {
            inst.mc_version = mc;
        }
    }
    if inst.loader.is_empty() || inst.loader == "unknown" {
        if loader != "unknown" {
            inst.loader = loader;
        }
    }
    if inst.loader_version.is_empty() && !lv.is_empty() {
        inst.loader_version = lv;
    }
}

fn refresh_mod_file_paths(mods: &mut [crate::models::IdentifiedMod], mods_path: &str) {
    for m in mods.iter_mut() {
        refresh_one_mod_path(m, mods_path);
    }
}

fn refresh_one_mod_path(m: &mut crate::models::IdentifiedMod, mods_path: &str) {
    let path = std::path::Path::new(mods_path).join(&m.file_name);
    if path.is_file() {
        m.file_path = path.to_string_lossy().to_string();
    }
}
