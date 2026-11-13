use super::ipc::{read_frame, write_frame, Frame};
use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::{self, BufReader, BufWriter, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, Once};
use std::time::{Duration, Instant};

#[cfg(windows)]
#[path = "../summary/summary_engine/sidecar_job.rs"]
mod job;

pub const CRASH_ERROR: &str = "Den lokale transskriptionsproces stoppede. Prøv igen.";
pub const TIMEOUT_ERROR: &str =
    "Den lokale transskription tog for lang tid og blev afbrudt. Prøv igen.";

/// Environment variable the worker reads to learn which device it may use.
pub const DEVICE_ENV: &str = "TINGSKRIVER_STT_DEVICE";
/// Set to `1` to keep the Vulkan implicit layers (see [`vulkan_layers_override`]).
pub const KEEP_VULKAN_LAYERS_ENV: &str = "TINGSKRIVER_KEEP_VULKAN_IMPLICIT_LAYERS";
const VULKAN_LAYERS_VAR: &str = "VK_LOADER_LAYERS_DISABLE";
const VULKAN_IMPLICIT_OFF: &str = "~implicit~";

/// Set when an automatic GPU worker fails. The rest of the session uses the CPU.
static GPU_UNAVAILABLE: AtomicBool = AtomicBool::new(false);

pub fn gpu_unavailable() -> bool {
    GPU_UNAVAILABLE.load(Ordering::Relaxed)
}

/// Which device the worker processes run on, as the user (or the code) asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevicePolicy {
    /// Use the GPU when the build has one; fall back to the CPU once if it fails.
    Auto,
    /// The user picked the GPU. A failure is reported, never retried on the CPU.
    Gpu,
    /// CPU only (also for models that never use the GPU).
    Cpu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    Gpu,
    Cpu,
}

impl Device {
    fn as_str(self) -> &'static str {
        match self {
            Device::Gpu => "gpu",
            Device::Cpu => "cpu",
        }
    }
}

/// Time limits for one call: `max(floor, factor * audio length)`.
/// The reasoning for the numbers is in `docs/stt-tidsfrister.md`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Deadlines {
    pub load_floor: Duration,
    pub run_floor: Duration,
    pub factor: f64,
}

impl Deadlines {
    pub const GPU: Self = Self {
        load_floor: Duration::from_secs(120),
        run_floor: Duration::from_secs(60),
        factor: 10.0,
    };
    pub const CPU: Self = Self {
        load_floor: Duration::from_secs(120),
        run_floor: Duration::from_secs(300),
        factor: 20.0,
    };

    pub fn for_device(device: Device) -> Self {
        match device {
            Device::Gpu => Self::GPU,
            Device::Cpu => Self::CPU,
        }
    }

    /// `audio_seconds` is `None` while the model loads.
    pub fn limit(&self, audio_seconds: Option<f64>) -> Duration {
        match audio_seconds {
            None => self.load_floor,
            Some(seconds) => self.run_floor.max(Duration::from_secs_f64(
                (seconds * self.factor).min(86_400.0),
            )),
        }
    }
}

/// Overlays and capture software (OBS and the like) install implicit Vulkan
/// layers that can crash the GPU initialisation. The value is only for the
/// worker's environment, never for the app, and only on Windows with a GPU
/// worker. A value the user has already set is kept, and
/// `TINGSKRIVER_KEEP_VULKAN_IMPLICIT_LAYERS=1` turns the override off.
pub fn vulkan_layers_override(
    windows: bool,
    device: Device,
    existing: Option<&OsString>,
    keep_layers: bool,
) -> Option<&'static str> {
    (windows && device == Device::Gpu && existing.is_none() && !keep_layers)
        .then_some(VULKAN_IMPLICIT_OFF)
}

fn log_vulkan_policy_once(device: Device) {
    static ONCE: Once = Once::new();
    if device != Device::Gpu {
        return;
    }
    ONCE.call_once(|| {
        let existing = std::env::var_os(VULKAN_LAYERS_VAR);
        let keep = keep_vulkan_layers();
        if !cfg!(windows) {
            log::info!("STT worker: Vulkan layer policy not used on this platform");
        } else if existing.is_some() {
            log::info!("STT worker: keeping the existing {VULKAN_LAYERS_VAR} value");
        } else if keep {
            log::info!("STT worker: Vulkan implicit layers kept ({KEEP_VULKAN_LAYERS_ENV}=1)");
        } else {
            log::info!(
                "STT worker: Vulkan implicit layers disabled for the worker only \
                 ({VULKAN_LAYERS_VAR}={VULKAN_IMPLICIT_OFF}); set {KEEP_VULKAN_LAYERS_ENV}=1 to keep them"
            );
        }
    });
}

