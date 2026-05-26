use crate::models::AppSettings;

pub fn api_concurrency(settings: &AppSettings) -> usize {
    settings.max_concurrent_downloads.clamp(2, 32) as usize
}

/// Higher parallelism for read-only compatibility checks (no disk writes).
pub fn check_concurrency(settings: &AppSettings) -> usize {
    (settings.max_concurrent_downloads.max(12) * 2).clamp(24, 48) as usize
}
