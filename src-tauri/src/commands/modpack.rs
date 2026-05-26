use std::io::Read;
use std::path::{Component, Path};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use zip::ZipArchive;

use super::market::MarketInstallContext;
use super::market_undo::MarketInstallSession;
use super::progress::MonotonicEmitter;
use crate::http::{download_bytes_with_progress, download_zip_file_validated};
use crate::models::MarketInstallResult;
use crate::providers::curseforge::CurseForgeProvider;
use crate::providers::endpoints::rewrite_cf_download_url;

#[derive(Debug, Deserialize)]
struct MrpackIndex {
    #[serde(rename = "formatVersion")]
    format_version: u32,
    files: Vec<MrpackFile>,
}

#[derive(Debug, Deserialize)]
struct MrpackFile {
    path: String,
    #[serde(default)]
    downloads: Vec<String>,
    #[serde(default)]
    hashes: std::collections::HashMap<String, String>,
}

pub async fn install_modpack(
    ctx: &MarketInstallContext<'_>,
    download_url: &str,
    file_name: &str,
    session: &mut MarketInstallSession,
    progress: Option<&Arc<MonotonicEmitter>>,
) -> Result<MarketInstallResult> {
    let game_dir = Path::new(&ctx.target.game_dir);
    let is_mrpack = file_name.to_ascii_lowercase().ends_with(".mrpack");

    if let Some(p) = progress {
        p.emit_status(file_name, "下载整合包…").await;
    }

    let url = rewrite_url(download_url, ctx);
    let bytes = download_bytes_with_progress(&url, ctx.settings, ctx.cancel, |_, _| {}).await?;

    if is_mrpack {
        return install_mrpack_bytes(
            &bytes,
            game_dir,
            file_name,
            ctx,
            session,
            progress,
        )
        .await;
    }

    if let Some(result) = try_install_cf_modpack_bytes(
        &bytes,
        game_dir,
        file_name,
        ctx,
        session,
        progress,
    )
    .await?
    {
        return Ok(result);
    }

    install_launcher_import(&bytes, file_name, game_dir, session).await
}

async fn install_mrpack_bytes(
    bytes: &[u8],
    game_dir: &Path,
    file_name: &str,
    ctx: &MarketInstallContext<'_>,
    session: &mut MarketInstallSession,
    progress: Option<&Arc<MonotonicEmitter>>,
) -> Result<MarketInstallResult> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).context("打开 mrpack 压缩包")?;

    let index: MrpackIndex = {
        let mut file = archive
            .by_name("modrinth.index.json")
            .context("缺少 modrinth.index.json")?;
        let mut text = String::new();
        file.read_to_string(&mut text)?;
        serde_json::from_str(&text).context("解析 modrinth.index.json")?
    };

    if index.format_version != 1 {
        bail!("不支持的 mrpack formatVersion: {}", index.format_version);
    }

    let total = index.files.len().max(1);
    let mut installed: Vec<String> = Vec::new();
    let pack_name = file_name_stem(file_name);

    for (idx, entry) in index.files.iter().enumerate() {
        ctx.cancel.ensure_running()?;
        if let Some(p) = progress {
            p.emit_status(&entry.path, &format!("安装整合包文件 ({}/{total})", idx + 1))
                .await;
        }

        let dest = safe_game_path(game_dir, &entry.path)?;
        if dest.exists() {
            session.track_overwrite(dest.clone())?;
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if !entry.downloads.is_empty() {
            let url = rewrite_url(&entry.downloads[0], ctx);
            let data = download_bytes_with_progress(&url, ctx.settings, ctx.cancel, |_, _| {})
                .await?;
            verify_hashes(&data, &entry.hashes)?;
            std::fs::write(&dest, &data)?;
        } else {
            let mut zip_file = archive
                .by_name(&entry.path)
                .with_context(|| format!("mrpack 内缺少文件: {}", entry.path))?;
            let mut data = Vec::new();
            zip_file.read_to_end(&mut data)?;
            verify_hashes(&data, &entry.hashes)?;
            std::fs::write(&dest, &data)?;
        }

        session.track_created(dest.clone());
        installed.push(dest.to_string_lossy().replace('\\', "/"));
    }

    if let Some(p) = progress {
        p.emit_status(&pack_name, "整合包安装完成").await;
    }

    Ok(MarketInstallResult {
        file_path: game_dir.to_string_lossy().to_string(),
        file_name: pack_name,
        needs_launcher_import: false,
        hint: String::new(),
        record_id: None,
        installed_files: installed,
    })
}

fn safe_game_path(game_dir: &Path, entry_path: &str) -> Result<std::path::PathBuf> {
    let rel = Path::new(entry_path);
    for component in rel.components() {
        match component {
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("非法 mrpack 路径: {entry_path}");
            }
            _ => {}
        }
    }
    Ok(game_dir.join(rel))
}

fn rewrite_url(url: &str, ctx: &MarketInstallContext<'_>) -> String {
    use crate::providers::endpoints::{mirrors_with_official_fallback, rewrite_cf_download_url};
    let mut out = url.to_string();
    for endpoints in mirrors_with_official_fallback(&ctx.settings.mod_api_mirror) {
        out = endpoints.rewrite_download_url(&out);
    }
    rewrite_cf_download_url(&out, &ctx.settings)
}

