use super::ipc::{read_frame, write_frame, Frame};
use std::ffi::OsString;
use std::io::{self, BufReader, BufWriter};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, Once};
use std::time::Duration;

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

/// Set when a GPU worker hung or died. The rest of the session uses the CPU.
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

struct Process {
    child: Arc<Mutex<Child>>,
    pid: u32,
    input: BufWriter<ChildStdin>,
    output: BufReader<ChildStdout>,
}

impl Process {
    fn kill(&self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
    }
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
        let mut program = exe.with_file_name(name);
        // Cargo test executables live in target/{profile}/deps.
        if !program.is_file()
            && exe
                .parent()
                .and_then(|p| p.file_name())
                .is_some_and(|p| p == "deps")
        {
            program = exe.parent().unwrap().parent().unwrap().join(name);
        }
        Ok(program)
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
            .stderr(Stdio::null())
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

    /// Start the worker. A GPU worker that hangs or dies while loading is
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

    /// Only a worker that started and then hung or died counts against the GPU.
    fn worth_retrying(failure: &Failure) -> bool {
        matches!(failure, Failure::Timeout | Failure::Crash)
    }

    fn message(failure: Failure) -> String {
        match failure {
            Failure::Timeout => TIMEOUT_ERROR,
            Failure::Crash | Failure::Rejected | Failure::Spawn => CRASH_ERROR,
        }
        .to_string()
    }

    fn start(&mut self) -> Result<(), Failure> {
        let mut command = self.command(self.device);
        let mut child = command.spawn().map_err(|_| Failure::Spawn)?;
        #[cfg(windows)]
        if job::assign(child.id()).is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Failure::Crash);
        }
        let pid = child.id();
        let input = BufWriter::new(child.stdin.take().unwrap());
        let output = BufReader::new(child.stdout.take().unwrap());
        self.process = Some(Process {
            child: Arc::new(Mutex::new(child)),
            pid,
            input,
            output,
        });
        let init = self.init.clone();
        let limit = self.limits(self.device).limit(None);
        match self.exchange(&init, &[], limit) {
            Ok(response) if response.control == b"null" => Ok(()),
            Ok(_) => {
                // The worker answered with an error (for example a model that
                // will not load). That is not a crash, so no fallback.
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
            _ => {
                process.kill();
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
    use std::os::unix::fs::PermissionsExt;
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
            std::fs::write(
                &program,
                r#"#!/usr/bin/python3
import os, signal, struct, sys, time
r, w = sys.stdin.buffer, sys.stdout.buffer
def exact(n):
    result = bytearray()
    while len(result) < n:
        part = r.read(n - len(result))
        if not part: raise EOFError()
        result.extend(part)
    return result
def fault(phase):
    spec = os.environ.get('TINGSKRIVER_STT_FAULT', '')
    gpu_only = spec.endswith(':gpu')
    spec = spec.removesuffix(':gpu')
    if gpu_only and os.environ.get('TINGSKRIVER_STT_DEVICE') == 'cpu': return
    if '@' not in spec: return
    kind, at = spec.split('@')
    if at != phase: return
    if kind == 'hang':
        time.sleep(3600)
    os.kill(os.getpid(), signal.SIGABRT if kind == 'abort' else signal.SIGSEGV)
fault('init')
count = 0
while True:
    try:
        i, c, n = struct.unpack('<QII', exact(16))
        control, pcm = exact(c), exact(n * 4)
    except EOFError: break
    count += 1
    fault('load' if count == 1 else 'run')
    if control == b'hang': time.sleep(3600)
    if control == b'crash': os.kill(os.getpid(), signal.SIGKILL)
    if control == b'wrong-id': i += 1
    w.write(struct.pack('<QII', i, 4, n) + b'null' + pcm)
    w.flush()
"#,
            )
            .unwrap();
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
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
            f.client.deadlines = Some(Deadlines {
                load_floor: Duration::from_millis(800),
                run_floor: Duration::from_millis(800),
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
            self.client.process.take();
            std::fs::remove_dir_all(&self.directory).unwrap();
        }
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
        let mut f = Fixture::new();
        assert!(f.client.call(b"wrong-id", &[]).is_err());
        assert!(f.client.process.is_none());
        assert!(f.client.call(b"echo", &[]).is_ok());
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
