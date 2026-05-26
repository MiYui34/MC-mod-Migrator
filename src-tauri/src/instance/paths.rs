use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::models::InstanceInfo;

#[derive(Clone)]
pub struct InstancePaths {
    pub game_dir: PathBuf,
    pub shaderpacks: PathBuf,
    pub resourcepacks: PathBuf,
    pub datapacks: PathBuf,
    pub schematics: PathBuf,
    pub config: PathBuf,
}

const GAME_SETTINGS_NAMES: &[&str] = &["options.txt", "keybindings.txt", "optionsof.txt"];

pub fn resolve_game_dir(instance: &InstanceInfo) -> PathBuf {
    if !instance.game_dir.is_empty() {
        let p = PathBuf::from(&instance.game_dir);
        if p.is_dir() {
            return p;
        }
    }
    PathBuf::from(&instance.mods_path)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from(&instance.mods_path))
}

pub fn resolve_instance_paths(instance: &InstanceInfo) -> InstancePaths {
    let game_dir = resolve_game_dir(instance);
    InstancePaths {
        shaderpacks: game_dir.join("shaderpacks"),
        resourcepacks: game_dir.join("resourcepacks"),
        datapacks: game_dir.join("datapacks"),
        schematics: game_dir.join("schematics"),
        config: game_dir.join("config"),
        game_dir,
    }
}

/// Candidate schematic directories for an instance (default, custom, global fallback).
pub fn resolve_schematic_roots(instance: &InstanceInfo) -> Vec<PathBuf> {
    let game_dir = resolve_game_dir(instance);
    let mut roots = Vec::new();
    let mut seen = HashSet::new();

    let default = game_dir.join("schematics");
    roots.push(default.clone());
    seen.insert(normalize_path_key(&default));

    for custom in read_litematica_schematic_dirs(&game_dir.join("config")) {
        let key = normalize_path_key(&custom);
        if seen.insert(key) && custom.is_dir() {
            roots.push(custom);
        }
    }

    if let Some(mc_root) = find_dot_minecraft_root(&game_dir) {
        let global = mc_root.join("schematics");
        if global != default {
            let key = normalize_path_key(&global);
            if seen.insert(key) && global.is_dir() {
                roots.push(global);
            }
        }
    }

    roots
}

fn normalize_path_key(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_lowercase()
}

pub fn instance_game_dir_key(instance: &InstanceInfo) -> String {
    normalize_path_key(&resolve_game_dir(instance))
}

pub fn game_dir_key_from_mods_path(mods_path: &str) -> String {
    let instance = InstanceInfo {
        name: String::new(),
        mods_path: mods_path.to_string(),
        mc_version: String::new(),
        loader: String::new(),
        loader_version: String::new(),
        game_dir: String::new(),
        launcher: None,
    };
    instance_game_dir_key(&instance)
}

pub fn same_game_version_folder(a: &InstanceInfo, b: &InstanceInfo) -> bool {
    instance_game_dir_key(a) == instance_game_dir_key(b)
}

pub fn same_game_version_folder_as_mods_path(instance: &InstanceInfo, mods_path: &str) -> bool {
    instance_game_dir_key(instance) == game_dir_key_from_mods_path(mods_path)
}

pub fn ensure_distinct_migration_instances(
    source: &InstanceInfo,
    target: &InstanceInfo,
) -> anyhow::Result<()> {
    if same_game_version_folder(source, target) {
        anyhow::bail!("源实例与目标实例不能为同一游戏版本文件夹");
    }
    Ok(())
}

pub fn ensure_distinct_migration_source_target(
    source: &InstanceInfo,
    target_mods_path: &str,
) -> anyhow::Result<()> {
    if same_game_version_folder_as_mods_path(source, target_mods_path) {
        anyhow::bail!("源实例与目标实例不能为同一游戏版本文件夹");
    }
    Ok(())
}

fn find_dot_minecraft_root(game_dir: &Path) -> Option<PathBuf> {
    let mut current = game_dir.to_path_buf();
    loop {
        if current.file_name().and_then(|n| n.to_str()) == Some(".minecraft") {
            return Some(current);
        }
        if current.join("launcher_profiles.json").exists() {
            return Some(current);
        }
        if current
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            == Some("versions")
        {
            return current.parent().map(|p| p.to_path_buf());
        }
        current = current.parent()?.to_path_buf();
    }
}

fn read_litematica_schematic_dirs(config_dir: &Path) -> Vec<PathBuf> {
    let candidates = [
        config_dir.join("litematica.json"),
        config_dir.join("litematica").join("litematica.json"),
    ];
    let mut dirs = Vec::new();
    for path in candidates {
        if let Some(dir) = parse_litematica_schematic_dir(&path) {
            dirs.push(dir);
        }
    }
    dirs
}

fn parse_litematica_schematic_dir(path: &Path) -> Option<PathBuf> {
    let content = fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    let generic = json.get("Generic")?;

    if generic
        .get("customSchematicBaseDirectoryEnabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        if let Some(dir) = generic
            .get("customSchematicBaseDirectory")
            .and_then(|v| v.as_str())
        {
            let p = PathBuf::from(dir);
            if p.is_dir() {
                return Some(p);
            }
        }
    }

    if let Some(obj) = generic.get("customSchematicDirectory") {
        if obj.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false) {
            for key in ["directory", "value", "path"] {
                if let Some(dir) = obj.get(key).and_then(|v| v.as_str()) {
                    let p = PathBuf::from(dir);
                    if p.is_dir() {
                        return Some(p);
                    }
                }
            }
        }
    }

    None
}

