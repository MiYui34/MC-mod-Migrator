use std::sync::Arc;

use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

use crate::models::TransferProgress;

/// 串行发送进度事件，避免并行任务导致 IPC 乱序、前端进度回退。
pub struct MonotonicEmitter {
    app: AppHandle,
    event: &'static str,
    state: Mutex<EmitterState>,
}

struct EmitterState {
    completed: u32,
    total: u32,
}

impl MonotonicEmitter {
    pub fn new(app: AppHandle, event: &'static str, total: u32) -> Arc<Self> {
        Arc::new(Self {
            app,
            event,
            state: Mutex::new(EmitterState { completed: 0, total }),
        })
    }

    pub async fn emit(&self, current: u32, file_name: &str, message: &str) {
        let state = self.state.lock().await;
        self.emit_locked(&state, current, file_name, message);
    }

    /// 仅更新文案，保持当前已完成数不变（用于阶段切换，不重置进度）。
    pub async fn emit_status(&self, file_name: &str, message: &str) {
        let state = self.state.lock().await;
        self.emit_locked(&state, state.completed, file_name, message);
    }

    pub async fn step<F>(&self, file_name: &str, message: F) -> u32
    where
        F: FnOnce(u32, u32) -> String,
    {
        let mut state = self.state.lock().await;
        state.completed += 1;
        let current = state.completed;
        let total = state.total;
        let message = message(current, total);
        self.emit_locked(&state, current, file_name, &message);
        current
    }

    fn emit_locked(&self, state: &EmitterState, current: u32, file_name: &str, message: &str) {
        let _ = self.app.emit(
            self.event,
            TransferProgress {
                current,
                total: state.total,
                file_name: file_name.to_string(),
                message: message.to_string(),
            },
        );
    }
}
