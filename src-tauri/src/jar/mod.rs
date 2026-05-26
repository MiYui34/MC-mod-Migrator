use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde::Deserialize;
use zip::ZipArchive;

/// Minimum valid ZIP size (END header + central directory).
const MIN_ZIP_BYTES: u64 = 22;

pub fn validate_zip_file(path: &Path) -> anyhow::Result<()> {
    let meta = std::fs::metadata(path)?;
    if meta.len() < MIN_ZIP_BYTES {
        anyhow::bail!("文件过小，可能下载不完整");
    }
    let mut f = File::open(path)?;
    let mut magic = [0u8; 2];
    f.read_exact(&mut magic)?;
    if magic != [0x50, 0x4B] {
        anyhow::bail!("不是有效的 ZIP/JAR 文件（可能为镜像错误页或 HTML）");
    }
    let file = File::open(path)?;
    ZipArchive::new(file).map_err(|e| anyhow::anyhow!("ZIP 结构无效: {e}"))?;
    Ok(())
}

#[derive(Debug, Clone, Default)]
pub struct JarMetadata {
    pub mod_id: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    pub depends: Vec<String>,
    pub depend_versions: HashMap<String, String>,
    pub loader_hint: Option<String>,
}

pub fn parse_jar_metadata(path: &Path) -> anyhow::Result<JarMetadata> {
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file)?;
    let mut meta = JarMetadata::default();

    if let Ok(mut f) = archive.by_name("fabric.mod.json") {
        let mut content = String::new();
        f.read_to_string(&mut content)?;
        if let Ok(parsed) = serde_json::from_str::<FabricModJson>(&content) {
            meta.mod_id = Some(parsed.id.clone());
            meta.name = Some(parsed.name.clone());
            meta.version = Some(parsed.version.clone());
            meta.loader_hint = Some("fabric".into());
            if let Some(deps) = parsed.depends {
                meta.depend_versions = deps.clone();
                meta.depends = deps.keys().cloned().collect();
            }
        }
        return Ok(meta);
    }

    if let Ok(mut f) = archive.by_name("META-INF/mods.toml") {
        let mut content = String::new();
        f.read_to_string(&mut content)?;
        parse_mods_toml(&content, &mut meta);
        if meta.loader_hint.is_none() {
            meta.loader_hint = Some("forge".into());
        }
        return Ok(meta);
    }

    if let Ok(mut f) = archive.by_name("mcmod.info") {
        let mut content = String::new();
        f.read_to_string(&mut content)?;
        if let Ok(entries) = serde_json::from_str::<Vec<McModInfoEntry>>(&content) {
            if let Some(entry) = entries.first() {
                meta.mod_id = Some(entry.modid.clone());
                meta.name = Some(entry.name.clone());
                meta.version = entry.version.clone();
                meta.loader_hint = Some("forge".into());
            }
        }
        return Ok(meta);
    }

    Ok(meta)
}

#[derive(Debug, Deserialize)]
struct FabricModJson {
    id: String,
    name: String,
    version: String,
    #[serde(default)]
    depends: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct McModInfoEntry {
    modid: String,
    name: String,
    version: Option<String>,
}

fn parse_mods_toml(content: &str, meta: &mut JarMetadata) {
    let mut in_mod = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("[[mods]]") {
            in_mod = true;
            continue;
        }
        if trimmed.starts_with('[') && !trimmed.starts_with("[[mods]]") {
            in_mod = false;
            continue;
        }
        if !in_mod {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim();
            let value = value.trim().trim_matches('"');
            match key {
                "modId" => meta.mod_id = Some(value.to_string()),
                "displayName" => meta.name = Some(value.to_string()),
                "version" => meta.version = Some(value.to_string()),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn rejects_invalid_zip() {
        let path = std::env::temp_dir().join(format!("invalid-jar-{}.txt", std::process::id()));
        std::fs::File::create(&path)
            .unwrap()
            .write_all(b"not a zip")
            .unwrap();
        assert!(validate_zip_file(&path).is_err());
        let _ = std::fs::remove_file(path);
    }
}
