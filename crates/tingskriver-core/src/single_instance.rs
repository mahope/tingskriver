//! One running app per user.
//!
//! The first instance listens on a per-user endpoint (a named pipe on
//! Windows, a Unix socket elsewhere). A later launch connects, sends what it
//! wants (show the window) and exits. The GPUI app uses its own endpoint, so
//! it can run next to the Tauri app during the migration.

use std::io;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

/// What a later launch asks the running instance to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// Show and focus the main window.
    Show,
    Command(crate::cli::Command),
}

impl Message {
    pub fn to_line(&self) -> String {
        match self {
            Message::Show => "show\n".into(),
            Message::Command(command) => format!("{}\n", command.to_argument()),
        }
    }

    pub fn parse(line: &str) -> Option<Message> {
        match line.trim() {
            "show" => Some(Message::Show),
            _ => crate::cli::Command::parse(line.trim())
                .ok()
                .map(Message::Command),
        }
    }
}

/// The outcome of [`claim`].
#[derive(Debug)]
pub enum Instance {
    /// This is the first instance; messages from later launches arrive here.
    Primary(UnboundedReceiver<Message>),
    /// Another instance is running and has been told; this one should exit.
    Forwarded,
}

/// Why claiming the endpoint or handing over failed. [`launch`] decides from
/// it whether to retry, to run on its own or to exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// Another account holds the endpoint, or its owner is not this account.
    ForeignOwner,
    /// This account's own instance holds the endpoint but runs as
    /// administrator, so a normal process can neither verify nor reach it.
    ElevatedOwner,
    /// The endpoint was busy or went away before anything was sent. Trying
    /// again is safe.
    Busy,
    /// The message was sent but not acknowledged in time. It may or may not
    /// have arrived.
    Unacknowledged,
    /// No endpoint could be created or named, so no other instance can hold
    /// one either.
    Unavailable,
    /// Anything else, with an instance possibly running.
    Other,
}

/// What a launch ends up as once [`launch`] has dealt with failures.
#[derive(Debug)]
pub enum Launch {
    /// This is the only instance; messages from later launches arrive here.
    Primary(UnboundedReceiver<Message>),
    /// The running instance has been told; this launch should exit.
    Forwarded,
    /// Another account holds the endpoint (or none can be created). This
    /// launch runs on its own and nothing is forwarded.
    Unforwarded,
}

impl From<Instance> for Launch {
    fn from(instance: Instance) -> Self {
        match instance {
            Instance::Primary(messages) => Launch::Primary(messages),
            Instance::Forwarded => Launch::Forwarded,
        }
    }
}

/// BUILTIN\Administrators: the owner of objects an elevated administrator
/// creates.
#[cfg_attr(not(windows), allow(dead_code))]
const ADMINISTRATORS_SID: &str = "S-1-5-32-544";

/// The server process of a pipe could not be inspected. Both an elevated
/// instance of this account and another account's process hide their token,
/// so the owner of the pipe object tells them apart. A different elevated
/// administrator reads as elevated too; nothing is sent to it either way.
#[cfg_attr(not(windows), allow(dead_code))]
fn owner_failure(owner_sid: &str, own_sid: &str) -> Failure {
    if owner_sid == own_sid || owner_sid == ADMINISTRATORS_SID {
        Failure::ElevatedOwner
    } else {
        Failure::ForeignOwner
    }
}

/// Opening the pipe was refused. The pipe name contains this account's own
/// SID, so a pipe that refuses this account is almost always its own instance
/// running as administrator (its integrity label or access list keeps a normal
/// process out). Nothing may start a second instance on that: the owner of the
/// pipe object, when it can be read, tells an elevated instance (exit 5);
/// otherwise the launch exits with the transport code. Never
/// [`Failure::ForeignOwner`], which a launch without a command answers by
/// running on its own.
#[cfg_attr(not(windows), allow(dead_code))]
fn open_denied_failure(owner_sid: Option<&str>, own_sid: Option<&str>) -> Failure {
    match (owner_sid, own_sid) {
        (Some(owner), Some(own)) if owner_failure(owner, own) == Failure::ElevatedOwner => {
            Failure::ElevatedOwner
        }
        _ => Failure::Other,
    }
}

