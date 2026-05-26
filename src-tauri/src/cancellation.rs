use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn reset(&self) {
        self.0.store(false, Ordering::Relaxed);
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    pub fn ensure_running(&self) -> anyhow::Result<()> {
        if self.is_cancelled() {
            anyhow::bail!("操作已取消");
        }
        Ok(())
    }
}

pub const CANCELLED_MSG: &str = "操作已取消";