fn keep_vulkan_layers() -> bool {
    std::env::var_os(KEEP_VULKAN_LAYERS_ENV).is_some_and(|v| v == "1")
}

/// How many of the worker's last stderr bytes are kept for the log.
const STDERR_TAIL_BYTES: usize = 4096;

/// The end of the worker's stderr (a panic message, an abort from ONNX
/// Runtime), kept so a crash can say what happened. A thread drains the pipe,
/// so the worker never blocks on a full stderr pipe.
struct StderrTail {
    bytes: Arc<Mutex<VecDeque<u8>>>,
    /// Disconnects when the reader thread ends (the worker closed stderr).
    finished: Option<mpsc::Receiver<()>>,
}

impl StderrTail {
    fn capture(stderr: Option<ChildStderr>) -> Self {
        let bytes = Arc::new(Mutex::new(VecDeque::new()));
        let (done, finished) = mpsc::channel::<()>();
        let sink = bytes.clone();
        let spawned = stderr.map(|mut stderr| {
            std::thread::Builder::new()
                .name("stt-stderr".into())
                .spawn(move || {
                    let _done = done;
                    let mut buffer = [0u8; 1024];
                    while let Ok(read) = stderr.read(&mut buffer) {
                        if read == 0 {
                            break;
                        }
                        if let Ok(mut tail) = sink.lock() {
                            tail.extend(&buffer[..read]);
                            let excess = tail.len().saturating_sub(STDERR_TAIL_BYTES);
                            tail.drain(..excess);
                        }
                    }
                })
        });
        Self {
            bytes,
            finished: matches!(spawned, Some(Ok(_))).then_some(finished),
        }
    }

    /// The captured text, after waiting up to `wait` for the worker to close stderr.
    fn text(&self, wait: Duration) -> String {
        if let Some(finished) = &self.finished {
            let _ = finished.recv_timeout(wait);
        }
        let bytes: Vec<u8> = self
            .bytes
            .lock()
            .map(|tail| tail.iter().copied().collect())
            .unwrap_or_default();
        String::from_utf8_lossy(&bytes).trim().to_string()
    }
}

struct Process {
    child: Arc<Mutex<Child>>,
    pid: u32,
    input: BufWriter<ChildStdin>,
    output: BufReader<ChildStdout>,
    stderr: StderrTail,
}

impl Process {
    fn kill(&self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
    }

    /// Reaps a worker that broke the pipe and says how it ended: its exit
    /// status and the end of its stderr. A worker still running after a short
    /// grace period is killed.
    fn post_mortem(&self) -> String {
        let status = match self.child.lock() {
            Ok(mut child) => {
                let grace = Instant::now() + Duration::from_millis(500);
                loop {
                    match child.try_wait() {
                        Ok(Some(status)) => break status.to_string(),
                        Ok(None) if Instant::now() < grace => {
                            std::thread::sleep(Duration::from_millis(10))
                        }
                        Ok(None) => {
                            let _ = child.kill();
                            break match child.wait() {
                                Ok(status) => format!("{status} (killed, it had closed its pipe)"),
                                Err(error) => format!("unknown exit status ({error})"),
                            };
                        }
                        Err(error) => break format!("unknown exit status ({error})"),
                    }
                }
            }
            Err(_) => "unknown exit status".to_string(),
        };
        let stderr = self.stderr.text(Duration::from_millis(500));
        if stderr.is_empty() {
            format!("{status}; stderr was empty")
        } else {
            format!("{status}; stderr:\n{stderr}")
        }
    }
}

/// Logs why a worker could not start or stopped. Test builds also print it,
/// so a failing test shows the cause next to the Danish user message.
fn report(message: &str) {
    log::warn!("STT: {message}");
    // eprintln!, not io::stderr(): only the macro goes to the test's captured output.
    #[cfg(test)]
    eprintln!("STT: {message}");
}

