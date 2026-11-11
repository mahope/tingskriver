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

/// Construct on a blocking worker, publishing only a fully initialized value.
pub(crate) fn initialize_engine<T, E>(
    slot: &std::sync::Mutex<Option<std::sync::Arc<T>>>,
    create: impl FnOnce() -> Result<T, E>,
) -> Result<(), E> {
    if slot.lock().unwrap_or_else(|p| p.into_inner()).is_some() {
        return Ok(());
    }
    let engine = std::sync::Arc::new(create()?);
    // Another initializer may have published while construction was in progress.
    slot.lock()
        .unwrap_or_else(|p| p.into_inner())
        .get_or_insert(engine);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::initialize_engine;
    use std::sync::{Arc, Mutex};

    #[test]
    fn engine_construction_does_not_lock_readers() {
        let slot = Mutex::new(None);
        initialize_engine(&slot, || {
            assert!(
                slot.try_lock().is_ok(),
                "construction blocks engine readers"
            );
            Ok::<_, ()>(42)
        })
        .unwrap();
        assert_eq!(
            **slot
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_ref()
                .unwrap(),
            42
        );
    }

    #[test]
    fn engine_initialization_preserves_an_existing_engine() {
        let original = Arc::new(7);
        let slot = Mutex::new(Some(original.clone()));
        initialize_engine(&slot, || -> Result<i32, ()> {
            panic!("already initialized")
        })
        .unwrap();
        assert!(Arc::ptr_eq(
            slot.lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_ref()
                .unwrap(),
            &original
        ));
    }

    #[test]
    fn concurrent_initialization_keeps_the_first_published_engine() {
        let slot = Mutex::new(None);
        initialize_engine(&slot, || {
            initialize_engine(&slot, || Ok::<_, ()>(7)).unwrap();
            Ok::<_, ()>(42)
        })
        .unwrap();
        assert_eq!(
            **slot
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_ref()
                .unwrap(),
            7
        );
    }

    #[test]
    fn failed_engine_initialization_can_be_retried() {
        let slot = Mutex::new(None);
        assert_eq!(
            initialize_engine(&slot, || Err::<i32, _>("disk error")),
            Err("disk error")
        );
        initialize_engine(&slot, || Ok::<_, &str>(9)).unwrap();
        assert_eq!(
            **slot
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_ref()
                .unwrap(),
            9
        );
    }
}
