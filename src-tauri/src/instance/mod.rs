use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::Value;

use crate::models::{InstanceInfo, TargetEnv};

pub mod paths;
pub use paths::{
    ensure_distinct_migration_instances, ensure_distinct_migration_source_target,
    game_settings_files, is_schematic_file_name, resolve_game_dir, resolve_instance_paths,
    resolve_schematic_roots, shader_pack_txt_settings, InstancePaths,
};

/// Try to infer MC version, loader and loader version from an instance path
/// (version folder, game dir, or mods folder).
pub fn infer_from_mods_path(path_str: &str) -> (String, String, String) {
    let path = PathBuf::from(path_str);
    if let Some(version_dir) = resolve_version_dir(&path) {
        let inferred = infer_from_version_dir(&version_dir);
        if inferred.0 != "unknown" {
            return inferred;
        }
    }

    let game_dir = if path_is_mods_dir(&path) {
        path.parent().map(|p| p.to_path_buf())
    } else if path.join("mods").is_dir() || is_version_instance_dir(&path) {
        Some(path.clone())
    } else {
        None
    };

    if let Some(game_dir) = game_dir {
        let mods = if path_is_mods_dir(&path) {
            path.clone()
        } else {
            game_dir.join("mods")
        };
        return infer_from_game_context(&game_dir, &mods);
    }

    ("unknown".to_string(), "unknown".to_string(), String::new())
}

pub fn detect_from_path(path: &str) -> anyhow::Result<InstanceInfo> {
    let path_buf = PathBuf::from(path);

    if path_is_mods_dir(&path_buf) {
        return detect_from_mods_folder(&path_buf);
    }

    if is_version_instance_dir(&path_buf) {
        return detect_from_version_dir(&path_buf);
    }

    if path_buf.join("mods").is_dir() {
        return detect_from_game_dir(&path_buf);
    }

    detect_from_game_dir(&path_buf)
}

fn is_version_instance_dir(path: &Path) -> bool {
    if path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case("versions"))
    {
        return true;
    }
    if let Some(version_id) = path.file_name().and_then(|n| n.to_str()) {
        if path.join(format!("{version_id}.json")).exists() {
            return true;
        }
    }
    path.join("PCL").join("Setup.ini").exists()
}

fn resolve_version_dir(path: &Path) -> Option<PathBuf> {
    if path_is_mods_dir(path) {
        return path
            .parent()
            .filter(|parent| is_version_instance_dir(parent))
            .map(|p| p.to_path_buf());
    }
    if is_version_instance_dir(path) {
        return Some(path.to_path_buf());
    }
    None
}

fn infer_from_version_dir(version_dir: &Path) -> (String, String, String) {
    let version_id = version_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    if let Some(pcl) = read_from_pcl_setup_ini(version_dir) {
        return pcl;
    }

    let json_path = version_dir.join(format!("{version_id}.json"));
    if json_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&json_path) {
            return parse_version_id_and_json(version_id, &content);
        }
    }

    let (mc, loader) = parse_version_id_string(version_id);
    let lv = extract_loader_version(version_id, &loader);
    (mc, loader, lv)
}

/// Fill unknown MC/loader fields from the selected folder layout (version dir or mods).
pub fn enrich_target_env(target: TargetEnv) -> TargetEnv {
    if target.mods_path.trim().is_empty() {
        return target;
    }

    if let Ok(info) = detect_from_path(&target.mods_path) {
        return TargetEnv {
            mods_path: info.mods_path,
            mc_version: coalesce_known(&target.mc_version, &info.mc_version),
            loader: coalesce_known(&target.loader, &info.loader),
            loader_version: if target.loader_version.is_empty() {
                info.loader_version
            } else {
                target.loader_version
            },
        };
    }

    let (mc, loader, lv) = infer_from_mods_path(&target.mods_path);
    TargetEnv {
        mc_version: coalesce_known(&target.mc_version, &mc),
        loader: coalesce_known(&target.loader, &loader),
        loader_version: if target.loader_version.is_empty() {
            lv
        } else {
            target.loader_version
        },
        ..target
    }
}