/// Where the worker binary is, given the running executable. The app ships it
/// next to its own executable. Cargo test executables live in
/// `target/<profile>/deps/` (Cargo before 1.100) or in
/// `target/<profile>/build/<package>/<hash>/out/` (the build-dir layout of
/// Cargo 1.100), while Cargo puts the worker binary in `target/<profile>/`.
fn worker_path(exe: &Path, name: &str, is_file: impl Fn(&Path) -> bool) -> PathBuf {
    let beside = exe.with_file_name(name);
    if is_file(&beside) {
        return beside;
    }
    let named = |dir: &Path, wanted: &str| dir.file_name().is_some_and(|n| n == wanted);
    let directory = exe.parent();
    let old_layout = directory
        .filter(|dir| named(dir, "deps"))
        .and_then(Path::parent);
    let build_dir_layout = directory
        .filter(|dir| named(dir, "out"))
        .and_then(|out| out.ancestors().nth(3))
        .filter(|build| named(build, "build"))
        .and_then(Path::parent);
    [old_layout, build_dir_layout]
        .into_iter()
        .flatten()
        .map(|profile| profile.join(name))
        .find(|candidate| is_file(candidate))
        .unwrap_or(beside)
}

impl Drop for Process {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Kills the worker when the call outlives its deadline. Killing the process
/// closes its pipes, which also unblocks a stuck read or write.
struct Watchdog {
    done: Option<mpsc::Sender<()>>,
    expired: Arc<AtomicBool>,
}

impl Watchdog {
    fn start(child: Arc<Mutex<Child>>, limit: Duration) -> Self {
        let (done, wait) = mpsc::channel::<()>();
        let expired = Arc::new(AtomicBool::new(false));
        let flag = expired.clone();
        let spawned = std::thread::Builder::new()
            .name("stt-watchdog".into())
            .spawn(move || {
                if let Err(mpsc::RecvTimeoutError::Timeout) = wait.recv_timeout(limit) {
                    flag.store(true, Ordering::SeqCst);
                    if let Ok(mut child) = child.lock() {
                        let _ = child.kill();
                    }
                }
            });
        if let Err(error) = &spawned {
            log::warn!(
                "STT: could not start the watchdog thread ({error}); no deadline for this call"
            );
        }
        Self {
            done: spawned.is_ok().then_some(done),
            expired,
        }
    }

    fn expired(&self) -> bool {
        self.expired.load(Ordering::SeqCst)
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.done.take();
    }
}

#[derive(Debug)]
enum Failure {
    Timeout,
    Crash,
    /// The worker could not be started at all (missing or blocked binary).
    /// The GPU is not to blame, so there is no CPU retry.
    Spawn,
    /// The worker answered, but with an error. Not a hang or a crash.
    Rejected,
    /// A response violated the wire protocol, without implicating the GPU.
    Protocol,
}

pub struct Client {
    program: PathBuf,
    init: Vec<u8>,
    process: Option<Process>,
    next_id: u64,
    policy: DevicePolicy,
    device: Device,
    gpu_capable: bool,
    /// Replaces the built-in time limits (tests).
    deadlines: Option<Deadlines>,
    /// Extra environment for the worker (tests).
    extra_env: Vec<(String, String)>,
}

impl Client {
    /// A worker that may use the GPU and falls back to the CPU once.
    pub fn new(init: Vec<u8>) -> Result<Self, String> {
        Self::with_policy(init, DevicePolicy::Auto)
    }

    pub fn with_policy(init: Vec<u8>, policy: DevicePolicy) -> Result<Self, String> {
        let mut client = Self::unstarted(Self::worker_program()?, init, policy);
        client.start_with_fallback()?;
        Ok(client)
    }

    fn worker_program() -> Result<PathBuf, String> {
        let exe = std::env::current_exe().map_err(|_| CRASH_ERROR)?;
        let name = if cfg!(windows) {
            "tingskriver-stt.exe"
        } else {
            "tingskriver-stt"
        };
        Ok(worker_path(&exe, name, Path::is_file))
    }

