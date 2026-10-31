//! One running app per user.
//!
//! The first instance listens on a per-user endpoint (a named pipe on
//! Windows, a Unix socket elsewhere). A later launch connects, sends what it
//! wants (show the window) and exits. The GPUI app uses its own endpoint, so
//! it can run next to the Tauri app during the migration.

use std::io;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

/// What a later launch asks the running instance to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    /// Show and focus the main window.
    Show,
}

impl Message {
    pub fn to_line(self) -> &'static str {
        match self {
            Message::Show => "show\n",
        }
    }

    pub fn parse(line: &str) -> Option<Message> {
        match line.trim() {
            "show" => Some(Message::Show),
            _ => None,
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
        tingskriver_core::paths::APP_ID,
        if user.is_empty() { "user" } else { &user }
    )
}

fn current_user() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default()
}

async fn read_messages<R: AsyncRead + Unpin>(stream: R, tx: UnboundedSender<Message>) {
    let mut lines = BufReader::new(stream).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        match Message::parse(&line) {
            Some(message) => {
                if tx.send(message).is_err() {
                    return;
                }
            }
            None => log::warn!("Ignoring unknown single-instance message"),
        }
    }
}

/// Become the running instance, or hand `message` to the one that already
/// runs. `message` is `None` when a later launch should just exit (for
/// example `--hidden` while the app already runs). Must run inside a tokio
/// runtime; the listener keeps running on it.
pub async fn claim(message: Option<Message>) -> io::Result<Instance> {
    claim_at(&platform::default_endpoint(), message).await
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
) -> io::Result<Instance> {
    let endpoint = platform::default_endpoint();
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
    use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions};

    const ERROR_ACCESS_DENIED: i32 = 5;
    const ERROR_PIPE_BUSY: i32 = 231;

    pub fn default_endpoint() -> String {
        format!(r"\\.\pipe\{}", endpoint_name(&current_user()))
    }

    pub async fn claim_at(name: &str, message: Option<Message>) -> io::Result<Instance> {
        match ServerOptions::new().first_pipe_instance(true).create(name) {
            Ok(server) => {
                let (tx, rx) = unbounded_channel();
                tokio::spawn(serve(name.to_string(), server, tx));
                Ok(Instance::Primary(rx))
            }
            Err(e) if e.raw_os_error() == Some(ERROR_ACCESS_DENIED) => {
                if let Some(message) = message {
                    forward(name, message).await?;
                }
                Ok(Instance::Forwarded)
            }
            Err(e) => Err(e),
        }
    }

    async fn forward(name: &str, message: Message) -> io::Result<()> {
        let mut attempts = 0;
        let mut client = loop {
            match ClientOptions::new().open(name) {
                Ok(client) => break client,
                Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && attempts < 40 => {
                    attempts += 1;
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                Err(e) => return Err(e),
            }
        };
        client.write_all(message.to_line().as_bytes()).await?;
        client.flush().await
    }

    async fn serve(name: String, mut server: NamedPipeServer, tx: UnboundedSender<Message>) {
        loop {
            if let Err(e) = server.connect().await {
                log::warn!("Single-instance pipe failed: {e}");
                return;
            }
            let connected = server;
            server = match ServerOptions::new().create(&name) {
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

    pub fn default_endpoint() -> PathBuf {
        let dir = dirs::runtime_dir().unwrap_or_else(std::env::temp_dir);
        dir.join(format!("{}.sock", endpoint_name(&current_user())))
    }

    pub async fn claim_at(path: &Path, message: Option<Message>) -> io::Result<Instance> {
        match UnixStream::connect(path).await {
            Ok(mut stream) => {
                if let Some(message) = message {
                    stream.write_all(message.to_line().as_bytes()).await?;
                    stream.flush().await?;
                }
                return Ok(Instance::Forwarded);
            }
            // Nobody listens: a socket left behind by a crash.
            Err(_) => {
                let _ = std::fs::remove_file(path);
            }
        }
        let listener = UnixListener::bind(path)?;
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
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip() {
        assert_eq!(Message::parse(Message::Show.to_line()), Some(Message::Show));
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
