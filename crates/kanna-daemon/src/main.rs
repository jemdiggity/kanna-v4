//! kanna-daemon: Kanna's terminals and kmux's protocol (kanna-mux) on
//! Kanna's socket. The CLI and the app are its clients; the app keeps a
//! long-lived link (app_link.rs). Everything kanna-mux touches runs on the
//! main thread; socket, pty and signal threads post events to it.
//!
//! For now (plan step 0.4) it spawns each terminal on a pty itself; the
//! keeper takes that over in step 0.5.

mod app_link;
mod pty;
mod screen;
mod terminals;

use app_link::{AppLink, APP_COMMANDS, HELLO};
use kanna_mux::json::Value;
use kanna_mux::{Brand, Core, Instance, PaneType, SocketServer, DEFAULT_NAME};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::Instant;
use terminals::Terminals;

const USAGE: &str = "usage: kanna-daemon [--uses-keeper]

Serves Kanna's terminals and kmux's control protocol on $KANNA_SOCKET
(default: the development socket, kanna-dev/kanna.sock in the app support
folder; never Kanna.app's own kanna/kanna.sock). Keeps its files in
$KANNA_STATE_DIR (default: that socket's folder).

  --uses-keeper   exits 0 if this daemon gets its ptys from a keeper
                  (KANNA_KEEPER), 1 if it spawns them itself (for now)";

/// What the main thread acts on.
pub enum Event {
    /// A control-protocol request and where its reply goes.
    Request(Value, Box<dyn FnOnce(Value) + Send>),
    /// The core has work to do.
    Wake,
    Output { pane: String, serial: u64, bytes: Vec<u8> },
    Exited { pane: String, serial: u64, code: i32 },
    AppHello { conn: u64, hello: Value, out: Sender<Vec<u8>>, stream: UnixStream },
    AppMessage { conn: u64, message: Value },
    AppGone { conn: u64 },
    Quit(i32),
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => {}
        Some("--uses-keeper") => std::process::exit(1),
        Some("-h" | "--help") => return println!("{USAGE}"),
        Some(other) => {
            eprintln!("kanna-daemon: unknown argument {other}\n{USAGE}");
            std::process::exit(2);
        }
    }
    if let Err(message) = run() {
        eprintln!("kanna-daemon: {message}");
        std::process::exit(1);
    }
}

/// Where Kanna's files live: `~/Library/Application Support` on macOS,
/// `$XDG_DATA_HOME` (`~/.local/share`) elsewhere.
fn support_folder() -> PathBuf {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    if cfg!(target_os = "macos") {
        return home.join("Library/Application Support");
    }
    std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()).map(PathBuf::from).unwrap_or_else(|| home.join(".local/share"))
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// The socket of the Kanna.app you use (kmux-branded until the end of M1),
/// which this daemon must never take or touch.
fn app_socket() -> PathBuf {
    support_folder().join("kanna/kanna.sock")
}

fn run() -> Result<(), String> {
    let _ = Brand::set_current(Brand::new("kanna", "Kanna"));
    let socket = env_path("KANNA_SOCKET").unwrap_or_else(|| support_folder().join("kanna-dev/kanna.sock"));
    // The support folder may be case-insensitive (macOS): compare as such.
    if socket.to_string_lossy().to_lowercase() == app_socket().to_string_lossy().to_lowercase() {
        return Err(format!("{} belongs to Kanna.app; use another KANNA_SOCKET", socket.display()));
    }
    let state_dir = env_path("KANNA_STATE_DIR").unwrap_or_else(|| socket.parent().map(Path::to_path_buf).unwrap_or_default());
    std::fs::create_dir_all(&state_dir).map_err(|e| format!("can't make {}: {e}", state_dir.display()))?;
    let socket_path = socket.to_string_lossy().into_owned();

    // The signals that stop the daemon are waited for on a thread of their
    // own, so every thread (and so every child) starts with them blocked.
    let stop_signals = signal_set(&[libc::SIGTERM, libc::SIGINT, libc::SIGHUP]);
    // SAFETY: a valid signal set.
    unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &stop_signals, std::ptr::null_mut()) };

    let (events, inbox) = mpsc::channel::<Event>();
    let core = Core::default();
    core.set_instance(Instance::with_socket(DEFAULT_NAME, Some(socket_path.clone())));
    // Web, markdown and iOS panes come with the app (M3).
    core.set_pane_types(vec![PaneType::Term]);
    let wake = events.clone();
    core.set_wakeup(Some(Arc::new(move || {
        let _ = wake.send(Event::Wake);
    })));

    let env = vec![("KANNA_SOCKET".to_string(), socket_path.clone())];
    let terminals = Rc::new(Terminals::new(Box::new(pty::LocalSpawner), events.clone(), env));
    *terminals.core.borrow_mut() = Some(core.weak());
    core.set_host(Some(terminals.clone()));

    let app = Rc::new(AppLink::new(core.weak(), terminals.clone()));
    let on_change = Rc::downgrade(&app);
    core.set_on_change(Some(Rc::new(move || {
        if let Some(app) = on_change.upgrade() {
            app.layout_changed();
        }
    })));
    let on_start = Rc::downgrade(&app);
    *terminals.on_start.borrow_mut() = Some(Box::new(move |pane| {
        if let Some(app) = on_start.upgrade() {
            app.started(pane);
        }
    }));
    for cmd in APP_COMMANDS {
        let link = Rc::downgrade(&app);
        core.add_command(
            cmd,
            Rc::new(move |args, responder| {
                if let Some(link) = link.upgrade() {
                    link.forward(cmd, args, responder);
                }
            }),
        );
    }

    let requests = events.clone();
    let mut server = SocketServer::new(
        &socket_path,
        Arc::new(move |request, reply| {
            let _ = requests.send(Event::Request(request, reply));
        }),
    );
    server.take_over(HELLO, app_link::takeover(events.clone()));
    server.start().map_err(|e| e.message)?;
    let pid_file = state_dir.join("daemon.pid");
    let _ = std::fs::write(&pid_file, format!("{}\n", std::process::id()));
    eprintln!("kanna-daemon: listening on {socket_path} (pid {})", std::process::id());

    let quit = events.clone();
    std::thread::Builder::new()
        .name("kanna.signals".into())
        .spawn(move || loop {
            let mut signal = 0;
            // SAFETY: a valid signal set and out-pointer.
            if unsafe { libc::sigwait(&stop_signals, &mut signal) } == 0 {
                let _ = quit.send(Event::Quit(signal));
                return;
            }
        })
        .map_err(|e| e.to_string())?;
    drop(events);

    loop {
        let deadline = core.poll();
        let event = match deadline {
            Some(at) => inbox.recv_timeout(at.saturating_duration_since(Instant::now())),
            None => inbox.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        let event = match event {
            Ok(event) => event,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        match event {
            Event::Request(request, reply) => core.handle(request, reply),
            Event::Wake => {}
            Event::Output { pane, serial, bytes } => {
                if terminals.output(&pane, serial, &bytes) {
                    app.output(&pane, &bytes);
                }
            }
            Event::Exited { pane, serial, code } => {
                if terminals.exited(&pane, serial, code) {
                    app.exited(&pane, code);
                }
            }
            Event::AppHello { conn, hello, out, stream } => app.hello(conn, hello, out, stream),
            Event::AppMessage { conn, message } => app.message(conn, message),
            Event::AppGone { conn } => app.gone(conn),
            Event::Quit(signal) => {
                eprintln!("kanna-daemon: stopping (signal {signal})");
                break;
            }
        }
    }

    server.stop();
    app.close();
    terminals.hang_up_all();
    let _ = std::fs::remove_file(&pid_file);
    Ok(())
}

fn signal_set(signals: &[libc::c_int]) -> libc::sigset_t {
    // SAFETY: sigset_t is plain data, initialised by sigemptyset.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for signal in signals {
            libc::sigaddset(&mut set, *signal);
        }
        set
    }
}