    fn unstarted(program: PathBuf, init: Vec<u8>, policy: DevicePolicy) -> Self {
        Self {
            program,
            init,
            process: None,
            next_id: 0,
            policy,
            device: Device::Cpu,
            gpu_capable: super::gpu_possible(),
            deadlines: None,
            extra_env: Vec::new(),
        }
    }

    pub fn device(&self) -> Device {
        self.device
    }

    fn pick_device(&self) -> Device {
        match self.policy {
            DevicePolicy::Cpu => Device::Cpu,
            DevicePolicy::Gpu => Device::Gpu,
            DevicePolicy::Auto => {
                if gpu_unavailable() || !self.gpu_capable {
                    Device::Cpu
                } else {
                    Device::Gpu
                }
            }
        }
    }

    fn can_fall_back(&self) -> bool {
        self.policy == DevicePolicy::Auto && self.device == Device::Gpu
    }

    fn mark_gpu_unavailable(&self) {
        if !GPU_UNAVAILABLE.swap(true, Ordering::Relaxed) {
            log::warn!("STT: the GPU worker failed; using the CPU for the rest of the session");
        }
    }

    fn limits(&self, device: Device) -> Deadlines {
        self.deadlines
            .unwrap_or_else(|| Deadlines::for_device(device))
    }

    fn command(&self, device: Device) -> Command {
        let mut command = Command::new(&self.program);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env(DEVICE_ENV, device.as_str());
        for (key, value) in &self.extra_env {
            command.env(key, value);
        }
        if let Some(value) = vulkan_layers_override(
            cfg!(windows),
            device,
            std::env::var_os(VULKAN_LAYERS_VAR).as_ref(),
            keep_vulkan_layers(),
        ) {
            command.env(VULKAN_LAYERS_VAR, value);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        command
    }

    /// Start the worker. A GPU worker that rejects init, hangs or dies is
    /// retried once on the CPU, unless the user chose the GPU.
    fn start_with_fallback(&mut self) -> Result<(), String> {
        self.device = self.pick_device();
        log_vulkan_policy_once(self.device);
        match self.start() {
            Ok(()) => Ok(()),
            Err(failure) if self.can_fall_back() && Self::worth_retrying(&failure) => {
                log::warn!("STT: GPU worker failed to start ({failure:?}); retrying on the CPU");
                self.mark_gpu_unavailable();
                self.device = Device::Cpu;
                self.start().map_err(Self::message)
            }
            Err(failure) => Err(Self::message(failure)),
        }
    }

    /// Rejected initialization can indicate an unavailable GPU or insufficient VRAM.
    fn worth_retrying(failure: &Failure) -> bool {
        matches!(
            failure,
            Failure::Timeout | Failure::Crash | Failure::Rejected
        )
    }

    fn message(failure: Failure) -> String {
        match failure {
            Failure::Timeout => TIMEOUT_ERROR,
            Failure::Crash | Failure::Rejected | Failure::Spawn | Failure::Protocol => CRASH_ERROR,
        }
        .to_string()
    }

    fn start(&mut self) -> Result<(), Failure> {
        let mut command = self.command(self.device);
        let mut child = command.spawn().map_err(|error| {
            report(&format!(
                "could not start the worker {}: {error}",
                self.program.display()
            ));
            Failure::Spawn
        })?;
        #[cfg(windows)]
        if job::assign(child.id()).is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Failure::Crash);
        }
        let pid = child.id();
        let input = BufWriter::new(child.stdin.take().unwrap());
        let output = BufReader::new(child.stdout.take().unwrap());
        let stderr = StderrTail::capture(child.stderr.take());
        self.process = Some(Process {
            child: Arc::new(Mutex::new(child)),
            pid,
            input,
            output,
            stderr,
        });
        let init = self.init.clone();
        let limit = self.limits(self.device).limit(None);
        match self.exchange(&init, &[], limit) {
            Ok(response) if response.control == b"null" => Ok(()),
            Ok(_) => {
                self.process.take();
                Err(Failure::Rejected)
            }
            Err(failure) => {
                self.process.take();
                Err(failure)
            }
        }
    }

