use std::collections::HashMap;
use std::path::Path;

use crate::jar::parse_jar_metadata;
use crate::models::IdentifiedMod;
use crate::version::{extract_mc_version, normalize_mc_version, release_supports_target_mc};

pub const FABRIC_API_PROJECT_ID: &str = "P7dR8mSH";

pub fn is_fabric_api_mod(mod_info: &IdentifiedMod) -> bool {
    if mod_info
        .mod_id
        .as_deref()
        .is_some_and(|id| id.eq_ignore_ascii_case("fabric-api"))
    {
        return true;
    }
    if mod_info
        .project_id
        .as_deref()
        .is_some_and(is_fabric_api_project_ref)
    {
        return true;
    }
    is_fabric_api_jar_name(&mod_info.file_name)
}

pub fn is_fabric_api_project_ref(s: &str) -> bool {
    s.eq_ignore_ascii_case("fabric-api") || s == FABRIC_API_PROJECT_ID
}

fn is_fabric_api_jar_name(file_name: &str) -> bool {
    let lower = file_name.to_lowercase();
    lower.ends_with(".jar") && (lower.starts_with("fabric-api") || lower.contains("fabric-api-"))
}

/// Target mods folder already contains a Fabric API jar compatible with `target_mc`.
pub fn find_usable_fabric_api_in_target(mods_path: &str, target_mc: &str) -> Option<String> {
    let target_mc = normalize_mc_version(target_mc);
    if target_mc.is_empty() || target_mc == "unknown" {
        return None;
    }

    let dir = Path::new(mods_path);
    let entries = std::fs::read_dir(dir).ok()?;

    for entry in entries.flatten() {
        if !entry.path().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_fabric_api_jar_name(&name) {
            continue;
        }

        let mut game_versions = Vec::new();
        if let Some(mc) = extract_mc_version(&name) {
            game_versions.push(mc);
        }

        if game_versions.is_empty() {
            if let Ok(meta) = parse_jar_metadata(&entry.path()) {
                let is_api = meta
                    .mod_id
                    .as_deref()
                    .is_some_and(|id| id.eq_ignore_ascii_case("fabric-api"));
                if !is_api {
                    continue;
                }
                if let Some(v) = &meta.version {
                    if let Some(mc) = extract_mc_version(v) {
                        game_versions.push(mc);
                    }
                }
            }
        }

        if game_versions.is_empty() {
            continue;
        }

        if release_supports_target_mc(&game_versions, &target_mc) {
            return Some(name);
        }
    }

    None
}

/// Whether an existing jar in the target mods folder supports the target MC version.
pub fn target_jar_supports_mc(mods_path: &str, file_name: &str, target_mc: &str) -> bool {
    let path = Path::new(mods_path).join(file_name);
    if !path.is_file() {
        return false;
    }

    let mut game_versions = Vec::new();
    if let Some(mc) = extract_mc_version(file_name) {
        game_versions.push(mc);
    }
    if let Ok(meta) = parse_jar_metadata(&path) {
        if let Some(v) = &meta.version {
            if let Some(mc) = extract_mc_version(v) {
                game_versions.push(mc);
            }
        }
    }

    release_supports_target_mc(&game_versions, target_mc)
}

/// One pass over the target mods folder: fabric-api reuse + per-jar MC support.
pub struct TargetModsSnapshot {
    pub fabric_api_file: Option<String>,
    pub jar_mc_support: HashMap<String, bool>,
}

