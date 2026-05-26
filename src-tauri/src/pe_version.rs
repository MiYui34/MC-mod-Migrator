use std::path::Path;

use semver::Version;

/// Normalize Windows ProductVersion (e.g. `0.2.1.0`) to semver `0.2.1`.
pub fn normalize_version_label(raw: &str) -> String {
    let trimmed = raw.trim().trim_start_matches(['v', 'V']);
    if let Ok(v) = Version::parse(trimmed) {
        return v.to_string();
    }
    let parts: Vec<&str> = trimmed.split('.').collect();
    match parts.len() {
        0 => trimmed.to_string(),
        1 | 2 | 3 => parts.join("."),
        _ => parts[..3].join("."),
    }
}

pub fn versions_match(expected: &str, actual: &str) -> bool {
    let a = normalize_version_label(expected);
    let b = normalize_version_label(actual);
    a == b
}

#[cfg(windows)]
pub fn read_pe_product_version(path: &Path) -> anyhow::Result<String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    use winapi::shared::minwindef::LPVOID;
    use winapi::um::winver::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};

    let wide: Vec<u16> = OsStr::new(path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let size = GetFileVersionInfoSizeW(wide.as_ptr(), std::ptr::null_mut());
        if size == 0 {
            anyhow::bail!("无法读取安装包版本信息: {}", path.display());
        }

        let mut buffer = vec![0u8; size as usize];
        if GetFileVersionInfoW(wide.as_ptr(), 0, size, buffer.as_mut_ptr() as LPVOID) == 0 {
            anyhow::bail!("读取安装包版本资源失败: {}", path.display());
        }

        let mut trans_ptr: LPVOID = std::ptr::null_mut();
        let mut trans_len: u32 = 0;
        let trans_key: Vec<u16> = "\\VarFileInfo\\Translation\0"
            .encode_utf16()
            .collect();
        if VerQueryValueW(
            buffer.as_ptr() as LPVOID,
            trans_key.as_ptr(),
            &mut trans_ptr,
            &mut trans_len,
        ) == 0
        {
            anyhow::bail!("安装包缺少版本 Translation 信息");
        }

        let trans = std::slice::from_raw_parts(trans_ptr as *const u16, 2);
        let lang = trans[0];
        let codepage = trans[1];
        for key in ["ProductVersion", "FileVersion"] {
            let subblock = format!("\\StringFileInfo\\{lang:04x}{codepage:04x}\\{key}");
            let subblock_wide: Vec<u16> = subblock.encode_utf16().chain(std::iter::once(0)).collect();
            let mut ver_ptr: LPVOID = std::ptr::null_mut();
            let mut ver_len: u32 = 0;
            if VerQueryValueW(
                buffer.as_ptr() as LPVOID,
                subblock_wide.as_ptr(),
                &mut ver_ptr,
                &mut ver_len,
            ) != 0
            {
                let slice = std::slice::from_raw_parts(ver_ptr as *const u16, ver_len as usize);
                let text = String::from_utf16_lossy(slice)
                    .trim_matches('\0')
                    .trim()
                    .to_string();
                if !text.is_empty() {
                    return Ok(normalize_version_label(&text));
                }
            }
        }
    }

    anyhow::bail!("安装包内未找到 ProductVersion / FileVersion")
}

#[cfg(not(windows))]
pub fn read_pe_product_version(_path: &Path) -> anyhow::Result<String> {
    anyhow::bail!("当前平台不支持读取安装包内嵌版本")
}

pub fn assert_installer_version(path: &Path, expected: &str) -> anyhow::Result<()> {
    let embedded = read_pe_product_version(path)?;
    if !versions_match(expected, &embedded) {
        anyhow::bail!(
            "安装包内嵌版本为 {embedded}，与更新清单 {expected} 不一致，已拒绝安装。请重新下载或联系发布者"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_windows_version() {
        assert_eq!(normalize_version_label("0.2.1.0"), "0.2.1");
        assert_eq!(normalize_version_label("v0.2.0"), "0.2.0");
    }

    #[test]
    fn versions_match_semantics() {
        assert!(versions_match("0.2.1", "0.2.1.0"));
        assert!(!versions_match("0.2.1", "0.2.0"));
    }
}
