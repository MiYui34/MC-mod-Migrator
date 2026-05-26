use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::StreamExt;
use reqwest::Response;
use reqwest::Client;
use serde::de::DeserializeOwned;
use tokio::io::AsyncWriteExt;

use crate::cancellation::CancelToken;
use crate::jar::validate_zip_file;

pub const APP_USER_AGENT: &str = concat!("MC-Mod-Migrator/", env!("CARGO_PKG_VERSION"));
pub const APP_USER_AGENT_MOD: &str = concat!("MC-Mod-Migrator/", env!("CARGO_PKG_VERSION"), " (mod-identifier)");

/// Shared HTTP client with sane timeouts so a hung mirror cannot block the UI forever.
pub fn build_http_client(user_agent: &str) -> Client {
    Client::builder()
        .user_agent(user_agent)
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(120))
        .pool_max_idle_per_host(64)
        .build()
        .expect("failed to build HTTP client")
}

/// Decode JSON with a short body preview when mirrors return HTML or unexpected shapes.
pub async fn decode_json_response<T: DeserializeOwned>(resp: Response) -> anyhow::Result<T> {
    let status = resp.status();
    let url = resp.url().to_string();
    let bytes = resp.bytes().await?;
    serde_json::from_slice(&bytes).map_err(|e| {
        let preview = String::from_utf8_lossy(&bytes[..bytes.len().min(240)]);
        anyhow::anyhow!(
            "failed to decode JSON from {url} (HTTP {status}): {e}; body: {preview}"
        )
    })
}

fn temp_download_path(dest: &Path) -> PathBuf {
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    dest.with_file_name(format!("{name}.part"))
}

pub async fn cleanup_temp_download(dest: &Path) {
    let temp = temp_download_path(dest);
    let _ = tokio::fs::remove_file(&temp).await;
    let _ = tokio::fs::remove_file(dest).await;
}

pub fn mirror_fallback_urls(url: &str) -> Vec<String> {
    let mut urls = vec![url.to_string()];
    if url.contains("mod.mcimirror.top") {
        for base in [
            "https://cdn.modrinth.com",
            "https://edge.forgecdn.net",
            "https://mediafilez.forgecdn.net",
        ] {
            urls.push(url.replacen("https://mod.mcimirror.top", base, 1));
        }
    }
    urls.sort();
    urls.dedup();
    urls
}

/// Stream download to a temp file, validate ZIP integrity, then atomically replace `dest`.
pub async fn download_zip_file_validated(
    client: &Client,
    url: &str,
    dest: &Path,
    cancel: &CancelToken,
) -> anyhow::Result<()> {
    let mut last_err: Option<anyhow::Error> = None;
    for candidate in mirror_fallback_urls(url) {
        cleanup_temp_download(dest).await;
        match download_zip_once(client, &candidate, dest, cancel).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                cleanup_temp_download(dest).await;
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("下载失败: {url}")))
}

async fn download_zip_once(
    client: &Client,
    url: &str,
    dest: &Path,
    cancel: &CancelToken,
) -> anyhow::Result<()> {
    let temp = temp_download_path(dest);
    let resp = client.get(url).send().await?.error_for_status()?;
    if let Some(parent) = temp.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut file = tokio::fs::File::create(&temp).await?;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        cancel.ensure_running()?;
        file.write_all(&chunk?).await?;
    }
    file.flush().await?;
    drop(file);

    let temp_for_validate = temp.clone();
    tokio::task::spawn_blocking(move || validate_zip_file(&temp_for_validate)).await??;

    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let _ = tokio::fs::remove_file(dest).await;
    tokio::fs::rename(&temp, dest).await?;
    Ok(())
}