    fn exchange(
        &mut self,
        control: &[u8],
        samples: &[f32],
        limit: Duration,
    ) -> Result<Frame, Failure> {
        self.next_id = self.next_id.wrapping_add(1);
        let id = self.next_id;
        let process = self.process.as_mut().ok_or(Failure::Crash)?;
        let watchdog = Watchdog::start(process.child.clone(), limit);
        let result = (|| {
            write_frame(&mut process.input, id, control, samples)?;
            read_frame(&mut process.output)?.ok_or_else(|| io::Error::other("STT exited"))
        })();
        let expired = watchdog.expired();
        drop(watchdog);
        match result {
            Ok(response) if !expired && response.id == id => Ok(response),
            _ if expired => {
                log::warn!(
                    "STT: worker {} passed its deadline of {:?}; killed",
                    process.pid,
                    limit
                );
                process.kill();
                Err(Failure::Timeout)
            }
            Ok(_) => {
                process.kill();
                Err(Failure::Protocol)
            }
            Err(error) => {
                let ending = process.post_mortem();
                report(&format!(
                    "worker {} stopped ({error}): {ending}",
                    process.pid
                ));
                Err(Failure::Crash)
            }
        }
    }

    pub fn call(&mut self, control: &[u8], samples: &[f32]) -> Result<Frame, String> {
        let seconds = samples.len() as f64 / 16_000.0;
        let mut retried = false;
        loop {
            let attempt = (|| {
                if self.process.is_none() {
                    if self.policy == DevicePolicy::Auto {
                        self.device = self.pick_device();
                    }
                    self.start()?;
                }
                let limit = self.limits(self.device).limit(Some(seconds));
                self.exchange(control, samples, limit)
            })();
            match attempt {
                Ok(frame) => return Ok(frame),
                Err(failure) => {
                    self.process.take();
                    if !retried && self.can_fall_back() && Self::worth_retrying(&failure) {
                        retried = true;
                        log::warn!(
                            "STT: GPU worker failed ({failure:?}); retrying once on the CPU"
                        );
                        self.mark_gpu_unavailable();
                        self.device = Device::Cpu;
                        continue;
                    }
                    return Err(Self::message(failure));
                }
            }
        }
    }
}