fn coalesce_known(preferred: &str, fallback: &str) -> String {
    if !preferred.is_empty() && preferred != "unknown" {
        preferred.to_string()
    } else if !fallback.is_empty() && fallback != "unknown" {
        fallback.to_string()
    } else {
        preferred.to_string()
    }
}

fn path_is_mods_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.eq_ignore_ascii_case("mods"))
        .unwrap_or(false)
}

fn detect_from_mods_folder(mods_path: &Path) -> anyhow::Result<InstanceInfo> {
    let mods_path = if path_is_mods_dir(mods_path) {
        mods_path.to_path_buf()
    } else {
        mods_path.join("mods")
    };

    let game_dir = mods_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| mods_path.clone());

    if let Some(vp) = game_dir.parent() {
        if vp.file_name().and_then(|n| n.to_str()) == Some("versions") {
            if let Some(version_id) = game_dir.file_name().and_then(|n| n.to_str()) {
                return parse_version_json(&game_dir, version_id, &mods_path);
            }
        }
    }

    let (mc_version, loader, loader_version) = infer_from_game_context(&game_dir, &mods_path);

    Ok(InstanceInfo {
        name: game_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("手动选择")
            .to_string(),
        mods_path: mods_path.to_string_lossy().to_string(),
        mc_version,
        loader,
        loader_version,
        game_dir: game_dir.to_string_lossy().to_string(),
        launcher: None,
    })
}

fn detect_from_game_dir(game_dir: &Path) -> anyhow::Result<InstanceInfo> {
    let mods_path = game_dir.join("mods");
    std::fs::create_dir_all(&mods_path)?;
    detect_from_mods_folder(&mods_path)
}

fn detect_from_version_dir(version_dir: &Path) -> anyhow::Result<InstanceInfo> {
    let version_id = version_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    let mods_path = version_dir.join("mods");
    std::fs::create_dir_all(&mods_path)?;
    parse_version_json(version_dir, version_id, &mods_path)
}

fn parse_version_json(
    version_dir: &Path,
    version_id: &str,
    mods_path: &Path,
) -> anyhow::Result<InstanceInfo> {
    let (mc_version, loader, loader_version) = infer_from_version_dir(version_dir);

    Ok(InstanceInfo {
        name: version_id.to_string(),
        mods_path: mods_path.to_string_lossy().to_string(),
        mc_version,
        loader,
        loader_version,
        game_dir: version_dir.to_string_lossy().to_string(),
        launcher: None,
    })
}

fn read_from_pcl_setup_ini(version_dir: &Path) -> Option<(String, String, String)> {
    let setup = version_dir.join("PCL").join("Setup.ini");
    let content = std::fs::read_to_string(setup).ok()?;
    let mut mc_version = None;
    let mut loader = "unknown".to_string();
    let mut loader_version = String::new();

    for line in content.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("VersionVanillaName:") {
            let v = v.trim();
            if !v.is_empty() {
                mc_version = Some(crate::version::normalize_mc_version(v));
            }
        }
        if let Some(v) = line.strip_prefix("VersionFabric:") {
            let v = v.trim();
            if !v.is_empty() {
                loader = "fabric".to_string();
                loader_version = v.to_string();
            }
        }
        if let Some(v) = line.strip_prefix("VersionForge:") {
            let v = v.trim();
            if !v.is_empty() {
                loader = "forge".to_string();
                loader_version = v.to_string();
            }
        }
        if let Some(v) = line.strip_prefix("VersionNeoForge:") {
            let v = v.trim();
            if !v.is_empty() {
                loader = "neoforge".to_string();
                loader_version = v.to_string();
            }
        }
        if let Some(info) = line.strip_prefix("Info:") {
            let lower = info.to_lowercase();
            if loader == "unknown" {
                if lower.contains("fabric") {
                    loader = "fabric".to_string();
                } else if lower.contains("neoforge") {
                    loader = "neoforge".to_string();
                } else if lower.contains("forge") {
                    loader = "forge".to_string();
                }
            }
            if mc_version.is_none() {
                mc_version = extract_mc_version_from_text(info);
            }
            if loader_version.is_empty() {
                loader_version = extract_loader_version(info, &loader);
            }
        }
    }

    if loader_version.is_empty() {
        if let Some(version_id) = version_dir.file_name().and_then(|n| n.to_str()) {
            loader_version = extract_loader_version(version_id, &loader);
        }
    }

    mc_version.map(|mc| (mc, loader, loader_version))
}