pub fn scan_target_mods_folder(mods_path: &str, target_mc: &str) -> TargetModsSnapshot {
    let target_mc = normalize_mc_version(target_mc);
    let mut snapshot = TargetModsSnapshot {
        fabric_api_file: None,
        jar_mc_support: HashMap::new(),
    };
    if target_mc.is_empty() || target_mc == "unknown" {
        return snapshot;
    }

    let dir = Path::new(mods_path);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return snapshot;
    };

    for entry in entries.flatten() {
        if !entry.path().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.to_lowercase().ends_with(".jar") {
            continue;
        }

        let mut game_versions = Vec::new();
        if let Some(mc) = extract_mc_version(&name) {
            game_versions.push(mc);
        }

        let needs_jar_parse = game_versions.is_empty()
            || (snapshot.fabric_api_file.is_none() && is_fabric_api_jar_name(&name));
        let meta = if needs_jar_parse {
            parse_jar_metadata(&entry.path()).ok()
        } else {
            None
        };

        if let Some(ref meta) = meta {
            if let Some(v) = &meta.version {
                if let Some(mc) = extract_mc_version(v) {
                    if !game_versions.iter().any(|g| g == &mc) {
                        game_versions.push(mc);
                    }
                }
            }
        }

        let supports = release_supports_target_mc(&game_versions, &target_mc);
        snapshot.jar_mc_support.insert(name.clone(), supports);

        if snapshot.fabric_api_file.is_none() && is_fabric_api_jar_name(&name) {
            let is_api = meta
                .as_ref()
                .and_then(|m| m.mod_id.as_deref())
                .is_some_and(|id| id.eq_ignore_ascii_case("fabric-api"))
                || is_fabric_api_jar_name(&name);
            if is_api && supports {
                snapshot.fabric_api_file = Some(name);
            }
        }
    }

    snapshot
}

/// Scan target mods once; map jar file name → supports target MC.
pub fn build_target_jar_mc_cache(mods_path: &str, target_mc: &str) -> HashMap<String, bool> {
    scan_target_mods_folder(mods_path, target_mc).jar_mc_support
}

pub fn target_jar_cached_supports(cache: &HashMap<String, bool>, file_name: &str) -> bool {
    cache.get(file_name).copied().unwrap_or(false)
}

/// Map fabric/forge mod id → jar filenames already in the target mods folder.
pub fn build_target_mod_id_index(mods_path: &str) -> HashMap<String, Vec<String>> {
    let mut index: HashMap<String, Vec<String>> = HashMap::new();
    let dir = Path::new(mods_path);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return index;
    };

    for entry in entries.flatten() {
        if !entry.path().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.to_lowercase().ends_with(".jar") {
            continue;
        }
        if let Ok(meta) = parse_jar_metadata(&entry.path()) {
            if let Some(mod_id) = meta.mod_id {
                index.entry(mod_id.to_lowercase()).or_default().push(name);
            }
        }
    }
    index
}

/// Remove older jars of the same mod from the target folder before writing the new file.
pub fn remove_conflicting_target_jars(
    mods_path: &str,
    mod_info: &IdentifiedMod,
    keep_file_name: &str,
) {
    let index = build_target_mod_id_index(mods_path);
    remove_conflicting_target_jars_with_index(mods_path, &index, mod_info, keep_file_name);
}

pub fn remove_conflicting_target_jars_with_index(
    mods_path: &str,
    index: &HashMap<String, Vec<String>>,
    mod_info: &IdentifiedMod,
    keep_file_name: &str,
) {
    let dir = Path::new(mods_path);
    let keep_lower = keep_file_name.to_lowercase();
    let remove = |name: &str| {
        if name.eq_ignore_ascii_case(keep_file_name) {
            return;
        }
        let path = dir.join(name);
        if path.is_file() {
            let _ = std::fs::remove_file(path);
        }
    };

    if mod_info.file_name.to_lowercase() != keep_lower {
        remove(&mod_info.file_name);
    }

    if let Some(mod_id) = &mod_info.mod_id {
        if let Some(names) = index.get(&mod_id.to_lowercase()) {
            for name in names {
                remove(name);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_fabric_api_mod() {
        let m = IdentifiedMod {
            file_name: "fabric-api-0.1+1.21.11.jar".into(),
            file_path: String::new(),
            sha512: String::new(),
            sha1: String::new(),
            fingerprint: 0,
            source: crate::models::ModSource::Modrinth,
            project_id: Some(FABRIC_API_PROJECT_ID.into()),
            curseforge_id: None,
            name: "Fabric API".into(),
            name_zh: None,
            mod_id: Some("fabric-api".into()),
            current_version: None,
            loaders: vec![],
            game_versions: vec![],
            icon_url: None,
            github_url: None,
            depends: vec![],
        };
        assert!(is_fabric_api_mod(&m));
    }
}
