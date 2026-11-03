use super::ipc::{read_frame, write_frame, Frame};
use std::io::{self, BufReader, BufWriter};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

#[cfg(windows)]
#[path = "../summary/summary_engine/sidecar_job.rs"]
mod job;

pub const CRASH_ERROR: &str = "Den lokale transskriptionsproces stoppede. Prøv igen.";

struct Process {
    child: Child,
    input: BufWriter<ChildStdin>,
    output: BufReader<ChildStdout>,
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct Client {
    program: PathBuf,
    init: Vec<u8>,
    process: Option<Process>,
    next_id: u64,
}

impl Client {
    pub fn new(init: Vec<u8>) -> Result<Self, String> {
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
        let mut client = Self {
            program,
            init,
            process: None,
            next_id: 0,
        };
        client.start().map_err(|_| CRASH_ERROR.to_string())?;
        Ok(client)
    }

    fn start(&mut self) -> io::Result<()> {
        let mut command = Command::new(&self.program);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let mut child = command.spawn()?;
        #[cfg(windows)]
        if job::assign(child.id()).is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other("Could not contain STT worker"));
        }
        let input = BufWriter::new(child.stdin.take().unwrap());
        let output = BufReader::new(child.stdout.take().unwrap());
        self.process = Some(Process {
            child,
            input,
            output,
        });
        let init = self.init.clone();
        let response = self.exchange(&init, &[])?;
        if response.control != b"null" {
            self.process.take();
            return Err(io::Error::other("STT initialization failed"));
        }
        Ok(())
    }

    fn exchange(&mut self, control: &[u8], samples: &[f32]) -> io::Result<Frame> {
        self.next_id = self.next_id.wrapping_add(1);
        let process = self
            .process
            .as_mut()
            .ok_or_else(|| io::Error::other("STT not running"))?;
        write_frame(&mut process.input, self.next_id, control, samples)?;
        let response =
            read_frame(&mut process.output)?.ok_or_else(|| io::Error::other("STT exited"))?;
        if response.id != self.next_id {
            return Err(io::Error::other("STT request ID mismatch"));
        }
        Ok(response)
    }

    pub fn call(&mut self, control: &[u8], samples: &[f32]) -> Result<Frame, String> {
        let result = (|| {
            if self.process.is_none() {
                self.start()?;
            }
            self.exchange(control, samples)
        })();
        result.map_err(|_| {
            self.process.take();
            CRASH_ERROR.to_string()
        })
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

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
import os, signal, struct, sys
r, w = sys.stdin.buffer, sys.stdout.buffer
def exact(n):
    result = bytearray()
    while len(result) < n:
        part = r.read(n - len(result))
        if not part: raise EOFError()
        result.extend(part)
    return result
while True:
    try:
        i, c, n = struct.unpack('<QII', exact(16))
        control, pcm = exact(c), exact(n * 4)
    except EOFError: break
    if control == b'crash': os.kill(os.getpid(), signal.SIGKILL)
    if control == b'wrong-id': i += 1
    w.write(struct.pack('<QII', i, 4, n) + b'null' + pcm)
    w.flush()
"#,
            )
            .unwrap();
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
            let mut client = Client {
                program,
                init: b"init".to_vec(),
                process: None,
                next_id: 0,
            };
            client.start().unwrap();
            Self { directory, client }
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
        let old = f.client.process.as_ref().unwrap().child.id();
        assert_eq!(
            f.client.call(b"crash", &[0.2; 16000]).err().as_deref(),
            Some(CRASH_ERROR)
        );
        assert!(f.client.process.is_none());
        assert_eq!(f.client.call(b"echo", &[0.25]).unwrap().samples, vec![0.25]);
        assert_ne!(old, f.client.process.as_ref().unwrap().child.id());
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
}