fn infer_from_game_context(game_dir: &Path, mods_path: &Path) -> (String, String, String) {
    if let Some((mc, loader, lv)) = read_from_launcher_profiles(game_dir) {
        if mc != "unknown" {
            return (mc, loader, lv);
        }
    }

    if let Some((mc, loader, lv)) = read_from_hmcl_version_cfg(game_dir) {
        if mc != "unknown" {
            return (mc, loader, lv);
        }
    }

    if let Some((mc, loader, lv)) = infer_from_versions_dir(game_dir) {
        if mc != "unknown" {
            return (mc, loader, lv);
        }
    }

    let (mc, loader) = infer_from_mods(mods_path);
    (mc, loader, String::new())
}

fn read_from_launcher_profiles(game_dir: &Path) -> Option<(String, String, String)> {
    let path = game_dir.join("launcher_profiles.json");
    let content = std::fs::read_to_string(path).ok()?;
    let json: Value = serde_json::from_str(&content).ok()?;

    let profiles = json.get("profiles")?.as_object()?;
    let selected = json
        .get("selectedProfile")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let profile = profiles.get(selected).or_else(|| profiles.values().next())?;
    let version_id = profile.get("lastVersionId")?.as_str()?;

    if let Some(game_dir_str) = profile.get("gameDir").and_then(|v| v.as_str()) {
        let expected = game_dir.to_string_lossy().to_string();
        let normalized_profile = normalize_path(game_dir_str);
        let normalized_expected = normalize_path(&expected);
        if !normalized_profile.is_empty()
            && !normalized_expected.is_empty()
            && normalized_profile != normalized_expected
        {
            return None;
        }
    }

    let (mc_version, loader) = parse_version_id_string(version_id);
    let loader_version = extract_loader_version(version_id, &loader);
    Some((mc_version, loader, loader_version))
}

fn read_from_hmcl_version_cfg(game_dir: &Path) -> Option<(String, String, String)> {
    let versions_dir = game_dir.join("versions");
    if !versions_dir.is_dir() {
        return None;
    }

    let mut best: Option<(String, String, String)> = None;
    if let Ok(entries) = std::fs::read_dir(&versions_dir) {
        for entry in entries.flatten() {
            let version_dir = entry.path();
            if !version_dir.is_dir() {
                continue;
            }
            let cfg = version_dir.join("hmclversion.cfg");
            if !cfg.exists() {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&cfg) {
                if let Ok(json) = serde_json::from_str::<Value>(&content) {
                    let version_id = version_dir
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("");
                    let parsed = parse_version_id_and_json(version_id, &content);
                    if parsed.0 != "unknown" {
                        best = Some(parsed);
                    } else if let Some(id) = json.get("id").and_then(|v| v.as_str()) {
                        let (mc, loader) = parse_version_id_string(id);
                        let lv = extract_loader_version(id, &loader);
                        best = Some((mc, loader, lv));
                    }
                }
            }
        }
    }
    best
}

fn infer_from_versions_dir(game_dir: &Path) -> Option<(String, String, String)> {
    let versions_dir = game_dir.join("versions");
    if !versions_dir.is_dir() {
        return None;
    }

    let mut candidates: Vec<(String, String, String)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&versions_dir) {
        for entry in entries.flatten() {
            let version_dir = entry.path();
            if !version_dir.is_dir() {
                continue;
            }
            let version_id = version_dir.file_name()?.to_str()?.to_string();
            let json_path = version_dir.join(format!("{version_id}.json"));
            let (mc, loader, loader_version) = if json_path.exists() {
                let content = std::fs::read_to_string(&json_path).ok()?;
                parse_version_id_and_json(&version_id, &content)
            } else {
                let (mc, loader) = parse_version_id_string(&version_id);
                let lv = extract_loader_version(&version_id, &loader);
                (mc, loader, lv)
            };
            if mc != "unknown" {
                candidates.push((mc, loader, loader_version));
            }
        }
    }

    candidates.pop()
}

