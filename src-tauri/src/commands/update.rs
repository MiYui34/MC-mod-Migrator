use std::path::{Path, PathBuf};



use futures::StreamExt;

use reqwest::StatusCode;

use semver::Version;

use sha2::{Digest, Sha256};

use tauri::{AppHandle, Emitter};

use tokio::io::AsyncWriteExt;



use crate::cancellation::CancelToken;
use crate::pe_version::assert_installer_version;

use crate::http::build_http_client;

use crate::models::{AppSettings, UpdateCheckResult, UpdateManifest, UpdateProgress, UpdateState};



const USER_AGENT: &str = "MC-Mod-Migrator-Updater/1.0";



pub const DEFAULT_UPDATE_MANIFEST_URL: &str = "https://www.sgu-server.xin/updates/latest.json";



pub fn app_version() -> String {

    env!("CARGO_PKG_VERSION").to_string()

}



pub fn effective_manifest_url(settings: &AppSettings) -> Option<String> {

    let custom = settings.update_manifest_url.trim();

    if !custom.is_empty() {

        return Some(custom.to_string());

    }

    if settings.update_use_default_source {

        return Some(DEFAULT_UPDATE_MANIFEST_URL.to_string());

    }

    None

}



pub fn validate_manifest_url(url: &str) -> anyhow::Result<()> {

    let trimmed = url.trim();

    if trimmed.is_empty() {

        return Ok(());

    }

    if trimmed.starts_with("https://") {

        return Ok(());

    }

    if trimmed.starts_with("http://") {

        anyhow::bail!("远程更新源必须使用 HTTPS，请改用 https:// 地址");

    }

    Ok(())

}



fn parse_version(raw: &str) -> anyhow::Result<Version> {

    let trimmed = raw.trim().trim_start_matches('v').trim_start_matches('V');

    Version::parse(trimmed).map_err(|e| anyhow::anyhow!("无效版本号 {raw}: {e}"))

}



fn is_newer(latest: &str, current: &str) -> anyhow::Result<bool> {

    Ok(parse_version(latest)? > parse_version(current)?)

}



fn is_below_min_supported(manifest: &UpdateManifest, current: &str) -> bool {

    let Some(min) = manifest.min_supported_version.as_deref() else {

        return false;

    };

    if min.trim().is_empty() {

        return false;

    }

    match (parse_version(current), parse_version(min)) {

        (Ok(c), Ok(m)) => c < m,

        _ => false,

    }

}



fn resolve_manifest_source(source: &str) -> PathBuf {

    let trimmed = source.trim();

    if let Some(rest) = trimmed.strip_prefix("file://") {

        if rest.starts_with('/') && rest.as_bytes().get(2) == Some(&b':') {

            return PathBuf::from(rest.trim_start_matches('/'));

        }

        return PathBuf::from(rest);

    }

    PathBuf::from(trimmed)

}



async fn fetch_manifest_text(source: &str) -> anyhow::Result<String> {

    let trimmed = source.trim();

    if trimmed.is_empty() {

        anyhow::bail!("未配置更新清单地址");

    }

    validate_manifest_url(trimmed)?;

    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {

        let client = build_http_client(USER_AGENT);

        let resp = client.get(trimmed).send().await?.error_for_status()?;

        Ok(resp.text().await?)

    } else {

        let path = resolve_manifest_source(trimmed);

        Ok(tokio::fs::read_to_string(path).await?)

    }

}



pub async fn load_update_manifest(source: &str) -> anyhow::Result<UpdateManifest> {

    let text = fetch_manifest_text(source).await?;

    serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("解析更新清单失败: {e}"))

}



pub async fn check_for_update(manifest_url: &str) -> anyhow::Result<UpdateCheckResult> {

    validate_manifest_url(manifest_url)?;

    let current = app_version();

    let mut manifest = load_update_manifest(manifest_url).await?;

    let newer = is_newer(&manifest.version, &current)?;

    let below_min = is_below_min_supported(&manifest, &current);

    if below_min {

        manifest.mandatory = true;

    }

    let update_available = newer || below_min;

    Ok(UpdateCheckResult {

        current_version: current,

        update_available,

        manifest: if update_available {

            Some(manifest)

        } else {

            None

        },

    })

}



