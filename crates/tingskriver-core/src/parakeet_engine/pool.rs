//! Loaded Parakeet instances shared by meetings and dictation.
//!
//! An ONNX session needs `&mut` to run, so one instance can only serve one
//! call at a time. Instead of holding a lock on the engine for the whole call,
//! a call borrows an idle instance from the pool. When all instances are busy
//! (a short dictation during a long meeting segment) and the pool is below its
//! limit, one more instance is loaded; it stays loaded for the next call, so
//! the extra load is paid once. Calls block on their own thread
//! (`spawn_blocking`), never on a tokio worker.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::Instant;

use super::model::ParakeetModel;

/// Upper bound for loaded instances of one model.
pub const MAX_INSTANCES: usize = 2;

/// One loaded speech model instance.
pub trait SpeechModel: Send + 'static {
    fn transcribe(&mut self, samples: Vec<f32>) -> Result<String, String>;
}

impl SpeechModel for ParakeetModel {
    fn transcribe(&mut self, samples: Vec<f32>) -> Result<String, String> {
        self.transcribe_samples(samples)
            .map(|result| result.text)
            .map_err(|e| e.to_string())
    }
}

pub type BoxedModel = Box<dyn SpeechModel>;
pub type Loader = Box<dyn Fn() -> Result<BoxedModel, String> + Send + Sync>;

struct PoolState {
    idle: Vec<BoxedModel>,
    /// Loaded instances, idle or busy, including ones being loaded.
    total: usize,
}

pub struct ModelPool {
    loader: Loader,
    max_instances: usize,
    state: Mutex<PoolState>,
    returned: Condvar,
    created_at: Instant,
    /// Milliseconds since `created_at` of the last finished call.
    last_used_ms: AtomicU64,
}

impl ModelPool {
    /// `first` is the instance the caller already loaded; `loader` loads more.
    pub fn new(first: BoxedModel, loader: Loader, max_instances: usize) -> ModelPool {
        ModelPool {
            loader,
            max_instances: max_instances.max(1),
            state: Mutex::new(PoolState {
                idle: vec![first],
                total: 1,
            }),
            returned: Condvar::new(),
            created_at: Instant::now(),
            last_used_ms: AtomicU64::new(0),
        }
    }

    fn lock(&self) -> MutexGuard<'_, PoolState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Transcribe on the calling thread. Blocks: call from `spawn_blocking`.
    pub fn transcribe(&self, samples: Vec<f32>) -> Result<String, String> {
        let mut lease = self.checkout()?;
        let result = match lease.model.as_mut() {
            Some(model) => model.transcribe(samples),
            None => Err("Parakeet instance missing".to_string()),
        };
        self.last_used_ms.store(
            self.created_at.elapsed().as_millis() as u64,
            Ordering::Relaxed,
        );
        result
    }

    fn checkout(&self) -> Result<Lease<'_>, String> {
        let mut state = self.lock();
        loop {
            if let Some(model) = state.idle.pop() {
                return Ok(Lease {
                    pool: self,
                    model: Some(model),
                });
            }
            if state.total < self.max_instances {
                state.total += 1;
                let number = state.total;
                drop(state);
                let started = Instant::now();
                return match (self.loader)() {
                    Ok(model) => {
                        log::info!(
                            "Parakeet: all instances busy, loaded instance {number} in {} ms",
                            started.elapsed().as_millis()
                        );
                        Ok(Lease {
                            pool: self,
                            model: Some(model),
                        })
                    }
                    Err(e) => {
                        self.lock().total -= 1;
                        self.returned.notify_one();
                        Err(format!("could not load another Parakeet instance: {e}"))
                    }
                };
            }
            state = self
                .returned
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    /// Loaded instances (idle and busy).
    pub fn instances(&self) -> usize {
        self.lock().total
    }

    /// Calls running right now.
    pub fn busy(&self) -> usize {
        let state = self.lock();
        state.total - state.idle.len()
    }

    /// When the last call finished, or when the pool was created if never used.
    pub fn last_used(&self) -> Instant {
        self.created_at
            + std::time::Duration::from_millis(self.last_used_ms.load(Ordering::Relaxed))
    }
}

/// A borrowed instance; goes back to the pool when dropped. An instance that
/// panicked mid-call is dropped instead, since its state is unknown.
struct Lease<'a> {
    pool: &'a ModelPool,
    model: Option<BoxedModel>,
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        let mut state = self.pool.lock();
        match self.model.take() {
            Some(model) if !std::thread::panicking() => state.idle.push(model),
            _ => state.total -= 1,
        }
        drop(state);
        self.pool.returned.notify_one();
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;
    use std::time::Duration;