/// Endpoint name for the current user, without characters that are not
/// allowed in a pipe or file name.
pub fn endpoint_name(user: &str) -> String {
    let user: String = user
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!(
        "{}.gpui-{}",
        crate::paths::APP_ID,
        if user.is_empty() { "user" } else { &user }
    )
}

#[cfg(unix)]
fn current_user() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default()
}

async fn read_messages<R: AsyncRead + AsyncWrite + Unpin>(stream: R, tx: UnboundedSender<Message>) {
    let mut reader = BufReader::new(stream.take(4097));
    let mut line = String::new();
    if matches!(
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            reader.read_line(&mut line)
        )
        .await,
        Ok(Ok(1..=4096))
    ) && line.ends_with('\n')
    {
        if let Some(message) = Message::parse(&line) {
            if tx.send(message).is_ok() {
                let _ = reader.get_mut().get_mut().write_all(&[0]).await;
            }
        }
    }
}

/// Become the running instance, or hand `message` to the one that already
/// runs. `message` is `None` when a later launch should just exit (for
/// example `--hidden` while the app already runs). Must run inside a tokio
/// runtime; the listener keeps running on it.
pub async fn claim(message: Option<Message>) -> Result<Instance, Failure> {
    let endpoint = platform::default_endpoint().map_err(|error| {
        log::debug!("Single-instance endpoint unavailable: {error}");
        Failure::Unavailable
    })?;
    claim_at(&endpoint, message).await
}

pub use platform::claim_at;

/// Set by the crash dialog's "Prøv igen" on the new process: the old one
/// still holds the endpoint for a moment and must not get the hand-over.
pub const RESTART_ENV: &str = "TINGSKRIVER_RESTARTED";

