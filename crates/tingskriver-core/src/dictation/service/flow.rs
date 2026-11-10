use super::*;

/// Content-free lifecycle notifications, independent of the overlay window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DictationState {
    Inactive,
    Recording,
    Processing,
    Inserting,
    Error,
    Cancelled,
}

/// Boundary for deterministic, network-free service integration tests.
/// Implementations must never log keys, transcripts or keyboard events.
#[async_trait::async_trait]
pub trait DictationBackend: Send + Sync + 'static {
    fn syv_key(&self) -> Option<String>;
    async fn local_available(&self) -> bool;
    async fn syv(
        &self,
        key: &str,
        settings: &DictationSettings,
        wav: Vec<u8>,
        prompt: String,
        language: String,
    ) -> Result<String, String>;
    async fn local(&self, samples: Vec<f32>) -> Result<String, String>;
    async fn insert(&self, text: String) -> Result<(), String>;
}

pub(super) struct Flow {
    states: tokio::sync::broadcast::Sender<DictationState>,
    events: Option<tokio::sync::mpsc::UnboundedSender<DictationState>>,
    current: Mutex<DictationState>,
}

impl Flow {
    pub(super) fn new(
        states: tokio::sync::broadcast::Sender<DictationState>,
        events: Option<tokio::sync::mpsc::UnboundedSender<DictationState>>,
    ) -> Self {
        let flow = Self {
            states,
            events,
            current: Mutex::new(DictationState::Inactive),
        };
        flow.set(DictationState::Inactive);
        flow
    }

    pub(super) fn set(&self, state: DictationState) {
        use DictationState::*;
        let mut current = self.current.lock().unwrap_or_else(|e| e.into_inner());
        let valid = matches!(
            (*current, state),
            (Inactive, Inactive | Recording | Error)
                | (Recording, Processing | Error | Cancelled | Inactive)
                | (Processing, Inserting | Error | Cancelled | Inactive)
                | (Inserting, Error | Cancelled | Inactive)
                | (Error | Cancelled, Inactive)
        );
        if !valid {
            return;
        }
        *current = state;
        let _ = self.states.send(state);
        if let Some(events) = &self.events {
            let _ = events.send(state);
        }
    }
}

impl Drop for Flow {
    fn drop(&mut self) {
        self.set(DictationState::Inactive);
    }
}

/// A test-triggered dictation on its own thread and runtime. Dropping it
/// cancels processing; join it to wait for teardown before removing fixtures.
pub struct DictationRun {
    pub events: tokio::sync::mpsc::UnboundedReceiver<DictationState>,
    cancel: tokio_util::sync::CancellationToken,
    thread: Option<std::thread::JoinHandle<()>>,
    service: Arc<Service>,
    ticket: u64,
}

impl DictationRun {
    pub fn cancel(&self) {
        if !is_dictating() && self.service.next_ticket.load(Ordering::SeqCst) == self.ticket + 1 {
            self.service.overlay.set(OverlayState::Hidden);
        }
        self.cancel.cancel();
    }
    pub fn join(mut self) -> std::thread::Result<()> {
        self.thread.take().unwrap().join()
    }
}

impl Drop for DictationRun {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.cancel();
        }
    }
}

impl Service {
    pub(super) async fn run_processing(
        &self,
        cancel: tokio_util::sync::CancellationToken,
        turn: &Turn,
        process: impl std::future::Future<Output = ()>,
    ) {
        tokio::pin!(process);
        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                let inserting = *turn.flow.current.lock().unwrap_or_else(|e| e.into_inner())
                    == DictationState::Inserting;
                if inserting {
                    // OS insertion cannot be recalled. Keep the turn until it completes.
                    process.await;
                } else {
                    turn.flow.set(DictationState::Cancelled);
                }
            }
            _ = &mut process => {}
        }
    }

    pub(super) fn processing_token(&self) -> tokio_util::sync::CancellationToken {
        self.processing_cancel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .child_token()
    }

    /// Cancel pending processing. An insertion already handed to the OS
    /// cannot be recalled.
    pub fn cancel_processing(&self) {
        let mut token = self
            .processing_cancel
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if !is_dictating() {
            self.overlay.set(OverlayState::Hidden);
        }
        token.cancel();
        *token = tokio_util::sync::CancellationToken::new();
    }

    /// Open an isolated service without installing hooks, starting keepers or
    /// publishing the global singleton. Uses the real store and processing.
    pub async fn open_for_test(
        app: HostHandle,
        overlay: Arc<dyn Overlay>,
        backend: Arc<dyn DictationBackend>,
    ) -> Result<Arc<Self>, String> {
        Self::open(app, overlay, Some(backend)).await
    }

    pub fn subscribe_states(&self) -> tokio::sync::broadcast::Receiver<DictationState> {
        self.states.subscribe()
    }

    /// Feed captured 16 kHz mono PCM through the ordinary processing path.
    /// No keyboard hook, microphone, host runtime or window is required.
    pub fn test_trigger(self: &Arc<Self>, samples: Vec<f32>) -> Result<DictationRun, String> {
        if self.backend.is_none() {
            return Err("Test-triggeren kræver en isoleret test-service".into());
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let (tx, events) = tokio::sync::mpsc::unbounded_channel();
        let cancel = self.processing_token();
        let token = cancel.clone();
        let ticket = self.next_ticket.fetch_add(1, Ordering::SeqCst);
        let service = self.clone();
        let thread = std::thread::Builder::new()
            .name("dictation-test".into())
            .spawn(move || {
                runtime.block_on(async move {
                    let turn = Turn {
                        ticket,
                        service: service.clone(),
                        flow: Flow::new(service.states.clone(), Some(tx)),
                    };
                    turn.flow.set(DictationState::Recording);
                    let mut settings = service.settings();
                    settings.history_enabled = false;
                    service.overlay.set(OverlayState::Processing);
                    service
                        .run_processing(
                            token,
                            &turn,
                            service.clone().process(
                                samples,
                                None,
                                0,
                                None,
                                (settings, None),
                                Instant::now(),
                                None,
                                &turn,
                            ),
                        )
                        .await;
                });
            })
            .map_err(|e| e.to_string())?;
        Ok(DictationRun {
            events,
            cancel,
            thread: Some(thread),
            service: self.clone(),
            ticket,
        })
    }
}

// Dropping processing aborts the spawned Syv request, including its test backend.
pub(super) struct AbortTask(pub tokio::task::AbortHandle);
impl Drop for AbortTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictation_capture_failure_does_not_publish_recording() {
        let (events, mut received) = tokio::sync::mpsc::unbounded_channel();
        let flow = Flow::new(tokio::sync::broadcast::channel(8).0, Some(events));
        flow.set(DictationState::Error);
        drop(flow);
        let mut states = Vec::new();
        while let Ok(state) = received.try_recv() {
            states.push(state);
        }
        assert_eq!(
            states,
            vec![
                DictationState::Inactive,
                DictationState::Error,
                DictationState::Inactive
            ]
        );
    }
}
