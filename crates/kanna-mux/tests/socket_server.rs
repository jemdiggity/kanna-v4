use kanna_mux::json::{json, Value};
use kanna_mux::{Handler, SocketServer};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Arc;

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("km-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn echo() -> Handler {
    Arc::new(|request: Value, reply: Box<dyn FnOnce(Value) + Send>| reply(json!({ "id": request["id"].clone(), "ok": true })))
}

fn ask(path: &str, line: &str) -> String {
    let mut stream = UnixStream::connect(path).unwrap();
    stream.write_all(line.as_bytes()).unwrap();
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply).unwrap();
    reply
}

#[test]
fn a_server_that_failed_to_start_leaves_the_running_one_alone() {
    let dir = folder("second");
    let path = dir.join("s.sock").to_string_lossy().into_owned();
    let first = SocketServer::new(&path, echo());
    first.start().unwrap();
    let second = SocketServer::new(&path, echo());
    assert!(second.start().is_err());
    second.stop();
    drop(second);
    assert!(std::path::Path::new(&path).exists());
    assert!(ask(&path, "{\"id\":1,\"cmd\":\"x\"}\n").contains("\"ok\":true"));
    drop(first);
    assert!(!std::path::Path::new(&path).exists());
    let _ = std::fs::remove_dir_all(&dir);
}