/// [`claim`] for a process started by "Prøv igen": wait up to `wait` for the
/// old instance to release the endpoint (asking it for nothing meanwhile),
/// then claim as usual.
pub async fn claim_after_restart(
    message: Option<Message>,
    wait: std::time::Duration,
) -> Result<Instance, Failure> {
    let endpoint = platform::default_endpoint().map_err(|error| {
        log::debug!("Single-instance endpoint unavailable: {error}");
        Failure::Unavailable
    })?;
    let deadline = tokio::time::Instant::now() + wait;
    while tokio::time::Instant::now() < deadline {
        if let Ok(Instance::Primary(rx)) = claim_at(&endpoint, None).await {
            return Ok(Instance::Primary(rx));
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    claim_at(&endpoint, message).await
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::time::Duration;
    use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer};

    const ERROR_FILE_NOT_FOUND: i32 = 2;
    const ERROR_ACCESS_DENIED: i32 = 5;
    const ERROR_PIPE_BUSY: i32 = 231;

    pub fn default_endpoint() -> io::Result<String> {
        Ok(format!(
            r"\\.\pipe\{}",
            endpoint_name(&super::windows_identity::sid()?)
        ))
    }

    pub async fn claim_at(name: &str, message: Option<Message>) -> Result<Instance, Failure> {
        match super::windows_identity::server(name, true) {
            Ok(server) => {
                let (tx, rx) = unbounded_channel();
                tokio::spawn(serve(name.to_string(), server, tx));
                Ok(Instance::Primary(rx))
            }
            // The pipe exists: somebody else holds it.
            Err(e) if e.raw_os_error() == Some(ERROR_ACCESS_DENIED) => {
                forward(name, message).await?;
                Ok(Instance::Forwarded)
            }
            Err(e) => {
                log::debug!("Could not create the single-instance pipe: {e}");
                Err(Failure::Unavailable)
            }
        }
    }

    async fn forward(name: &str, message: Option<Message>) -> Result<(), Failure> {
        let mut attempts = 0;
        let mut client = loop {
            match ClientOptions::new().open(name) {
                Ok(client) => break client,
                Err(e) => match e.raw_os_error() {
                    Some(ERROR_PIPE_BUSY) if attempts < 40 => {
                        attempts += 1;
                        tokio::time::sleep(Duration::from_millis(25)).await;
                    }
                    // Still busy, or the instance exited after the claim
                    // failed: another attempt decides.
                    Some(ERROR_PIPE_BUSY | ERROR_FILE_NOT_FOUND) => return Err(Failure::Busy),
                    // The name holds this account's own SID, so the pipe
                    // that refuses us is almost always our own instance
                    // running as administrator, not another account's.
                    // Doubt never starts a second instance.
                    Some(ERROR_ACCESS_DENIED) => {
                        return Err(super::windows_identity::open_denied(name));
                    }
                    _ => {
                        log::debug!("Could not open the single-instance pipe: {e}");
                        return Err(Failure::Other);
                    }
                },
            }
        };
        super::windows_identity::verify_server(&client)?;
        if let Some(message) = message {
            // Nothing arrives without its newline, so a failed write
            // delivered nothing.
            client
                .write_all(message.to_line().as_bytes())
                .await
                .map_err(|_| Failure::Busy)?;
            client.flush().await.map_err(|_| Failure::Busy)?;
            acknowledge(&mut client)
                .await
                .map_err(|_| Failure::Unacknowledged)?;
        }
        Ok(())
    }

    async fn serve(name: String, mut server: NamedPipeServer, tx: UnboundedSender<Message>) {
        loop {
            if let Err(e) = server.connect().await {
                log::warn!("Single-instance pipe failed: {e}");
                return;
            }
            let connected = server;
            server = match super::windows_identity::server(&name, false) {
                Ok(next) => next,
                Err(e) => {
                    log::warn!("Could not reopen the single-instance pipe: {e}");
                    return;
                }
            };
            tokio::spawn(read_messages(connected, tx.clone()));
        }
    }
}

#[cfg(unix)]
mod platform {
    use super::*;
    use std::path::{Path, PathBuf};
    use tokio::net::{UnixListener, UnixStream};

    pub fn default_endpoint() -> io::Result<PathBuf> {
        let dir = dirs::runtime_dir().unwrap_or_else(std::env::temp_dir);
        Ok(dir.join(format!("{}.sock", endpoint_name(&current_user()))))
    }

    /// A connect error that proves nobody listens: the socket file is
    /// refused (a crash left it behind) or gone.
    pub(super) fn socket_is_stale(kind: io::ErrorKind) -> bool {
        matches!(
            kind,
            io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
        )
    }

    #[allow(clippy::disallowed_methods)]
    pub async fn claim_at(path: &Path, message: Option<Message>) -> Result<Instance, Failure> {
        match UnixStream::connect(path).await {
            Ok(mut stream) => {
                if let Some(message) = message {
                    stream
                        .write_all(message.to_line().as_bytes())
                        .await
                        .map_err(|_| Failure::Busy)?;
                    stream.flush().await.map_err(|_| Failure::Busy)?;
                    acknowledge(&mut stream)
                        .await
                        .map_err(|_| Failure::Unacknowledged)?;
                }
                return Ok(Instance::Forwarded);
            }
            // A socket this account may not use belongs to someone else.
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                return Err(Failure::ForeignOwner);
            }
            // Nobody listens: a socket left behind by a crash. Any other
            // error (a full backlog, a race with a launch that is binding
            // right now) may be a live instance, so its socket stays and the
            // attempt is retried like a busy pipe.
            Err(e) if socket_is_stale(e.kind()) => {
                let _ = std::fs::remove_file(path);
            }
            Err(e) => {
                log::debug!("Could not reach the single-instance socket: {e}");
                return Err(Failure::Busy);
            }
        }
        let listener = UnixListener::bind(path).map_err(|e| {
            log::debug!("Could not create the single-instance socket: {e}");
            // Another launch bound it first: its instance answers next time.
            if e.kind() == io::ErrorKind::AddrInUse {
                Failure::Busy
            } else {
                Failure::Unavailable
            }
        })?;
        let (tx, rx) = unbounded_channel();
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        tokio::spawn(read_messages(stream, tx.clone()));
                    }
                    Err(e) => {
                        log::warn!("Single-instance socket failed: {e}");
                        return;
                    }
                }
            }
        });
        Ok(Instance::Primary(rx))
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip() {
        assert_eq!(
            Message::parse(&Message::Show.to_line()),
            Some(Message::Show)
        );
        assert_eq!(Message::parse("  show \r\n"), Some(Message::Show));
        assert_eq!(Message::parse("quit"), None);
    }

    #[test]
    fn endpoint_names_are_safe() {
        assert_eq!(endpoint_name("mads"), "dk.mahoje.tingskriver.gpui-mads");
        assert_eq!(
            endpoint_name(r"DOMÆNE\Søren Ås"),
            "dk.mahoje.tingskriver.gpui-DOMÆNE_Søren_Ås"
        );
        assert_eq!(endpoint_name(""), "dk.mahoje.tingskriver.gpui-user");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn second_launch_reaches_the_first() {
        let path =
            std::env::temp_dir().join(format!("tingskriver-si-test-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let Instance::Primary(mut rx) = claim_at(&path, Some(Message::Show)).await.unwrap() else {
            panic!("first launch must become the primary instance");
        };
        assert!(matches!(
            claim_at(&path, Some(Message::Show)).await.unwrap(),
            Instance::Forwarded
        ));
        assert_eq!(rx.recv().await, Some(Message::Show));

        // A hidden launch exits quietly without showing the window.
        assert!(matches!(
            claim_at(&path, None).await.unwrap(),
            Instance::Forwarded
        ));
        assert!(matches!(
            claim_at(&path, Some(Message::Show)).await.unwrap(),
            Instance::Forwarded
        ));
        assert_eq!(rx.recv().await, Some(Message::Show));
        let _ = std::fs::remove_file(&path);
    }

    // The named pipe is recreated after every client, so the third and
    // fourth launch must still reach the first instance.
    #[cfg(windows)]
    #[tokio::test]
    async fn second_launch_reaches_the_first() {
        let name = format!(r"\\.\pipe\tingskriver-si-test-{}", std::process::id());

        let Instance::Primary(mut rx) = claim_at(&name, Some(Message::Show)).await.unwrap() else {
            panic!("first launch must become the primary instance");
        };
        for _ in 0..3 {
            assert!(matches!(
                claim_at(&name, Some(Message::Show)).await.unwrap(),
                Instance::Forwarded
            ));
            assert_eq!(rx.recv().await, Some(Message::Show));
        }
        // A hidden launch exits quietly without showing the window.
        assert!(matches!(
            claim_at(&name, None).await.unwrap(),
            Instance::Forwarded
        ));
        assert!(matches!(
            claim_at(&name, Some(Message::Show)).await.unwrap(),
            Instance::Forwarded
        ));
        assert_eq!(rx.recv().await, Some(Message::Show));
    }

    // A connect error that does not prove the socket dead settles nothing:
    // the attempt is busy (retried, never a second instance).
    #[cfg(unix)]
    #[tokio::test]
    async fn unclear_connect_error_is_busy_not_a_free_endpoint() {
        let file = std::env::temp_dir().join(format!("tingskriver-si-file-{}", std::process::id()));
        std::fs::write(&file, b"not a directory").unwrap();
        // Connecting through a regular file fails with ENOTDIR.
        let result = claim_at(&file.join("x.sock"), Some(Message::Show)).await;
        assert!(matches!(result, Err(Failure::Busy)), "{result:?}");
        assert_eq!(std::fs::read(&file).unwrap(), b"not a directory");
        let _ = std::fs::remove_file(&file);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stale_socket_from_a_crash_is_replaced() {
        let path =
            std::env::temp_dir().join(format!("tingskriver-si-stale-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
        assert!(path.exists());
        assert!(matches!(
            claim_at(&path, Some(Message::Show)).await.unwrap(),
            Instance::Primary(_)
        ));
        let _ = std::fs::remove_file(&path);
    }
}

/// Injectable hand-over policy: an occupied endpoint must never start another host.
#[async_trait::async_trait]
pub trait Transport {
    async fn claim(&self, message: Option<Message>) -> Result<Instance, Failure>;
}

pub struct LocalTransport;
#[async_trait::async_trait]
impl Transport for LocalTransport {
    async fn claim(&self, message: Option<Message>) -> Result<Instance, Failure> {
        claim(message).await
    }
}

/// Pauses between attempts when the endpoint is busy or the running instance
/// does not answer in time; about five seconds in all.
pub const RETRY_DELAYS: [std::time::Duration; 6] = [
    std::time::Duration::from_millis(100),
    std::time::Duration::from_millis(200),
    std::time::Duration::from_millis(400),
    std::time::Duration::from_millis(800),
    std::time::Duration::from_millis(1500),
    std::time::Duration::from_millis(2000),
];

#[derive(Debug, PartialEq, Eq)]
enum Decision {
    Retry,
    StartAnyway,
    Exit(crate::cli::ExitCode),
}

/// What to do about a failed attempt. Only a launch without a command may
/// run on its own, and a command is never sent twice: it could have arrived
/// even though nobody acknowledged it.
fn decide(failure: Failure, message: Option<&Message>) -> Decision {
    use crate::cli::ExitCode;
    let command = matches!(message, Some(Message::Command(_)));
    match failure {
        Failure::ElevatedOwner => Decision::Exit(ExitCode::ElevatedInstance),
        Failure::ForeignOwner | Failure::Unavailable if !command => Decision::StartAnyway,
        Failure::Busy => Decision::Retry,
        Failure::Unacknowledged if !command => Decision::Retry,
        _ => Decision::Exit(ExitCode::Transport),
    }
}

/// [`launch_with`] and [`RETRY_DELAYS`].
pub async fn launch(
    transport: &impl Transport,
    message: Option<Message>,
) -> Result<Launch, crate::cli::ExitCode> {
    launch_with(transport, message, &RETRY_DELAYS).await
}

/// Claim the endpoint or hand `message` over, waiting `delays` between
/// attempts that may succeed later. Never starts a second instance while a
/// first one runs: what cannot be settled within the delays is an exit code.
pub async fn launch_with(
    transport: &impl Transport,
    message: Option<Message>,
    delays: &[std::time::Duration],
) -> Result<Launch, crate::cli::ExitCode> {
    let mut delays = delays.iter();
    loop {
        let failure = match transport.claim(message.clone()).await {
            Ok(instance) => return Ok(instance.into()),
            Err(failure) => failure,
        };
        match decide(failure, message.as_ref()) {
            Decision::StartAnyway => return Ok(Launch::Unforwarded),
            Decision::Exit(code) => return Err(code),
            Decision::Retry => match delays.next() {
                Some(delay) => {
                    log::debug!("Single-instance attempt failed ({failure:?}); trying again");
                    tokio::time::sleep(*delay).await;
                }
                None => return Err(crate::cli::ExitCode::Transport),
            },
        }
    }
}

/// The outcome of an attempt that has already had its time to succeed (the
/// grace period after a restart): nothing is retried.
pub fn settle(
    result: Result<Instance, Failure>,
    message: Option<&Message>,
) -> Result<Launch, crate::cli::ExitCode> {
    match result {
        Ok(instance) => Ok(instance.into()),
        Err(failure) => match decide(failure, message) {
            Decision::StartAnyway => Ok(Launch::Unforwarded),
            Decision::Exit(code) => Err(code),
            Decision::Retry => Err(crate::cli::ExitCode::Transport),
        },
    }
}

#[cfg(windows)]
#[path = "single_instance_windows.rs"]
mod windows_identity;

#[cfg(test)]
mod transport_tests {
    use super::*;
    use crate::cli::{Command, ExitCode};
    use std::time::Duration;

    /// Answers attempt n with `script[n]`, and the last entry for any later
    /// attempt. `Ok(true)` is an occupied endpoint, `Ok(false)` a free one.
    struct Fake {
        script: Vec<Result<bool, Failure>>,
        sent: std::sync::Mutex<Vec<Option<Message>>>,
    }

    impl Fake {
        fn new(script: Vec<Result<bool, Failure>>) -> Self {
            Self {
                script,
                sent: Default::default(),
            }
        }
        fn attempts(&self) -> usize {
            self.sent.lock().unwrap().len()
        }
    }

    #[async_trait::async_trait]
    impl Transport for Fake {
        async fn claim(&self, message: Option<Message>) -> Result<Instance, Failure> {
            let mut sent = self.sent.lock().unwrap();
            let step = self.script[sent.len().min(self.script.len() - 1)];
            sent.push(message);
            step.map(|occupied| {
                if occupied {
                    Instance::Forwarded
                } else {
                    Instance::Primary(unbounded_channel().1)
                }
            })
        }
    }

    const NO_WAIT: [Duration; 3] = [Duration::ZERO; 3];

    fn launches() -> [Option<Message>; 4] {
        [
            None,
            Some(Message::Show),
            Some(Message::Command(Command::Cancel)),
            Some(Message::Command(Command::Mode("Møde".into()))),
        ]
    }

    fn commands() -> [Option<Message>; 2] {
        [
            Some(Message::Command(Command::ToggleDictation)),
            Some(Message::Command(Command::Mode("Møde".into()))),
        ]
    }

    fn plain() -> [Option<Message>; 2] {
        [None, Some(Message::Show)]
    }

    #[tokio::test]
    async fn launch_policy_with_fake_transport() {
        for occupied in [false, true] {
            for command in launches() {
                let fake = Fake::new(vec![Ok(occupied)]);
                let result = launch_with(&fake, command.clone(), &NO_WAIT).await.unwrap();
                assert_eq!(matches!(result, Launch::Forwarded), occupied);
                assert_eq!(matches!(result, Launch::Primary(_)), !occupied);
                assert_eq!(*fake.sent.lock().unwrap(), vec![command]);
            }
        }
    }

    // (a) Another account owns the pipe: run on our own and send it nothing.
    #[tokio::test]
    async fn foreign_owner_starts_without_forwarding() {
        for message in plain() {
            let fake = Fake::new(vec![Err(Failure::ForeignOwner)]);
            let result = launch_with(&fake, message, &NO_WAIT).await.unwrap();
            assert!(matches!(result, Launch::Unforwarded));
            assert_eq!(fake.attempts(), 1);
        }
        // No endpoint can be created at all: nobody else holds one either.
        let fake = Fake::new(vec![Err(Failure::Unavailable)]);
        let result = launch_with(&fake, None, &NO_WAIT).await.unwrap();
        assert!(matches!(result, Launch::Unforwarded));
        // A command is never run without the instance it was meant for.
        for failure in [Failure::ForeignOwner, Failure::Unavailable] {
            for command in commands() {
                let fake = Fake::new(vec![Err(failure)]);
                let result = launch_with(&fake, command, &NO_WAIT).await;
                assert_eq!(result.unwrap_err(), ExitCode::Transport);
                assert_eq!(fake.attempts(), 1);
            }
        }
    }

    // (b) A busy endpoint is retried and never answered with a second instance.
    #[tokio::test]
    async fn busy_endpoint_is_retried_then_settled() {
        for message in launches() {
            // Free after two busy attempts.
            let fake = Fake::new(vec![Err(Failure::Busy), Err(Failure::Busy), Ok(true)]);
            let result = launch_with(&fake, message.clone(), &NO_WAIT).await.unwrap();
            assert!(matches!(result, Launch::Forwarded));
            assert_eq!(fake.attempts(), 3);

            let fake = Fake::new(vec![Err(Failure::Busy), Ok(false)]);
            let result = launch_with(&fake, message.clone(), &NO_WAIT).await.unwrap();
            assert!(matches!(result, Launch::Primary(_)));
            assert_eq!(fake.attempts(), 2);

            // Still busy after every delay: exit, do not start anyway.
            let fake = Fake::new(vec![Err(Failure::Busy)]);
            let result = launch_with(&fake, message, &NO_WAIT).await;
            assert_eq!(result.unwrap_err(), ExitCode::Transport);
            assert_eq!(fake.attempts(), NO_WAIT.len() + 1);
        }
    }

    #[tokio::test]
    async fn silent_instance_is_retried_only_when_that_cannot_repeat_a_command() {
        for message in plain() {
            let fake = Fake::new(vec![Err(Failure::Unacknowledged), Ok(true)]);
            let result = launch_with(&fake, message.clone(), &NO_WAIT).await.unwrap();
            assert!(matches!(result, Launch::Forwarded));
            assert_eq!(fake.attempts(), 2);

            let fake = Fake::new(vec![Err(Failure::Unacknowledged)]);
            let result = launch_with(&fake, message, &NO_WAIT).await;
            assert_eq!(result.unwrap_err(), ExitCode::Transport);
            assert_eq!(fake.attempts(), NO_WAIT.len() + 1);
        }
        // The command may have arrived: sending it again could toggle twice.
        for command in commands() {
            let fake = Fake::new(vec![Err(Failure::Unacknowledged), Ok(true)]);
            let result = launch_with(&fake, command, &NO_WAIT).await;
            assert_eq!(result.unwrap_err(), ExitCode::Transport);
            assert_eq!(fake.attempts(), 1);
        }
    }

    // (c) The account's own instance runs as administrator: it is running,
    // so exit with the documented code instead of starting a second one.
    #[tokio::test]
    async fn elevated_instance_is_reported_not_duplicated() {
        for message in launches() {
            let fake = Fake::new(vec![Err(Failure::ElevatedOwner)]);
            let result = launch_with(&fake, message, &NO_WAIT).await;
            assert_eq!(result.unwrap_err(), ExitCode::ElevatedInstance);
            assert_eq!(fake.attempts(), 1);
        }
        assert!(ExitCode::ElevatedInstance.explanation().is_some());
    }

    #[tokio::test]
    async fn unknown_failures_exit_with_the_transport_code() {
        for message in launches() {
            let fake = Fake::new(vec![Err(Failure::Other)]);
            let result = launch_with(&fake, message, &NO_WAIT).await;
            assert_eq!(result.unwrap_err(), ExitCode::Transport);
            assert_eq!(fake.attempts(), 1);
        }
    }

    // The default delays add up to about five seconds, and every failure
    // has an answer: no combination ends up retried without end.
    #[test]
    fn decisions_and_the_retry_window() {
        let total: Duration = RETRY_DELAYS.iter().sum();
        assert!(total >= Duration::from_secs(4) && total <= Duration::from_secs(5));
        let cancel = Some(Message::Command(Command::Cancel));
        for (failure, plain_launch, command) in [
            (
                Failure::ForeignOwner,
                Decision::StartAnyway,
                Decision::Exit(ExitCode::Transport),
            ),
            (
                Failure::Unavailable,
                Decision::StartAnyway,
                Decision::Exit(ExitCode::Transport),
            ),
            (
                Failure::ElevatedOwner,
                Decision::Exit(ExitCode::ElevatedInstance),
                Decision::Exit(ExitCode::ElevatedInstance),
            ),
            (Failure::Busy, Decision::Retry, Decision::Retry),
            (
                Failure::Unacknowledged,
                Decision::Retry,
                Decision::Exit(ExitCode::Transport),
            ),
            (
                Failure::Other,
                Decision::Exit(ExitCode::Transport),
                Decision::Exit(ExitCode::Transport),
            ),
        ] {
            assert_eq!(decide(failure, None), plain_launch, "{failure:?}");
            assert_eq!(decide(failure, Some(&Message::Show)), plain_launch);
            assert_eq!(decide(failure, cancel.as_ref()), command, "{failure:?}");
        }
    }

    // After a restart the grace period was the retry window.
    #[test]
    fn settle_does_not_retry() {
        assert!(matches!(
            settle(Err(Failure::Busy), None),
            Err(ExitCode::Transport)
        ));
        assert!(matches!(
            settle(Err(Failure::ForeignOwner), Some(&Message::Show)),
            Ok(Launch::Unforwarded)
        ));
        assert!(matches!(
            settle(Err(Failure::ElevatedOwner), None),
            Err(ExitCode::ElevatedInstance)
        ));
        assert!(matches!(
            settle(Ok(Instance::Forwarded), None),
            Ok(Launch::Forwarded)
        ));
    }

    // Of a pipe whose server cannot be inspected, the owner of the pipe object
    // tells an elevated instance of this account from another account.
    #[test]
    fn unverifiable_server_is_told_apart_by_the_pipe_owner() {
        let own = "S-1-5-21-1-2-3-1001";
        assert_eq!(
            owner_failure(ADMINISTRATORS_SID, own),
            Failure::ElevatedOwner
        );
        assert_eq!(owner_failure(own, own), Failure::ElevatedOwner);
        assert_eq!(
            owner_failure("S-1-5-21-1-2-3-1002", own),
            Failure::ForeignOwner
        );
    }

    // A pipe that refuses this account at open carries this account's own
    // SID in its name. Whatever the owner turns out to be, no launch may
    // start a second instance on that.
    #[tokio::test]
    async fn refused_open_never_starts_a_second_instance() {
        let own = "S-1-5-21-1-2-3-1001";
        for (owner, expected) in [
            (Some(ADMINISTRATORS_SID), ExitCode::ElevatedInstance),
            (Some(own), ExitCode::ElevatedInstance),
            (Some("S-1-5-21-1-2-3-1002"), ExitCode::Transport),
            (None, ExitCode::Transport),
        ] {
            let failure = open_denied_failure(owner, Some(own));
            assert_ne!(failure, Failure::ForeignOwner);
            assert_ne!(failure, Failure::Unavailable);
            for message in launches() {
                let fake = Fake::new(vec![Err(failure)]);
                let result = launch_with(&fake, message.clone(), &NO_WAIT).await;
                assert_eq!(result.unwrap_err(), expected, "{owner:?}");
                assert_eq!(fake.attempts(), 1);
                assert!(matches!(
                    settle(Err(failure), message.as_ref()),
                    Err(code) if code == expected
                ));
            }
        }
        // Without an own SID nothing can be compared: exit 3.
        assert_eq!(
            open_denied_failure(Some(ADMINISTRATORS_SID), None),
            Failure::Other
        );
    }

    // Only a connect error that proves nobody listens may delete the socket;
    // a full backlog or a race with a launch that is binding is a live one.
    #[cfg(unix)]
    #[test]
    fn only_a_dead_socket_is_deleted() {
        use std::io::ErrorKind;
        assert!(platform::socket_is_stale(ErrorKind::ConnectionRefused));
        assert!(platform::socket_is_stale(ErrorKind::NotFound));
        for kind in [
            ErrorKind::WouldBlock,
            ErrorKind::Interrupted,
            ErrorKind::TimedOut,
            ErrorKind::PermissionDenied,
            ErrorKind::ConnectionReset,
            ErrorKind::Other,
        ] {
            assert!(!platform::socket_is_stale(kind), "{kind:?}");
        }
    }

    #[tokio::test]
    async fn wire_accepts_only_bounded_commands() {
        for command in [Command::Cancel, Command::Mode("Møde noter".into())] {
            let (mut client, server) = tokio::io::duplex(8192);
            let (tx, mut rx) = unbounded_channel();
            client
                .write_all(Message::Command(command.clone()).to_line().as_bytes())
                .await
                .unwrap();
            read_messages(server, tx).await;
            assert_eq!(rx.recv().await, Some(Message::Command(command)));
        }
        for invalid in ["--exec\n".to_string(), "a".repeat(5000) + "\n"] {
            let (mut client, server) = tokio::io::duplex(8192);
            let (tx, mut rx) = unbounded_channel();
            client.write_all(invalid.as_bytes()).await.unwrap();
            read_messages(server, tx).await;
            assert_eq!(rx.recv().await, None);
        }
    }
}

async fn acknowledge(stream: &mut (impl AsyncRead + Unpin)) -> io::Result<()> {
    match tokio::time::timeout(std::time::Duration::from_secs(2), stream.read_u8()).await {
        Ok(Ok(0)) => Ok(()),
        _ => Err(io::Error::other("command was not acknowledged")),
    }
}