fn updates_dir(data_dir: &Path) -> PathBuf {

    data_dir.join("updates")

}



async fn emit_progress(app: &AppHandle, downloaded: u64, total: Option<u64>, message: &str) {

    let _ = app.emit(

        "app-update-progress",

        UpdateProgress {

            downloaded,

            total,

            message: message.to_string(),

        },

    );

}



pub fn compute_file_sha256(path: &Path) -> anyhow::Result<String> {

    let bytes = std::fs::read(path)?;

    let hash = Sha256::digest(&bytes);

    Ok(format!("{:x}", hash))

}



fn verify_downloaded_file(path: &Path, manifest: &UpdateManifest) -> anyhow::Result<()> {

    let meta = std::fs::metadata(path)?;

    if let Some(expected) = manifest.file_size {

        if meta.len() != expected {

            let _ = std::fs::remove_file(path);

            anyhow::bail!(

                "文件大小不匹配（期望 {} 字节，实际 {}）",

                expected,

                meta.len()

            );

        }

    }

    if let Some(ref hash) = manifest.sha256 {

        let expected = hash.trim();

        if !expected.is_empty() {

            let actual = compute_file_sha256(path)?;

            if actual.to_lowercase() != expected.to_lowercase() {

                let _ = std::fs::remove_file(path);

                anyhow::bail!("SHA256 校验失败，安装包可能已损坏，请重试下载");

            }

        }

    }

    assert_installer_version(path, &manifest.version)?;

    Ok(())

}



fn cleanup_old_updates(dir: &Path, keep: usize) {

    let Ok(read_dir) = std::fs::read_dir(dir) else {

        return;

    };

    let mut files: Vec<(PathBuf, std::time::SystemTime)> = read_dir

        .filter_map(|e| e.ok())

        .filter(|e| {

            e.path()

                .extension()

                .is_none_or(|ext| ext != "part")

        })

        .filter(|e| e.path().is_file())

        .filter_map(|e| {

            let modified = e.metadata().ok()?.modified().ok()?;

            Some((e.path(), modified))

        })

        .collect();

    if files.len() <= keep {

        return;

    }

    files.sort_by_key(|(_, t)| *t);

    let remove_count = files.len().saturating_sub(keep);

    for (path, _) in files.into_iter().take(remove_count) {

        let _ = std::fs::remove_file(path);

    }

}



