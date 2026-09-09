use std::env;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use serde_json::{Value, json};

fn main() {
    let mut args = env::args().skip(1);
    let mut socket = PathBuf::from("/tmp/ped.sock");
    let mut positional = Vec::new();
    while let Some(argument) = args.next() {
        if argument == "--socket" {
            socket = PathBuf::from(args.next().unwrap_or_else(|| {
                eprintln!("ped: --socket requires a path");
                std::process::exit(2);
            }));
        } else {
            positional.push(argument);
        }
    }

    let Some(method) = positional.first().map(String::as_str) else {
        usage();
    };
    let request = match method {
        "status" | "stop" | "refresh" | "reload-config" => json!({"id": 1, "method": method}),
        "open" => {
            let Some(url) = positional.get(1) else {
                eprintln!("ped: open requires a URL");
                std::process::exit(2);
            };
            json!({"id": 1, "method": "open", "url": url})
        }
        "eval" => {
            let Some(script) = positional.get(1) else {
                eprintln!("ped: eval requires JavaScript");
                std::process::exit(2);
            };
            json!({"id": 1, "method": "eval", "script": script})
        }
        "emit" => {
            let Some(event_type) = positional.get(1) else {
                eprintln!("ped: emit requires an event type");
                std::process::exit(2);
            };
            let detail = positional
                .get(2)
                .map(|detail| serde_json::from_str(detail).unwrap_or(Value::String(detail.clone())))
                .unwrap_or(Value::Null);
            json!({"id": 1, "method": "emit", "type": event_type, "detail": detail})
        }
        _ => usage(),
    };

    let mut stream = UnixStream::connect(&socket).unwrap_or_else(|error| {
        eprintln!("ped: cannot connect to {}: {error}", socket.display());
        std::process::exit(1);
    });
    serde_json::to_writer(&mut stream, &request).expect("write request");
    stream.write_all(b"\n").expect("write request terminator");
    stream.flush().expect("flush request");

    let mut response = String::new();
    BufReader::new(stream)
        .read_line(&mut response)
        .expect("read response");
    match serde_json::from_str::<Value>(&response) {
        Ok(value) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&value).expect("format response")
            );
            if value.get("ok") == Some(&Value::Bool(false)) {
                std::process::exit(1);
            }
        }
        Err(error) => {
            eprintln!("ped: invalid response: {error}");
            std::process::exit(1);
        }
    }
}

fn usage() -> ! {
    eprintln!(
        "usage: ped [--socket PATH] <status|stop|refresh|reload-config|open URL|eval SCRIPT|emit EVENT [JSON_DETAIL]>"
    );
    std::process::exit(2);
}