fn file_name_stem(file_name: &str) -> String {
    file_name
        .rsplit('/')
        .next()
        .unwrap_or(file_name)
        .trim_end_matches(".mrpack")
        .trim_end_matches(".zip")
        .to_string()
}

#[derive(Debug, Deserialize)]
struct CfModpackManifest {
    #[serde(rename = "manifestType")]
    manifest_type: String,
    files: Vec<CfModpackFileEntry>,
}

#[derive(Debug, Deserialize)]
struct CfModpackFileEntry {
    #[serde(rename = "projectID")]
    project_id: i64,
    #[serde(rename = "fileID")]
    file_id: i64,
    #[serde(default)]
    required: bool,
}

async fn try_install_cf_modpack_bytes(
    bytes: &[u8],
    _game_dir: &Path,
    file_name: &str,
    ctx: &MarketInstallContext<'_>,
    session: &mut MarketInstallSession,
    progress: Option<&Arc<MonotonicEmitter>>,
) -> Result<Option<MarketInstallResult>> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = match ZipArchive::new(cursor) {
        Ok(a) => a,
        Err(_) => return Ok(None),
    };

    let manifest: CfModpackManifest = {
        let mut file = match archive.by_name("manifest.json") {
            Ok(f) => f,
            Err(_) => return Ok(None),
        };
        let mut text = String::new();
        file.read_to_string(&mut text)?;
        match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(_) => return Ok(None),
        }
    };

    if manifest.manifest_type != "minecraftModpack" {
        return Ok(None);
    }

    let mods_dir = Path::new(&ctx.target.mods_path);
    std::fs::create_dir_all(mods_dir)?;
    let cf = CurseForgeProvider::from_settings(ctx.settings);
    let total = manifest.files.len().max(1);
    let mut installed: Vec<String> = Vec::new();
    let pack_name = file_name_stem(file_name);

    for (idx, entry) in manifest.files.iter().enumerate() {
        ctx.cancel.ensure_running()?;
        if let Some(p) = progress {
            p.emit_status(
                &format!("cf-{}-{}", entry.project_id, entry.file_id),
                &format!("安装整合包 Mod ({}/{total})", idx + 1),
            )
            .await;
        }

        let (file_name, download_url) = cf
            .get_mod_file_for_install(entry.project_id, entry.file_id)
            .await
            .with_context(|| {
                format!(
                    "获取 CurseForge 文件 {}:{} 失败",
                    entry.project_id, entry.file_id
                )
            })?;

        let safe_name = sanitize_file_name(&file_name);
        let dest = mods_dir.join(&safe_name);
        if dest.exists() {
            session.track_overwrite(dest.clone())?;
        }

        let url = rewrite_cf_download_url(&download_url, ctx.settings);
        download_zip_file_validated(ctx.client, &url, &dest, ctx.cancel).await?;
        session.track_created(dest.clone());
        installed.push(dest.to_string_lossy().replace('\\', "/"));
    }

    if let Some(p) = progress {
        p.emit_status(&pack_name, "CurseForge 整合包 Mod 安装完成").await;
    }

    Ok(Some(MarketInstallResult {
        file_path: mods_dir.to_string_lossy().to_string(),
        file_name: pack_name,
        needs_launcher_import: false,
        hint: format!("已从 CurseForge 整合包安装 {} 个 Mod 到 mods 文件夹", installed.len()),
        record_id: None,
        installed_files: installed,
    }))
}

async fn install_launcher_import(
    bytes: &[u8],
    file_name: &str,
    game_dir: &Path,
    session: &mut MarketInstallSession,
) -> Result<MarketInstallResult> {
    let downloads_dir = game_dir.join("downloads");
    std::fs::create_dir_all(&downloads_dir)?;
    let dest = downloads_dir.join(sanitize_file_name(file_name));
    if dest.exists() {
        session.track_overwrite(dest.clone())?;
    }
    std::fs::write(&dest, bytes)?;
    session.track_created(dest.clone());

    Ok(MarketInstallResult {
        file_path: dest.to_string_lossy().to_string(),
        file_name: file_name.to_string(),
        needs_launcher_import: true,
        hint: "请在 PCL 或 HMCL 启动器中导入此整合包".to_string(),
        record_id: None,
        installed_files: vec![dest.to_string_lossy().replace('\\', "/")],
    })
}

fn verify_hashes(data: &[u8], hashes: &std::collections::HashMap<String, String>) -> Result<()> {
    use sha1::Digest;
    if let Some(expected) = hashes.get("sha1") {
        let digest = hex::encode(sha1::Sha1::digest(data));
        if !digest.eq_ignore_ascii_case(expected) {
            bail!("sha1 校验失败");
        }
    }
    if let Some(expected) = hashes.get("sha512") {
        use sha2::Digest as Sha512Digest;
        let digest = hex::encode(sha2::Sha512::digest(data));
        if !digest.eq_ignore_ascii_case(expected) {
            bail!("sha512 校验失败");
        }
    }
    Ok(())
}

fn sanitize_file_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn safe_game_path_rejects_traversal() {
        let game = PathBuf::from(r"C:\game");
        assert!(safe_game_path(&game, "../mods/evil.jar").is_err());
        assert!(safe_game_path(&game, r"C:\evil.jar").is_err());
        assert_eq!(
            safe_game_path(&game, "mods/fabric-api.jar").unwrap(),
            game.join("mods/fabric-api.jar")
        );
    }
}