pub async fn download_update(

    app: AppHandle,

    data_dir: &Path,

    cancel: &CancelToken,

    manifest: &UpdateManifest,

) -> anyhow::Result<PathBuf> {

    let dir = updates_dir(data_dir);

    tokio::fs::create_dir_all(&dir).await?;

    let dest = dir.join(sanitize_file_name(&manifest.file_name));

    let temp = dest.with_extension("part");



    if dest.exists() {

        if verify_downloaded_file(&dest, manifest).is_ok() {

            let len = tokio::fs::metadata(&dest).await?.len();

            emit_progress(&app, len, manifest.file_size, "已存在有效安装包").await;

            return Ok(dest);

        }

        let _ = tokio::fs::remove_file(&dest).await;

    }



    let mut downloaded: u64 = 0;

    if temp.exists() {

        downloaded = tokio::fs::metadata(&temp).await?.len();

    }



    let client = build_http_client(USER_AGENT);

    let mut req = client.get(&manifest.download_url);

    if downloaded > 0 {

        req = req.header("Range", format!("bytes={downloaded}-"));

    }



    let resp = req.send().await?.error_for_status()?;

    let status = resp.status();

    let total = resp

        .content_length()

        .map(|t| t + downloaded)

        .or(manifest.file_size);



    if downloaded > 0 && status != StatusCode::PARTIAL_CONTENT {

        downloaded = 0;

        let _ = tokio::fs::remove_file(&temp).await;

    }



    emit_progress(&app, downloaded, total, "正在下载更新…").await;



    let mut file = if downloaded > 0 {

        tokio::fs::OpenOptions::new()

            .append(true)

            .open(&temp)

            .await?

    } else {

        tokio::fs::File::create(&temp).await?

    };



    let mut stream = resp.bytes_stream();

    while let Some(chunk) = stream.next().await {

        cancel.ensure_running()?;

        let chunk = chunk?;

        downloaded += chunk.len() as u64;

        file.write_all(&chunk).await?;

        emit_progress(&app, downloaded, total, "正在下载更新…").await;

    }

    file.flush().await?;

    drop(file);



    tokio::fs::rename(&temp, &dest).await?;

    verify_downloaded_file(&dest, manifest)?;

    cleanup_old_updates(&dir, 2);

    emit_progress(&app, downloaded, total, "下载完成").await;

    Ok(dest)

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



pub fn launch_installer(path: &Path) -> anyhow::Result<()> {

    if !path.exists() {

        anyhow::bail!("安装包不存在: {}", path.display());

    }

    #[cfg(windows)]

    {

        std::process::Command::new(path)

            .spawn()

            .map_err(|e| anyhow::anyhow!("无法启动安装程序: {e}"))?;

    }

    #[cfg(target_os = "macos")]

    {

        std::process::Command::new("open")

            .arg(path)

            .spawn()

            .map_err(|e| anyhow::anyhow!("无法打开安装包: {e}"))?;

    }

    #[cfg(all(unix, not(target_os = "macos")))]

    {

        use std::os::unix::fs::PermissionsExt;

        let mut perms = std::fs::metadata(path)?.permissions();

        perms.set_mode(0o755);

        std::fs::set_permissions(path, perms)?;

        std::process::Command::new(path)

            .spawn()

            .map_err(|e| anyhow::anyhow!("无法启动安装程序: {e}"))?;

    }

    Ok(())

}



pub fn should_check_now(state: &UpdateState, interval_hours: u32) -> bool {

    let Some(last) = &state.last_check_at else {

        return true;

    };

    let Ok(parsed) = chrono_like_parse(last) else {

        return true;

    };

    let elapsed = std::time::SystemTime::now()

        .duration_since(parsed)

        .unwrap_or_default();

    elapsed >= std::time::Duration::from_secs(interval_hours as u64 * 3600)

}



fn chrono_like_parse(raw: &str) -> Result<std::time::SystemTime, ()> {

    use std::time::{Duration, UNIX_EPOCH};

    if let Ok(secs) = raw.parse::<u64>() {

        return Ok(UNIX_EPOCH + Duration::from_secs(secs));

    }

    Err(())

}



pub fn touch_last_check(state: &mut UpdateState) {

    state.last_check_at = Some(

        std::time::SystemTime::now()

            .duration_since(std::time::UNIX_EPOCH)

            .map(|d| d.as_secs().to_string())

            .unwrap_or_default(),

    );

}



pub fn record_check_result(

    state: &mut UpdateState,

    ok: bool,

    error: Option<String>,

    version: Option<String>,

) {

    touch_last_check(state);

    state.last_check_ok = ok;

    state.last_check_error = error;

    state.last_check_version = version;

}



#[cfg(test)]

mod tests {

    use super::*;



    #[test]

    fn newer_version_detected() {

        assert!(is_newer("0.2.0", "0.1.0").unwrap());

        assert!(!is_newer("0.1.0", "0.1.0").unwrap());

        assert!(!is_newer("0.0.9", "0.1.0").unwrap());

    }



    #[test]

    fn effective_url_uses_default() {

        let settings = AppSettings {

            update_manifest_url: String::new(),

            update_use_default_source: true,

            ..AppSettings::default()

        };

        assert_eq!(

            effective_manifest_url(&settings).as_deref(),

            Some(DEFAULT_UPDATE_MANIFEST_URL)

        );

    }



    #[test]

    fn rejects_http_remote_url() {

        assert!(validate_manifest_url("http://example.com/latest.json").is_err());

        assert!(validate_manifest_url("https://example.com/latest.json").is_ok());

        assert!(validate_manifest_url("C:\\updates\\latest.json").is_ok());

    }



    #[test]

    fn min_supported_forces_update() {

        let manifest = UpdateManifest {

            version: "0.2.0".into(),

            release_date: None,

            notes: String::new(),

            download_url: String::new(),

            file_name: String::new(),

            mandatory: false,

            sha256: None,

            file_size: None,

            min_supported_version: Some("0.2.0".into()),

            release_notes_url: None,

        };

        assert!(is_below_min_supported(&manifest, "0.1.0"));

        assert!(!is_below_min_supported(&manifest, "0.2.0"));

    }

}