fn parse_version_id_and_json(version_id: &str, json_content: &str) -> (String, String, String) {
    let (mut mc_version, mut loader) = parse_version_id_string(version_id);
    let mut loader_version = extract_loader_version(version_id, &loader);

    if let Ok(json) = serde_json::from_str::<Value>(json_content) {
        if let Some(id) = json.get("id").and_then(|v| v.as_str()) {
            let (mv, ld) = parse_version_id_string(id);
            if mv != "unknown" {
                mc_version = mv;
            }
            if ld != "unknown" {
                loader = ld;
            }
            if loader_version.is_empty() {
                loader_version = extract_loader_version(id, &loader);
            }
        }

        if let Some(inherits) = json.get("inheritsFrom").and_then(|v| v.as_str()) {
            if let Some(v) = extract_mc_version_from_text(inherits) {
                mc_version = v;
            } else if !inherits.is_empty() {
                mc_version = inherits.to_string();
            }
        }

        if mc_version == "unknown" {
            if let Some(v) = extract_mc_from_libraries(&json) {
                mc_version = v;
            }
        }

        if loader == "unknown" {
            loader = infer_loader_from_json(&json);
        }

        if loader_version.is_empty() {
            loader_version = extract_loader_version_from_json(&json, &loader);
        }
    }

    (mc_version, loader, loader_version)
}

fn extract_mc_from_libraries(json: &Value) -> Option<String> {
    let libraries = json.get("libraries")?.as_array()?;
    for lib in libraries {
        let name = lib
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if let Some(v) = extract_mc_from_maven_name(name) {
            return Some(v);
        }
    }
    None
}

fn extract_mc_from_maven_name(name: &str) -> Option<String> {
    // com.mojang:minecraft:1.21.4
    // net.minecraft:client:1.21.4:client
    let parts: Vec<&str> = name.split(':').collect();
    for part in parts {
        if let Some(v) = extract_mc_version_from_text(part) {
            return Some(v);
        }
    }
    None
}

fn extract_mc_version_from_text(text: &str) -> Option<String> {
    let re = Regex::new(r"1\.\d+(?:\.\d+)?").ok()?;
    re.find_iter(text).map(|m| m.as_str().to_string()).last()
}

fn extract_loader_version(text: &str, loader: &str) -> String {
    if loader == "unknown" {
        return String::new();
    }
    let lower = text.to_lowercase();
    let patterns: &[&str] = match loader {
        "fabric" => &[
            r"fabric-loader[-_]?(\d+\.\d+(?:\.\d+)?(?:\.\d+)?)",
            r"fabric[^\d]*(\d+\.\d+(?:\.\d+)?(?:\.\d+)?)",
        ],
        "forge" => &[
            r"forge[-_]?(\d+\.\d+(?:\.\d+)?(?:\.\d+)?)",
            r"minecraftforge:forge:(\d+\.\d+(?:\.\d+)?(?:\.\d+)?)",
        ],
        "neoforge" => &[
            r"neoforge[-_]?(\d+\.\d+(?:\.\d+)?(?:\.\d+)?)",
            r"net\.neoforged:neoforge:(\d+\.\d+(?:\.\d+)?(?:\.\d+)?)",
        ],
        "quilt" => &[
            r"quilt-loader[-_]?(\d+\.\d+(?:\.\d+)?(?:\.\d+)?)",
            r"quilt[^\d]*(\d+\.\d+(?:\.\d+)?(?:\.\d+)?)",
        ],
        _ => return String::new(),
    };

    for pat in patterns {
        if let Ok(re) = Regex::new(pat) {
            if let Some(caps) = re.captures(&lower) {
                if let Some(m) = caps.get(1) {
                    let ver = m.as_str().to_string();
                    if !ver.starts_with("1.21") && !ver.starts_with("1.20") && !ver.starts_with("1.19") {
                        return ver;
                    }
                    if pat.contains("loader") || pat.contains("forge") {
                        return ver;
                    }
                }
            }
        }
    }

    String::new()
}

