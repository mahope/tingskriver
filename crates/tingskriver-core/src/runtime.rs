//! Spawning async work from any thread.
//!
//! Core code runs on callback threads (audio, keyboard hook) where no tokio
//! runtime is current. The host registers its runtime once at startup with
//! [`set_handle`]; [`spawn`] then works from anywhere, like
//! `tauri::async_runtime::spawn` did.

use std::future::Future;
use std::sync::OnceLock;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

static HANDLE: OnceLock<Handle> = OnceLock::new();

/// Register the runtime that [`spawn`] uses when called outside a runtime.
pub fn set_handle(handle: Handle) {
    let _ = HANDLE.set(handle);
}

fn handle() -> Handle {
    if let Some(handle) = HANDLE.get() {
        return handle.clone();
    }
    Handle::try_current()
        .expect("tingskriver_core::runtime::set_handle must be called before spawning from a non-runtime thread")
}

/// Spawn a future on the host runtime.
pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    handle().spawn(future)
}