    /// Sleeps for as many milliseconds as the first sample says.
    pub(crate) struct FakeModel;

    impl SpeechModel for FakeModel {
        fn transcribe(&mut self, samples: Vec<f32>) -> Result<String, String> {
            let ms = samples.first().copied().unwrap_or(0.0) as u64;
            std::thread::sleep(Duration::from_millis(ms));
            Ok(format!("{ms} ms"))
        }
    }

    pub(crate) fn fake_pool(loads: Arc<AtomicUsize>) -> ModelPool {
        ModelPool::new(
            Box::new(FakeModel),
            Box::new(move || {
                loads.fetch_add(1, Ordering::SeqCst);
                Ok(Box::new(FakeModel) as BoxedModel)
            }),
            MAX_INSTANCES,
        )
    }

    #[test]
    fn short_call_during_long_call_does_not_wait() {
        let loads = Arc::new(AtomicUsize::new(0));
        let pool = Arc::new(fake_pool(loads.clone()));
        let long = {
            let pool = pool.clone();
            std::thread::spawn(move || pool.transcribe(vec![600.0]))
        };
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(pool.busy(), 1);

        let started = Instant::now();
        assert_eq!(pool.transcribe(vec![10.0]).as_deref(), Ok("10 ms"));
        assert!(
            started.elapsed() < Duration::from_millis(300),
            "short call waited for the long one"
        );
        assert_eq!(
            long.join().ok().and_then(|r| r.ok()).as_deref(),
            Some("600 ms")
        );

        // The extra instance stays loaded and is reused.
        assert_eq!(loads.load(Ordering::SeqCst), 1);
        assert_eq!(pool.instances(), 2);
        assert_eq!(pool.busy(), 0);
    }

    #[test]
    fn pool_never_exceeds_its_limit() {
        let loads = Arc::new(AtomicUsize::new(0));
        let pool = Arc::new(fake_pool(loads.clone()));
        let calls: Vec<_> = (0..6)
            .map(|_| {
                let pool = pool.clone();
                std::thread::spawn(move || pool.transcribe(vec![40.0]))
            })
            .collect();
        for call in calls {
            assert!(call.join().map(|r| r.is_ok()).unwrap_or(false));
        }
        assert_eq!(pool.instances(), MAX_INSTANCES);
        assert_eq!(loads.load(Ordering::SeqCst), MAX_INSTANCES - 1);
    }

    #[test]
    fn failed_extra_load_frees_its_slot() {
        let pool = Arc::new(ModelPool::new(
            Box::new(FakeModel),
            Box::new(|| Err("disk full".to_string())),
            MAX_INSTANCES,
        ));
        let long = {
            let pool = pool.clone();
            std::thread::spawn(move || pool.transcribe(vec![200.0]))
        };
        std::thread::sleep(Duration::from_millis(30));
        let error = pool.transcribe(vec![1.0]).err().unwrap_or_default();
        assert!(error.contains("disk full"), "{error}");
        assert!(long.join().map(|r| r.is_ok()).unwrap_or(false));
        assert_eq!(pool.instances(), 1);
        // The remaining instance still serves calls.
        assert_eq!(pool.transcribe(vec![1.0]).as_deref(), Ok("1 ms"));
    }

    #[test]
    fn last_used_moves_forward() {
        let pool = fake_pool(Arc::new(AtomicUsize::new(0)));
        let before = pool.last_used();
        std::thread::sleep(Duration::from_millis(20));
        let _ = pool.transcribe(vec![0.0]);
        assert!(pool.last_used() > before);
    }
}