#[cfg(all(test, unix))]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex as TestMutex;
    use std::time::Instant;

    /// The GPU-unavailable flag is process-wide; tests that touch it run one at a time.
    static GPU_STATE: TestMutex<()> = TestMutex::new(());

    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Fixture {
        directory: PathBuf,
        client: Client,
    }
    impl Fixture {
        fn new() -> Self {
            let directory = std::env::temp_dir().join(format!(
                "stt-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&directory).unwrap();
            let program = directory.join("worker");
            // Execute an immutable script: concurrently spawning tests must not
            // inherit a writable descriptor to another test's executable.
            std::os::unix::fs::symlink(
                concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/stt-worker.py"),
                &program,
            )
            .unwrap();
            let mut client = Client::unstarted(program, b"init".to_vec(), DevicePolicy::Cpu);
            client.start().unwrap();
            Self { directory, client }
        }

        /// A fake worker that misbehaves as `fault` says, with short deadlines.
        fn faulty(
            fault: Option<&str>,
            policy: DevicePolicy,
            gpu_capable: bool,
        ) -> Result<Self, String> {
            let mut f = Self::unstarted();
            f.client.policy = policy;
            f.client.gpu_capable = gpu_capable;
            // Only a phase the fault hangs in gets the short deadline. A crash or
            // rejection must surface as itself even when a busy CI runner needs
            // more than 800 ms to start the Python fixture.
            let deadline = |phase: &str| match fault {
                Some(fault) if fault.starts_with(&format!("hang@{phase}")) => {
                    Duration::from_millis(800)
                }
                _ => Duration::from_secs(30),
            };
            f.client.deadlines = Some(Deadlines {
                load_floor: deadline("load"),
                run_floor: deadline("run"),
                factor: 0.0,
            });
            if let Some(fault) = fault {
                f.client
                    .extra_env
                    .push(("TINGSKRIVER_STT_FAULT".into(), fault.into()));
            }
            f.client.start_with_fallback()?;
            Ok(f)
        }

        fn unstarted() -> Self {
            let mut f = Self::new();
            f.client.process.take();
            f
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            // Process::drop kills and waits before we remove the worker's files.
            drop(self.client.process.take());
            if let Err(error) = std::fs::remove_dir_all(&self.directory) {
                use std::io::Write;
                let _ = writeln!(
                    io::stderr().lock(),
                    "Failed to remove STT fixture directory {}: {error}",
                    self.directory.display()
                );
            }
        }
    }

    #[test]
    fn fixture_cleanup_error_does_not_panic() {
        let mut f = Fixture::new();
        let directory = f.directory.clone();
        // A file in place of the directory forces remove_dir_all to fail.
        f.directory = directory.join("not-a-directory");
        std::fs::write(&f.directory, b"not a directory").unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(f)));
        std::fs::remove_dir_all(directory).unwrap();
        assert!(result.is_ok());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn fixture_drop_reaps_worker_and_removes_directory() {
        let f = Fixture::new();
        let pid = f.client.process.as_ref().unwrap().pid;
        let directory = f.directory.clone();
        drop(f);
        assert!(!PathBuf::from(format!("/proc/{pid}")).exists());
        assert!(!directory.exists());
    }

    #[test]
    fn killed_worker_fails_in_danish_and_next_call_restarts_it() {
        let mut f = Fixture::new();
        let old = f.client.process.as_ref().unwrap().pid;
        assert_eq!(
            f.client.call(b"crash", &[0.2; 16000]).err().as_deref(),
            Some(CRASH_ERROR)
        );
        assert!(f.client.process.is_none());
        assert_eq!(f.client.call(b"echo", &[0.25]).unwrap().samples, vec![0.25]);
        assert_ne!(old, f.client.process.as_ref().unwrap().pid);
    }

    #[test]
    fn wrong_request_id_discards_worker() {
        let _guard = GPU_STATE.lock().unwrap_or_else(|e| e.into_inner());
        GPU_UNAVAILABLE.store(false, Ordering::Relaxed);
        let mut f = Fixture::faulty(None, DevicePolicy::Auto, true).unwrap();
        assert!(f.client.call(b"wrong-id", &[]).is_err());
        assert!(f.client.process.is_none());
        assert!(!gpu_unavailable());
        assert!(f.client.call(b"echo", &[]).is_ok());
        assert_eq!(f.client.device(), Device::Gpu);
    }

    #[test]
    fn rejected_gpu_init_falls_back_only_under_auto() {
        let _guard = GPU_STATE.lock().unwrap_or_else(|e| e.into_inner());
        GPU_UNAVAILABLE.store(false, Ordering::Relaxed);
        let error = Fixture::faulty(Some("reject@load:gpu"), DevicePolicy::Gpu, true)
            .err()
            .unwrap();
        assert_eq!(error, CRASH_ERROR);
        assert!(!gpu_unavailable());
        let mut f = Fixture::faulty(Some("reject@load:gpu"), DevicePolicy::Auto, true).unwrap();
        assert_eq!(f.client.device(), Device::Cpu);
        assert!(gpu_unavailable());
        assert!(f.client.call(b"echo", &[]).is_ok());
        GPU_UNAVAILABLE.store(false, Ordering::Relaxed);
    }

    #[test]
    fn fifteen_seconds_pcm_roundtrip_stays_below_fifty_ms() {
        let mut f = Fixture::new();
        let samples = vec![0.125; 15 * 16000];
        f.client.call(b"echo", &samples).unwrap();
        let mut measurements = Vec::new();
        for _ in 0..15 {
            let start = Instant::now();
            let frame = f.client.call(b"echo", &samples).unwrap();
            measurements.push(start.elapsed());
            assert_eq!(frame.samples, samples);
        }
        measurements.sort();
        eprintln!(
            "15 s PCM IPC roundtrip: p50={:?}, p95={:?}",
            measurements[7], measurements[14]
        );
        if !cfg!(debug_assertions) {
            assert!(measurements[14].as_millis() < 50);
        }
    }

    fn pid(f: &Fixture) -> u32 {
        f.client.process.as_ref().unwrap().pid
    }

    #[test]
    fn hang_at_run_times_out_in_danish_and_next_call_starts_a_new_process() {
        let mut f = Fixture::faulty(None, DevicePolicy::Cpu, false).unwrap();
        // The hang comes from the call itself, not from a fault, so shorten the run deadline here.
        f.client.deadlines.as_mut().unwrap().run_floor = Duration::from_millis(800);
        let old = pid(&f);
        let start = Instant::now();
        let error = f.client.call(b"hang", &[0.1; 160]).err();
        assert_eq!(error.as_deref(), Some(TIMEOUT_ERROR));
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "{:?}",
            start.elapsed()
        );
        assert!(f.client.process.is_none());
        assert_eq!(f.client.call(b"echo", &[0.25]).unwrap().samples, vec![0.25]);
        assert_ne!(old, pid(&f));
    }

    #[test]
    fn injected_hang_at_run_via_environment_variable() {
        let mut f = Fixture::faulty(Some("hang@run"), DevicePolicy::Cpu, false).unwrap();
        assert_eq!(
            f.client.call(b"echo", &[0.1]).err().as_deref(),
            Some(TIMEOUT_ERROR)
        );
    }

    #[test]
    fn missing_worker_binary_does_not_mark_the_gpu_unavailable() {
        let _guard = GPU_STATE.lock().unwrap_or_else(|e| e.into_inner());
        GPU_UNAVAILABLE.store(false, Ordering::Relaxed);
        let mut client = Client::unstarted(
            PathBuf::from("/nonexistent/tingskriver-stt"),
            b"init".to_vec(),
            DevicePolicy::Auto,
        );
        client.gpu_capable = true;
        assert_eq!(
            client.start_with_fallback().err().as_deref(),
            Some(CRASH_ERROR)
        );
        assert!(!gpu_unavailable());
    }

    #[test]
    fn crashed_worker_reports_its_exit_status_and_stderr() {
        let mut f = Fixture::faulty(Some("segv@run"), DevicePolicy::Cpu, false).unwrap();
        let limit = Duration::from_secs(5);
        assert!(matches!(
            f.client.exchange(b"echo", &[0.5], limit),
            Err(Failure::Crash)
        ));
        let ending = f.client.process.as_ref().unwrap().post_mortem();
        assert!(ending.contains("SIGSEGV"), "{ending}");
        assert!(ending.contains("fake worker: segv at run"), "{ending}");
    }

    #[test]
    fn worker_is_found_in_both_cargo_test_layouts() {
        let worker = PathBuf::from("/t/debug/tingskriver-stt");
        let exists = |path: &Path| path == worker;
        // Cargo before 1.100: target/debug/deps/<test>.
        assert_eq!(
            worker_path(Path::new("/t/debug/deps/core-1"), "tingskriver-stt", exists),
            worker
        );
        // Cargo 1.100: target/debug/build/<package>/<hash>/out/<test>.
        assert_eq!(
            worker_path(
                Path::new("/t/debug/build/tingskriver-core/0a1b/out/core-1"),
                "tingskriver-stt",
                exists
            ),
            worker
        );
        // The app: the worker sits next to the executable, nothing else is searched.
        let app = PathBuf::from("/app/tingskriver-stt");
        assert_eq!(
            worker_path(Path::new("/app/tingskriver-app"), "tingskriver-stt", |p| {
                p == app || p == worker
            }),
            app
        );
        // A missing worker resolves next to the executable, so the spawn error names it.
        assert_eq!(
            worker_path(
                Path::new("/x/build/p/h/out/core-1"),
                "tingskriver-stt",
                |_| false
            ),
            PathBuf::from("/x/build/p/h/out/tingskriver-stt")
        );
    }

    #[test]
    fn deadline_scales_with_audio_and_has_a_floor() {
        let cpu = Deadlines::CPU;
        assert_eq!(cpu.limit(None), Duration::from_secs(120));
        assert_eq!(cpu.limit(Some(5.0)), Duration::from_secs(300));
        assert_eq!(cpu.limit(Some(60.0)), Duration::from_secs(1200));
        let gpu = Deadlines::GPU;
        assert_eq!(gpu.limit(None), Duration::from_secs(120));
        assert_eq!(gpu.limit(Some(1.0)), Duration::from_secs(60));
        assert_eq!(gpu.limit(Some(60.0)), Duration::from_secs(600));
    }

    #[test]
    fn gpu_worker_aborting_at_load_gets_one_cpu_attempt_and_gpu_is_marked_unavailable() {
        let _guard = GPU_STATE.lock().unwrap_or_else(|e| e.into_inner());
        GPU_UNAVAILABLE.store(false, Ordering::Relaxed);
        let mut f = Fixture::faulty(Some("abort@load:gpu"), DevicePolicy::Auto, true).unwrap();
        assert_eq!(f.client.device(), Device::Cpu);
        assert!(gpu_unavailable());
        assert_eq!(f.client.call(b"echo", &[0.5]).unwrap().samples, vec![0.5]);
        // The next client in the session starts on the CPU straight away.
        let next = Fixture::faulty(Some("abort@load:gpu"), DevicePolicy::Auto, true).unwrap();
        assert_eq!(next.client.device(), Device::Cpu);
        GPU_UNAVAILABLE.store(false, Ordering::Relaxed);
    }

    #[test]
    fn gpu_hang_at_run_retries_once_on_cpu() {
        let _guard = GPU_STATE.lock().unwrap_or_else(|e| e.into_inner());
        GPU_UNAVAILABLE.store(false, Ordering::Relaxed);
        let mut f = Fixture::faulty(Some("hang@run:gpu"), DevicePolicy::Auto, true).unwrap();
        assert_eq!(f.client.device(), Device::Gpu);
        let frame = f.client.call(b"echo", &[0.5]).unwrap();
        assert_eq!(frame.samples, vec![0.5]);
        assert_eq!(f.client.device(), Device::Cpu);
        assert!(gpu_unavailable());
        GPU_UNAVAILABLE.store(false, Ordering::Relaxed);
    }

    #[test]
    fn explicitly_chosen_gpu_never_falls_back() {
        let _guard = GPU_STATE.lock().unwrap_or_else(|e| e.into_inner());
        GPU_UNAVAILABLE.store(false, Ordering::Relaxed);
        let error = Fixture::faulty(Some("abort@load:gpu"), DevicePolicy::Gpu, true)
            .err()
            .unwrap();
        assert_eq!(error, CRASH_ERROR);
        assert!(!gpu_unavailable());
        let mut f = Fixture::faulty(Some("hang@run:gpu"), DevicePolicy::Gpu, true).unwrap();
        assert_eq!(
            f.client.call(b"echo", &[0.5]).err().as_deref(),
            Some(TIMEOUT_ERROR)
        );
        assert_eq!(f.client.device(), Device::Gpu);
        assert!(!gpu_unavailable());
    }

    #[test]
    fn cpu_failure_is_not_retried_and_does_not_touch_the_gpu_flag() {
        let _guard = GPU_STATE.lock().unwrap_or_else(|e| e.into_inner());
        GPU_UNAVAILABLE.store(false, Ordering::Relaxed);
        let error = Fixture::faulty(Some("segv@load"), DevicePolicy::Cpu, false)
            .err()
            .unwrap();
        assert_eq!(error, CRASH_ERROR);
        assert!(!gpu_unavailable());
    }

    #[test]
    fn vulkan_implicit_layers_are_off_only_for_a_gpu_worker_on_windows() {
        let set = OsString::from("VK_LAYER_X");
        let off = Some("~implicit~");
        assert_eq!(vulkan_layers_override(true, Device::Gpu, None, false), off);
        assert_eq!(vulkan_layers_override(true, Device::Cpu, None, false), None);
        assert_eq!(
            vulkan_layers_override(false, Device::Gpu, None, false),
            None
        );
        // A value the user has set is kept, and the exception variable wins.
        assert_eq!(
            vulkan_layers_override(true, Device::Gpu, Some(&set), false),
            None
        );
        assert_eq!(vulkan_layers_override(true, Device::Gpu, None, true), None);
    }

    #[test]
    fn worker_command_gets_the_device_and_never_changes_the_apps_own_environment() {
        let f = Fixture::unstarted();
        let command = f.client.command(Device::Gpu);
        let envs: Vec<_> = command.get_envs().collect();
        assert!(envs
            .iter()
            .any(|(k, v)| *k == DEVICE_ENV && *v == Some("gpu".as_ref())));
        assert_eq!(
            envs.iter().any(|(k, _)| *k == VULKAN_LAYERS_VAR),
            cfg!(windows) && std::env::var_os(VULKAN_LAYERS_VAR).is_none() && !keep_vulkan_layers()
        );
    }
}
