use std::fs;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use url::Url;

pub struct StaticFileServer {
    base_url: Url,
    stop: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl StaticFileServer {
    pub fn for_page(
        page_url: &Url,
        enabled: bool,
        bind: &str,
        port: u16,
        prefix: &str,
    ) -> Result<(Option<Self>, Url), String> {
        if page_url.scheme() != "file" || !enabled {
            return Ok((None, page_url.clone()));
        }

        let page_path = page_url
            .to_file_path()
            .map_err(|()| format!("page URL is not a valid file URL: {page_url}"))?;
        let root = page_path
            .parent()
            .ok_or_else(|| format!("page URL has no parent directory: {}", page_path.display()))?
            .canonicalize()
            .map_err(|error| format!("cannot resolve page directory: {error}"))?;
        let relative_page = page_path
            .strip_prefix(&root)
            .map_err(|error| format!("cannot map page into static root: {error}"))?;

        let server = Self::new(root, bind, port, prefix)?;
        let mut served_url = server.base_url.clone();
        let mut path = served_url.path().trim_end_matches('/').to_owned();
        path.push('/');
        path.push_str(&relative_page.to_string_lossy().replace('\\', "/"));
        served_url.set_path(&path);
        Ok((Some(server), served_url))
    }

    fn new(root: PathBuf, bind: &str, port: u16, prefix: &str) -> Result<Self, String> {
        let listener = TcpListener::bind((bind, port))
            .map_err(|error| format!("cannot bind static server: {error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("cannot configure static server: {error}"))?;
        let address = listener
            .local_addr()
            .map_err(|error| format!("cannot inspect static server address: {error}"))?;
        let prefix = normalize_prefix(prefix);
        let base_url = Url::parse(&format!("http://{address}{prefix}"))
            .map_err(|error| format!("cannot construct static server URL: {error}"))?;
        let (stop, stop_receiver) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("ped-static-http".to_owned())
            .spawn(move || serve(listener, root, prefix, stop_receiver))
            .map_err(|error| format!("cannot start static server: {error}"))?;

        Ok(Self {
            base_url,
            stop: Some(stop),
            thread: Some(thread),
        })
    }

    pub fn base_url(&self) -> &Url {
        &self.base_url
    }
}

impl Drop for StaticFileServer {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(listener: TcpListener, root: PathBuf, prefix: String, stop: Receiver<()>) {
    loop {
        if stop.try_recv().is_ok() {
            return;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = handle_request(stream, &root, &prefix);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => return,
        }
    }
}

fn handle_request(mut stream: TcpStream, root: &Path, prefix: &str) -> std::io::Result<()> {
    let mut request = [0_u8; 16 * 1024];
    let size = stream.read(&mut request)?;
    let line = std::str::from_utf8(&request[..size])
        .ok()
        .and_then(|request| request.lines().next())
        .unwrap_or_default();
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let request_path = parts.next().unwrap_or_default();
    if method != "GET" && method != "HEAD" {
        return respond(
            &mut stream,
            405,
            "text/plain",
            b"method not allowed",
            method == "HEAD",
        );
    }

    let Some(path) = request_path.split('?').next() else {
        return respond(
            &mut stream,
            400,
            "text/plain",
            b"bad request",
            method == "HEAD",
        );
    };
    let Some(relative) = path.strip_prefix(prefix) else {
        return respond(
            &mut stream,
            404,
            "text/plain",
            b"not found",
            method == "HEAD",
        );
    };
    let Some(relative) = decode_path(relative) else {
        return respond(
            &mut stream,
            400,
            "text/plain",
            b"bad path",
            method == "HEAD",
        );
    };
    let candidate = root.join(relative.trim_start_matches('/'));
    let Ok(candidate) = candidate.canonicalize() else {
        return respond(
            &mut stream,
            404,
            "text/plain",
            b"not found",
            method == "HEAD",
        );
    };
    if !candidate.starts_with(root) || !candidate.is_file() {
        return respond(
            &mut stream,
            404,
            "text/plain",
            b"not found",
            method == "HEAD",
        );
    }
    let body = fs::read(&candidate)?;
    respond(
        &mut stream,
        200,
        content_type(&candidate),
        &body,
        method == "HEAD",
    )
}

fn respond(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
    head_only: bool,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nContent-Type: {content_type}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    if !head_only {
        stream.write_all(body)?;
    }
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}

fn normalize_prefix(prefix: &str) -> String {
    let prefix = prefix.trim();
    if prefix.is_empty() || prefix == "/" {
        "/".to_owned()
    } else {
        format!("/{}/", prefix.trim_matches('/'))
    }
}

fn decode_path(path: &str) -> Option<String> {
    let mut decoded = String::with_capacity(path.len());
    let bytes = path.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return None;
            }
            let high = hex(bytes[index + 1])?;
            let low = hex(bytes[index + 2])?;
            decoded.push((high << 4 | low) as char);
            index += 3;
        } else {
            decoded.push(bytes[index] as char);
            index += 1;
        }
    }
    if decoded.split('/').any(|part| part == "..") {
        return None;
    }
    Some(decoded)
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") | Some("htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_are_normalized() {
        assert_eq!(normalize_prefix(""), "/");
        assert_eq!(normalize_prefix("/assets"), "/assets/");
        assert_eq!(normalize_prefix("assets/"), "/assets/");
    }

    #[test]
    fn traversal_is_rejected_after_decoding() {
        assert!(decode_path("/%2e%2e/secret").is_none());
        assert!(decode_path("/safe/file.js").is_some());
    }

    #[test]
    fn common_mime_types_are_available() {
        assert_eq!(
            content_type(Path::new("index.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            content_type(Path::new("app.js")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(content_type(Path::new("font.woff2")), "font/woff2");
    }
}