fn extract_loader_version_from_json(json: &Value, loader: &str) -> String {
    if loader == "unknown" {
        return String::new();
    }
    if let Some(libraries) = json.get("libraries").and_then(|v| v.as_array()) {
        for lib in libraries {
            let name = lib
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let ver = extract_loader_version_from_maven(name, loader);
            if !ver.is_empty() {
                return ver;
            }
        }
    }
    if let Some(id) = json.get("id").and_then(|v| v.as_str()) {
        return extract_loader_version(id, loader);
    }
    String::new()
}

fn extract_loader_version_from_maven(name: &str, loader: &str) -> String {
    let lower = name.to_lowercase();
    let marker = match loader {
        "fabric" => "fabric-loader:",
        "quilt" => "quilt-loader:",
        "forge" => "minecraftforge:forge:",
        "neoforge" => "neoforge:",
        _ => return String::new(),
    };
    if let Some(idx) = lower.find(marker) {
        let rest = &lower[idx + marker.len()..];
        let ver: String = rest
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if !ver.is_empty() {
            return ver;
        }
    }
    extract_loader_version(name, loader)
}

fn infer_loader_from_json(json: &Value) -> String {
    if let Some(libraries) = json.get("libraries").and_then(|v| v.as_array()) {
        for lib in libraries {
            let name = lib
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_lowercase();
            if name.contains("fabric-loader") || name.contains(":fabric-loader:") {
                return "fabric".to_string();
            }
            if name.contains("quilt-loader") {
                return "quilt".to_string();
            }
            if name.contains("neoforge") {
                return "neoforge".to_string();
            }
            if name.contains(":forge:") || name.contains("minecraftforge") || name.contains("modlauncher") {
                return "forge".to_string();
            }
        }
    }

    if let Some(main_class) = json.get("mainClass").and_then(|v| v.as_str()) {
        let lower = main_class.to_lowercase();
        if lower.contains("fabric") {
            return "fabric".to_string();
        }
        if lower.contains("quilt") {
            return "quilt".to_string();
        }
        if lower.contains("forge") || lower.contains("modlauncher") {
            return "forge".to_string();
        }
    }

    if let Some(id) = json.get("id").and_then(|v| v.as_str()) {
        let (_, loader) = parse_version_id_string(id);
        if loader != "unknown" {
            return loader;
        }
    }

    "unknown".to_string()
}

fn parse_version_id_string(version_id: &str) -> (String, String) {
    let mc_version = extract_mc_version_from_text(version_id).unwrap_or_else(|| "unknown".to_string());

    let lower = version_id.to_lowercase();
    let loader = if lower.contains("neoforge") {
        "neoforge"
    } else if lower.contains("fabric") {
        "fabric"
    } else if lower.contains("quilt") {
        "quilt"
    } else if lower.contains("forge") {
        "forge"
    } else {
        "unknown"
    };

    (mc_version, loader.to_string())
}

fn infer_from_mods(mods_path: &Path) -> (String, String) {
    if !mods_path.exists() {
        return ("unknown".to_string(), "unknown".to_string());
    }
    let Ok(entries) = std::fs::read_dir(mods_path) else {
        return ("unknown".to_string(), "unknown".to_string());
    };

    let mut loader_hint = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jar") {
            continue;
        }
        if let Ok(meta) = crate::jar::parse_jar_metadata(&path) {
            if let Some(loader) = meta.loader_hint {
                loader_hint = Some(loader);
            }
            if let Some(version) = meta.version.as_ref().and_then(|v| extract_mc_version_from_text(v)) {
                return (version, loader_hint.unwrap_or_else(|| "unknown".to_string()));
            }
        }
    }
    (
        "unknown".to_string(),
        loader_hint.unwrap_or_else(|| "unknown".to_string()),
    )
}