/// Stream download to a temp file, then atomically replace `dest` (no ZIP validation).
pub async fn download_raw_file(
    client: &Client,
    url: &str,
    dest: &Path,
    cancel: &CancelToken,
) -> anyhow::Result<()> {
    let mut last_err: Option<anyhow::Error> = None;
    for candidate in mirror_fallback_urls(url) {
        cleanup_temp_download(dest).await;
        match download_raw_once(client, &candidate, dest, cancel).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                cleanup_temp_download(dest).await;
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("下载失败: {url}")))
}

async fn download_raw_once(
    client: &Client,
    url: &str,
    dest: &Path,
    cancel: &CancelToken,
) -> anyhow::Result<()> {
    let temp = temp_download_path(dest);
    let resp = client.get(url).send().await?.error_for_status()?;
    if let Some(parent) = temp.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut file = tokio::fs::File::create(&temp).await?;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        cancel.ensure_running()?;
        file.write_all(&chunk?).await?;
    }
    file.flush().await?;
    drop(file);

    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let _ = tokio::fs::remove_file(dest).await;
    tokio::fs::rename(&temp, dest).await?;
    Ok(())
}

/// Download URL bytes with mirror fallback; optional progress callback (downloaded, total).
pub async fn download_bytes_with_progress<F>(
    url: &str,
    settings: &crate::models::AppSettings,
    cancel: &CancelToken,
    mut on_progress: F,
) -> anyhow::Result<Vec<u8>>
where
    F: FnMut(u64, Option<u64>),
{
    use crate::providers::endpoints::{mirrors_with_official_fallback, rewrite_cf_download_url};

    let client = build_http_client(APP_USER_AGENT);
    let mut candidates = vec![url.to_string()];
    for endpoints in mirrors_with_official_fallback(&settings.mod_api_mirror) {
        candidates.push(endpoints.rewrite_download_url(url));
    }
    candidates.push(rewrite_cf_download_url(url, settings));
    candidates.extend(mirror_fallback_urls(url));
    candidates.sort();
    candidates.dedup();

    let mut last_err: Option<anyhow::Error> = None;
    for candidate in candidates {
        match download_bytes_once(&client, &candidate, cancel, &mut on_progress).await {
            Ok(bytes) => return Ok(bytes),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("下载失败: {url}")))
}

async fn download_bytes_once<F>(
    client: &Client,
    url: &str,
    cancel: &CancelToken,
    on_progress: &mut F,
) -> anyhow::Result<Vec<u8>>
where
    F: FnMut(u64, Option<u64>),
{
    let resp = client.get(url).send().await?.error_for_status()?;
    let total = resp.content_length();
    let mut stream = resp.bytes_stream();
    let mut downloaded = 0u64;
    let mut data = Vec::new();
    let mut last_emit = 0u64;
    const EMIT_STEP: u64 = 256 * 1024;

    while let Some(chunk) = stream.next().await {
        cancel.ensure_running()?;
        let chunk = chunk?;
        downloaded += chunk.len() as u64;
        data.extend_from_slice(&chunk);
        if downloaded - last_emit >= EMIT_STEP {
            on_progress(downloaded, total);
            last_emit = downloaded;
        }
    }
    on_progress(downloaded, total);
    Ok(data)
}

pub async fn download_zip_once_public(
    client: &Client,
    url: &str,
    dest: &Path,
    cancel: &CancelToken,
) -> anyhow::Result<()> {
    download_zip_once(client, url, dest, cancel).await
}

pub async fn copy_zip_file_validated(src: &Path, dest: &Path) -> anyhow::Result<()> {
    let src = src.to_path_buf();
    let dest = dest.to_path_buf();
    tokio::task::spawn_blocking(move || {
        validate_zip_file(&src)?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temp = temp_download_path(&dest);
        let _ = std::fs::remove_file(&temp);
        std::fs::copy(&src, &temp)?;
        validate_zip_file(&temp)?;
        if dest.exists() {
            std::fs::remove_file(&dest)?;
        }
        std::fs::rename(&temp, &dest)?;
        Ok::<(), anyhow::Error>(())
    })
    .await??;
    Ok(())
}