use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const DEFAULT_MAX_LINE_BYTES: usize = 128 * 1024;

#[derive(Debug, Deserialize)]
pub struct Request {
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub script: Option<String>,
    #[serde(rename = "type", default)]
    pub event_type: Option<String>,
    #[serde(default)]
    pub detail: Option<Value>,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Serialize)]
pub struct Response {
    pub id: u64,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorResponse>,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub code: &'static str,
    pub message: String,
}

pub struct Command {
    pub request: Request,
    pub reply: Sender<Response>,
}

pub struct ControlServer {
    receiver: Receiver<Command>,
    stop: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
    socket: PathBuf,
    running: Arc<AtomicBool>,
}

impl ControlServer {
    pub fn bind(path: &Path, max_line_bytes: usize, queue_capacity: usize) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    format!(
                        "cannot create socket directory {}: {error}",
                        parent.display()
                    )
                })?;
            }
        }
        if path.exists() {
            let metadata = std::fs::symlink_metadata(path).map_err(|error| {
                format!("cannot inspect old socket {}: {error}", path.display())
            })?;
            if !metadata.file_type().is_socket() {
                return Err(format!("refusing to replace non-socket {}", path.display()));
            }
            std::fs::remove_file(path)
                .map_err(|error| format!("cannot remove old socket {}: {error}", path.display()))?;
        }
        let listener = UnixListener::bind(path)
            .map_err(|error| format!("cannot bind socket {}: {error}", path.display()))?;
        set_socket_permissions(path)?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("cannot configure socket: {error}"))?;

        let (commands, receiver) = mpsc::sync_channel(queue_capacity);
        let (stop, stop_receiver) = mpsc::channel();
        let running = Arc::new(AtomicBool::new(true));
        let thread_running = running.clone();
        let thread = thread::Builder::new()
            .name("ped-control".to_owned())
            .spawn(move || {
                accept_commands(
                    listener,
                    commands,
                    stop_receiver,
                    max_line_bytes,
                    thread_running,
                )
            })
            .map_err(|error| format!("cannot start control server: {error}"))?;

        Ok(Self {
            receiver,
            stop: Some(stop),
            thread: Some(thread),
            socket: path.to_owned(),
            running,
        })
    }

    pub fn try_recv(&self) -> Option<Command> {
        self.receiver.try_recv().ok()
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = std::fs::remove_file(&self.socket);
    }
}

fn accept_commands(
    listener: UnixListener,
    commands: SyncSender<Command>,
    stop: Receiver<()>,
    max_line_bytes: usize,
    running: Arc<AtomicBool>,
) {
    loop {
        if !running.load(Ordering::Acquire) || stop.try_recv().is_ok() {
            return;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let connection_commands = commands.clone();
                let connection_running = running.clone();
                let _ = thread::Builder::new()
                    .name("ped-control-client".to_owned())
                    .spawn(move || {
                        let _ = handle_connection(
                            stream,
                            &connection_commands,
                            max_line_bytes,
                            &connection_running,
                        );
                    });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => return,
        }
    }
}

fn handle_connection(
    stream: UnixStream,
    commands: &SyncSender<Command>,
    max_line_bytes: usize,
    running: &Arc<AtomicBool>,
) -> std::io::Result<()> {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    loop {
        let mut line = String::new();
        let bytes = match reader.read_line(&mut line) {
            Ok(bytes) => bytes,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut =>
            {
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        if bytes == 0 {
            return Ok(());
        }
        if bytes > max_line_bytes.min(DEFAULT_MAX_LINE_BYTES) {
            write_response(
                &mut writer,
                Response {
                    id: 0,
                    ok: false,
                    result: None,
                    error: Some(ErrorResponse {
                        code: "request_too_large",
                        message: "request exceeds the maximum size".to_owned(),
                    }),
                },
            )?;
            continue;
        }

        let request = match serde_json::from_str::<Request>(&line) {
            Ok(request) => request,
            Err(error) => {
                write_response(
                    &mut writer,
                    Response {
                        id: 0,
                        ok: false,
                        result: None,
                        error: Some(ErrorResponse {
                            code: "invalid_json",
                            message: error.to_string(),
                        }),
                    },
                )?;
                continue;
            }
        };
        let id = request.id;
        let (reply, response) = mpsc::channel();
        if commands.try_send(Command { request, reply }).is_err() {
            write_response(
                &mut writer,
                error(id, "queue_full", "PED command queue is full"),
            )?;
            continue;
        }
        let response = loop {
            match response.recv_timeout(Duration::from_millis(250)) {
                Ok(response) => break response,
                Err(mpsc::RecvTimeoutError::Timeout) if running.load(Ordering::Acquire) => {
                    continue;
                }
                Err(_) => {
                    break error(
                        id,
                        "runtime_stopped",
                        "PED stopped before handling the request",
                    );
                }
            }
        };
        write_response(&mut writer, response)?;
    }
}

fn set_socket_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("cannot restrict socket permissions: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_helpers_encode_expected_shape() {
        let response = serde_json::to_value(ok(7, serde_json::json!({"ready": true}))).unwrap();
        assert_eq!(response["id"], 7);
        assert_eq!(response["ok"], true);
        assert_eq!(response["result"]["ready"], true);
    }
}

fn write_response(writer: &mut UnixStream, response: Response) -> std::io::Result<()> {
    serde_json::to_writer(&mut *writer, &response)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

pub fn ok(id: u64, result: Value) -> Response {
    Response {
        id,
        ok: true,
        result: Some(result),
        error: None,
    }
}

pub fn error(id: u64, code: &'static str, message: impl Into<String>) -> Response {
    Response {
        id,
        ok: false,
        result: None,
        error: Some(ErrorResponse {
            code,
            message: message.into(),
        }),
    }
}