fn normalize_path(path: &str) -> String {
    PathBuf::from(path)
        .to_string_lossy()
        .replace('\\', "/")
        .to_lowercase()
}

pub fn scan_launcher_instances() -> Vec<InstanceInfo> {
    let mut instances = Vec::new();
    let mut seen_paths = std::collections::HashSet::new();

    scan_hmcl_global_config(&mut instances, &mut seen_paths);
    scan_hmcl_portable_configs(&mut instances, &mut seen_paths);
    scan_pcl_instances(&mut instances, &mut seen_paths);

    for dot_minecraft in common_minecraft_roots() {
        let launcher = if dot_minecraft.join("PCL.ini").exists() {
            "PCL2"
        } else {
            "manual"
        };
        scan_dot_minecraft(&dot_minecraft, &mut instances, &mut seen_paths, launcher);
    }

    instances
}

fn common_minecraft_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".minecraft"));
    }
    if let Some(data) = dirs::data_dir() {
        roots.push(data.join(".minecraft"));
    }

    for drive in ["C:", "D:", "E:", "F:"] {
        roots.push(PathBuf::from(format!("{drive}\\.minecraft")));
        roots.push(PathBuf::from(format!("{drive}\\Minecraft\\.minecraft")));
        roots.push(PathBuf::from(format!("{drive}\\Games\\.minecraft")));
    }

    roots.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));
    roots.dedup_by(|a, b| normalize_path(&a.to_string_lossy()) == normalize_path(&b.to_string_lossy()));
    roots.retain(|p| p.is_dir());
    roots
}

fn scan_dot_minecraft(
    dot_minecraft: &Path,
    instances: &mut Vec<InstanceInfo>,
    seen: &mut std::collections::HashSet<String>,
    launcher: &str,
) {
    push_instance(
        instances,
        seen,
        detect_from_game_dir(dot_minecraft).ok().map(|mut i| {
            i.launcher = Some(launcher.to_string());
            i.name = ".minecraft (全局)".to_string();
            i
        }),
    );

    let versions_dir = dot_minecraft.join("versions");
    if versions_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&versions_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    push_instance(
                        instances,
                        seen,
                        detect_from_version_dir(&path).ok().map(|mut i| {
                            i.launcher = Some(launcher.to_string());
                            i
                        }),
                    );
                }
            }
        }
    }
}

fn scan_hmcl_global_config(instances: &mut Vec<InstanceInfo>, seen: &mut std::collections::HashSet<String>) {
    let Some(home) = dirs::home_dir() else {
        return;
    };

    let config_paths = [
        home.join(".hmcl").join("config.json"),
        home.join(".hmcl").join("hmcl.json"),
    ];

    for config_path in config_paths {
        if config_path.exists() {
            scan_hmcl_config_file(&config_path, instances, seen);
        }
    }

    for appdata in [dirs::data_dir(), dirs::data_local_dir()] {
        if let Some(appdata) = appdata {
            let appdata_config = appdata.join(".hmcl").join("config.json");
            if appdata_config.exists() {
                scan_hmcl_config_file(&appdata_config, instances, seen);
            }
        }
    }
}

fn scan_hmcl_portable_configs(instances: &mut Vec<InstanceInfo>, seen: &mut std::collections::HashSet<String>) {
    let search_roots = [
        dirs::home_dir(),
        dirs::data_local_dir(),
        Some(PathBuf::from("D:\\")),
        Some(PathBuf::from("E:\\")),
        Some(PathBuf::from("C:\\Games")),
        Some(PathBuf::from("D:\\Games")),
        Some(PathBuf::from("D:\\Minecraft")),
        Some(PathBuf::from("C:\\Minecraft")),
    ];

    for root in search_roots.into_iter().flatten() {
        if !root.exists() {
            continue;
        }
        scan_hmcl_json_in_dir(&root, 3, instances, seen);
    }
}