pub fn is_schematic_file_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(".litematic")
        || lower.ends_with(".schem")
        || lower.ends_with(".schematic")
}

/// Game settings files that exist under `game_dir`.
pub fn game_settings_files(game_dir: &Path) -> Vec<PathBuf> {
    GAME_SETTINGS_NAMES
        .iter()
        .map(|name| game_dir.join(name))
        .filter(|p| p.is_file())
        .collect()
}

/// `.txt` shader settings under `shaderpacks/` (OptiFine/Iris companion files & in-folder configs).
/// Returns `(relative_path from shaderpacks, absolute path, is_directory)`.
pub fn shader_pack_txt_settings(shaderpacks: &Path) -> Vec<(String, PathBuf, bool)> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    if !shaderpacks.is_dir() {
        return out;
    }

    let Ok(read) = fs::read_dir(shaderpacks) else {
        return out;
    };

    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if path.is_file() && name.to_lowercase().ends_with(".txt") {
            push_shader_txt(&mut out, &mut seen, name, path, false);
        } else if path.is_dir() {
            scan_shader_folder_txt(shaderpacks, &path, &mut out, &mut seen);
        }
    }

    out.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    out
}

fn push_shader_txt(
    out: &mut Vec<(String, PathBuf, bool)>,
    seen: &mut HashSet<String>,
    relative: String,
    path: PathBuf,
    is_dir: bool,
) {
    let key = relative.replace('\\', "/").to_lowercase();
    if seen.insert(key) {
        out.push((relative, path, is_dir));
    }
}

fn scan_shader_folder_txt(
    shaderpacks: &Path,
    dir: &Path,
    out: &mut Vec<(String, PathBuf, bool)>,
    seen: &mut HashSet<String>,
) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let relative = path
            .strip_prefix(shaderpacks)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or(name.clone());
        if path.is_file() && name.to_lowercase().ends_with(".txt") {
            push_shader_txt(out, seen, relative, path, false);
        } else if path.is_dir() {
            scan_shader_folder_txt(shaderpacks, &path, out, seen);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn same_game_version_folder_detects_mods_and_game_dir() {
        let a = InstanceInfo {
            name: "a".into(),
            mods_path: r"C:\mc\versions\1.21.4-fabric\mods".into(),
            mc_version: "1.21.4".into(),
            loader: "fabric".into(),
            loader_version: String::new(),
            game_dir: r"C:\mc\versions\1.21.4-fabric".into(),
            launcher: None,
        };
        let b = InstanceInfo {
            name: "b".into(),
            mods_path: r"C:\mc\versions\1.21.4-fabric\mods".into(),
            mc_version: "1.21.4".into(),
            loader: "fabric".into(),
            loader_version: String::new(),
            game_dir: String::new(),
            launcher: None,
        };
        let c = InstanceInfo {
            name: "c".into(),
            mods_path: r"C:\mc\versions\1.21.4-forge\mods".into(),
            mc_version: "1.21.4".into(),
            loader: "forge".into(),
            loader_version: String::new(),
            game_dir: String::new(),
            launcher: None,
        };
        assert!(same_game_version_folder(&a, &b));
        assert!(!same_game_version_folder(&a, &c));
        assert!(same_game_version_folder_as_mods_path(&a, r"C:\mc\versions\1.21.4-fabric\mods"));
    }

    #[test]
    fn game_dir_from_mods_parent() {
        let inst = InstanceInfo {
            name: "test".into(),
            mods_path: r"C:\game\mods".into(),
            mc_version: "1.21.4".into(),
            loader: "fabric".into(),
            loader_version: String::new(),
            game_dir: String::new(),
            launcher: None,
        };
        let paths = resolve_instance_paths(&inst);
        assert_eq!(paths.game_dir, PathBuf::from(r"C:\game"));
        assert_eq!(paths.datapacks, PathBuf::from(r"C:\game\datapacks"));
    }

    #[test]
    fn schematic_file_extensions() {
        assert!(is_schematic_file_name("build.litematic"));
        assert!(is_schematic_file_name("build.schem"));
        assert!(is_schematic_file_name("build.schematic"));
        assert!(!is_schematic_file_name("readme.txt"));
    }

    #[test]
    fn shader_pack_txt_settings_finds_companion_and_nested() {
        let base = std::env::temp_dir().join(format!("shader-txt-{}", std::process::id()));
        let shaderpacks = base.join("shaderpacks");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(shaderpacks.join("BSL/shaders")).unwrap();
        fs::File::create(shaderpacks.join("Complementary.zip"))
            .unwrap()
            .write_all(b"zip")
            .unwrap();
        fs::File::create(shaderpacks.join("Complementary.txt"))
            .unwrap()
            .write_all(b"settings")
            .unwrap();
        fs::File::create(shaderpacks.join("BSL/shaders/settings.txt"))
            .unwrap()
            .write_all(b"nested")
            .unwrap();

        let found = shader_pack_txt_settings(&shaderpacks);
        assert!(found.iter().any(|(r, _, _)| r == "Complementary.txt"));
        assert!(found.iter().any(|(r, _, _)| r == "BSL/shaders/settings.txt"));
        assert!(!found.iter().any(|(r, _, _)| r.ends_with(".zip")));

        let _ = fs::remove_dir_all(&base);
    }
}
