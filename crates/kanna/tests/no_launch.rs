use std::process::Command;
use std::time::{Duration, Instant};

#[test]
fn with_kanna_socket_set_an_unreachable_daemon_fails_with_3_and_starts_nothing() {
    let dir = std::env::temp_dir().join(format!("kanna-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for mux in [None, Some("kanna")] {
        let started = Instant::now();
        let mut command = Command::new(env!("CARGO_BIN_EXE_kanna"));
        if let Some(mux) = mux {
            command.args(["--mux", mux]);
        }
        let output = command
            .arg("list")
            .env("KANNA_SOCKET", dir.join("nothing.sock"))
            .env("KANNA_APP", "/nonexistent/Kanna.app")
            .env_remove("KANNA_MUX")
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(3), "{mux:?}: {stderr}");
        assert!(stderr.contains("doesn't start an app"), "{mux:?}: {stderr}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }
    let _ = std::fs::remove_dir_all(&dir);
}