fn scan_hmcl_json_in_dir(
    dir: &Path,
    depth: u32,
    instances: &mut Vec<InstanceInfo>,
    seen: &mut std::collections::HashSet<String>,
) {
    if depth == 0 {
        return;
    }

    for name in ["hmcl.json", ".hmcl.json"] {
        let config = dir.join(name);
        if config.exists() {
            scan_hmcl_config_file(&config, instances, seen);
        }
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if dir_name.starts_with('.')
                || dir_name.eq_ignore_ascii_case("node_modules")
                || dir_name.eq_ignore_ascii_case("target")
                || dir_name.eq_ignore_ascii_case("Windows")
            {
                continue;
            }
            scan_hmcl_json_in_dir(&path, depth - 1, instances, seen);
        }
    }
}

fn scan_hmcl_config_file(
    config_path: &Path,
    instances: &mut Vec<InstanceInfo>,
    seen: &mut std::collections::HashSet<String>,
) {
    let Ok(content) = std::fs::read_to_string(config_path) else {
        return;
    };
    let Ok(json) = serde_json::from_str::<Value>(&content) else {
        return;
    };

    let Some(profiles) = json.get("configurations").and_then(|v| v.as_object()) else {
        return;
    };

    for (name, profile) in profiles {
        let game_dir = profile
            .get("gameDir")
            .and_then(|v| v.as_str())
            .map(PathBuf::from);
        let Some(game_dir) = game_dir else {
            continue;
        };
        if !game_dir.exists() {
            continue;
        }
        push_instance(
            instances,
            seen,
            detect_from_game_dir(&game_dir).ok().map(|mut i| {
                i.launcher = Some("HMCL".to_string());
                i.name = format!("HMCL · {name}");
                i
            }),
        );
    }
}

fn scan_pcl_instances(instances: &mut Vec<InstanceInfo>, seen: &mut std::collections::HashSet<String>) {
    let search_dirs = mut_pcl_search_dirs();
    for pcl_dir in search_dirs {
        scan_pcl_config(&pcl_dir, instances, seen);
    }

    for dot_minecraft in common_minecraft_roots() {
        if dot_minecraft.join("PCL.ini").exists() {
            scan_dot_minecraft(&dot_minecraft, instances, seen, "PCL2");
        }
    }
}

fn mut_pcl_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for appdata in [dirs::data_dir(), dirs::data_local_dir()] {
        if let Some(appdata) = appdata {
            for name in ["PCL", "PCL2", "Plain Craft Launcher 2"] {
                dirs.push(appdata.join(name));
            }
        }
    }
    for drive in ["C:", "D:", "E:"] {
        for name in ["PCL", "PCL2", "Plain Craft Launcher 2", "Minecraft/PCL2", "Minecraft\\PCL2"] {
            dirs.push(PathBuf::from(format!("{drive}\\{name}")));
        }
    }
    dirs.retain(|p| p.exists());
    dirs
}

fn scan_pcl_config(pcl_dir: &Path, instances: &mut Vec<InstanceInfo>, seen: &mut std::collections::HashSet<String>) {
    for file_name in ["Setup.ini", "PCL.ini", "Config.ini", "Setup.ini.bak"] {
        let ini = pcl_dir.join(file_name);
        if ini.exists() {
            if let Ok(content) = std::fs::read_to_string(&ini) {
                for mc_path in extract_paths_from_text(&content) {
                    if mc_path.exists() {
                        push_instance(
                            instances,
                            seen,
                            detect_from_game_dir(&mc_path).ok().map(|mut i| {
                                i.launcher = Some("PCL2".to_string());
                                i
                            }),
                        );
                    }
                }
            }
        }
    }

    scan_text_files_for_minecraft_paths(pcl_dir, 2, instances, seen);

    if let Some(parent) = pcl_dir.parent() {
        let mc = parent.join(".minecraft");
        if mc.exists() {
            scan_dot_minecraft(&mc, instances, seen, "PCL2");
        }
    }
}

