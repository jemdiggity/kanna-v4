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

#[test]
fn a_stopped_server_starts_again() {
    let dir = folder("again");
    let path = dir.join("s.sock").to_string_lossy().into_owned();
    let server = SocketServer::new(&path, echo());
    server.start().unwrap();
    server.stop();
    assert!(!std::path::Path::new(&path).exists());
    assert!(!SocketServer::is_listening(&path));
    server.start().unwrap();
    assert!(ask(&path, "{\"id\":2,\"cmd\":\"x\"}\n").contains("\"id\":2"));
    assert!(ask(&path, "{\"id\":3,\"cmd\":\"x\"}\n").contains("\"id\":3"));
    drop(server);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn stopping_leaves_a_server_that_took_the_path_over_alone() {
    let dir = folder("took");
    let path = dir.join("s.sock").to_string_lossy().into_owned();
    let old = SocketServer::new(&path, echo());
    old.start().unwrap();
    std::fs::remove_file(&path).unwrap();
    let new = SocketServer::new(&path, echo());
    new.start().unwrap();
    old.stop();
    assert!(ask(&path, "{\"id\":4,\"cmd\":\"x\"}\n").contains("\"id\":4"));
    drop(new);
    assert!(!std::path::Path::new(&path).exists());
    let _ = std::fs::remove_dir_all(&dir);
}
