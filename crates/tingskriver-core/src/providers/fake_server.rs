//! A fake OpenAI-compatible server for contract tests: each connection gets
//! the next scripted reply (the last one repeats), and the raw requests are
//! kept for inspection.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Clone, Debug)]
pub enum Reply {
    Respond {
        status: &'static str,
        headers: Vec<(&'static str, String)>,
        body: String,
    },
    /// Read the request, then never answer.
    Hang,
    /// Send the headers with a body length, then never send the body.
    HangAfterHeaders,
}

impl Reply {
    pub fn json(status: &'static str, body: impl Into<String>) -> Reply {
        Reply::Respond {
            status,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    pub fn ok(body: impl Into<String>) -> Reply {
        Reply::json("200 OK", body)
    }

    pub fn with_header(self, name: &'static str, value: impl Into<String>) -> Reply {
        match self {
            Reply::Respond {
                status,
                mut headers,
                body,
            } => {
                headers.push((name, value.into()));
                Reply::Respond {
                    status,
                    headers,
                    body,
                }
            }
            other => other,
        }
    }
}

pub struct FakeServer {
    pub base_url: String,
    requests: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}

impl FakeServer {
    pub async fn start(replies: Vec<Reply>) -> FakeServer {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        let task = tokio::spawn(async move {
            let mut index = 0;
            loop {
                let Ok((socket, _)) = listener.accept().await else {
                    return;
                };
                let reply = replies[index.min(replies.len() - 1)].clone();
                index += 1;
                let seen = seen.clone();
                tokio::spawn(serve(socket, reply, seen));
            }
        });
        FakeServer {
            base_url: format!("http://{addr}/v1"),
            requests,
            task,
        }
    }

    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve(mut socket: TcpStream, reply: Reply, seen: Arc<Mutex<Vec<String>>>) {
    let request = read_request(&mut socket).await;
    seen.lock().unwrap().push(request);
    match reply {
        Reply::Respond {
            status,
            headers,
            body,
        } => {
            let mut head = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n",
                body.len()
            );
            for (name, value) in headers {
                head.push_str(&format!("{name}: {value}\r\n"));
            }
            head.push_str("\r\n");
            let _ = socket.write_all(head.as_bytes()).await;
            let _ = socket.write_all(body.as_bytes()).await;
            let _ = socket.shutdown().await;
        }
        Reply::Hang => {
            tokio::time::sleep(Duration::from_secs(120)).await;
        }
        Reply::HangAfterHeaders => {
            let head = "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 1000\r\n\r\n{\"text\":";
            let _ = socket.write_all(head.as_bytes()).await;
            tokio::time::sleep(Duration::from_secs(120)).await;
        }
    }
}

async fn read_request(socket: &mut TcpStream) -> String {
    let mut request = Vec::new();
    let mut buf = [0u8; 16384];
    loop {
        let n = match socket.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        request.extend_from_slice(&buf[..n]);
        let text = String::from_utf8_lossy(&request);
        if let Some(header_end) = text.find("\r\n\r\n") {
            let length = text[..header_end]
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|v| v.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if request.len() >= header_end + 4 + length {
                break;
            }
        }
    }
    String::from_utf8_lossy(&request).to_string()
}