fn scan_text_files_for_minecraft_paths(
    dir: &Path,
    depth: u32,
    instances: &mut Vec<InstanceInfo>,
    seen: &mut std::collections::HashSet<String>,
) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_text_files_for_minecraft_paths(&path, depth - 1, instances, seen);
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !matches!(ext, "ini" | "json" | "txt" | "cfg" | "yml" | "yaml") {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&path) {
            for mc_path in extract_paths_from_text(&content) {
                if mc_path.join("versions").exists() || mc_path.join("mods").exists() {
                    push_instance(
                        instances,
                        seen,
                        detect_from_game_dir(&mc_path).ok().map(|mut i| {
                            i.launcher = Some("PCL2".to_string());
                            i
                        }),
                    );
                }
            }
        }
    }
}

fn extract_paths_from_text(content: &str) -> Vec<PathBuf> {
    let re = Regex::new(r#"[A-Za-z]:\\[^\s"'\r\n]+"#).unwrap();
    let mut paths = Vec::new();
    for cap in re.find_iter(content) {
        let mut path_str = cap.as_str().trim_end_matches('\\').to_string();
        if path_str.to_lowercase().contains(".minecraft") {
            paths.push(PathBuf::from(&path_str));
        }
        while path_str.contains('\\') {
            if let Some(parent) = PathBuf::from(&path_str).parent() {
                if parent.file_name().and_then(|n| n.to_str()) == Some(".minecraft") {
                    paths.push(parent.to_path_buf());
                    break;
                }
                path_str = parent.to_string_lossy().to_string();
            } else {
                break;
            }
        }
    }
    paths
}

fn push_instance(
    instances: &mut Vec<InstanceInfo>,
    seen: &mut std::collections::HashSet<String>,
    info: Option<InstanceInfo>,
) {
    if let Some(info) = info {
        let key = normalize_path(&info.mods_path);
        if seen.insert(key) {
            instances.push(info);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fabric_version_id_parses_mc_version() {
        let (mc, loader) = parse_version_id_string("fabric-loader-0.16.9-1.21.4");
        assert_eq!(mc, "1.21.4");
        assert_eq!(loader, "fabric");
    }

    #[test]
    fn forge_version_id_parses_mc_version() {
        let (mc, loader) = parse_version_id_string("1.21.4-forge-54.0.0");
        assert_eq!(mc, "1.21.4");
        assert_eq!(loader, "forge");
    }

    #[test]
    fn inherits_from_json() {
        let json = r#"{"id":"fabric-loader-0.16.9-1.21.4","inheritsFrom":"1.21.4"}"#;
        let (mc, _, lv) = parse_version_id_and_json("fabric-loader-0.16.9-1.21.4", json);
        assert_eq!(mc, "1.21.4");
        assert_eq!(lv, "0.16.9");
    }

    #[test]
    fn pcl_version_id_parses_mc_version() {
        let (mc, loader) = parse_version_id_string("1.21.11-Fabric");
        assert_eq!(mc, "1.21.11");
        assert_eq!(loader, "fabric");
    }

    #[test]
    fn pcl_version_id_with_loader_version() {
        let (mc, loader) = parse_version_id_string("1.21.8-Fabric 0.17.2");
        assert_eq!(mc, "1.21.8");
        assert_eq!(loader, "fabric");
        assert_eq!(
            extract_loader_version("1.21.8-Fabric 0.17.2", "fabric"),
            "0.17.2"
        );
    }

    #[test]
    fn fabric_loader_id_extracts_loader_version() {
        assert_eq!(
            extract_loader_version("fabric-loader-0.16.9-1.21.4", "fabric"),
            "0.16.9"
        );
    }

    #[test]
    fn detect_version_folder_without_mods_if_present() {
        let version = PathBuf::from(r"D:\.minecraft\versions\1.21.11-Fabric");
        if !version.is_dir() {
            return;
        }
        let info = detect_from_path(version.to_str().unwrap()).expect("detect");
        assert_eq!(info.mc_version, "1.21.11");
        assert_eq!(info.loader, "fabric");
        assert!(info.mods_path.ends_with("mods"));
    }
}
