//! kanna-daemon: Kanna's terminals and kmux's protocol (kanna-mux) on
//! Kanna's socket. The CLI and the app are its clients; the app keeps a
//! long-lived link (app_link.rs). Everything kanna-mux touches runs on the
//! main thread; socket, pty and signal threads post events to it.

mod app_link;
mod keeper;
mod pty;
mod reader;
mod screen;
mod terminals;

use app_link::{AppLink, Outbox, APP_COMMANDS, HELLO};
use kanna_mux::json::Value;
use kanna_mux::{Brand, Core, Instance, PaneType, SocketServer, DEFAULT_NAME};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};
use reader::Taken;
use terminals::Terminals;

const USAGE: &str = "usage: kanna-daemon [--uses-keeper]

Serves Kanna's terminals and kmux's control protocol on $KANNA_SOCKET
(default: the development socket, kanna-dev/kanna.sock in the app support
folder; never Kanna.app's own kanna/kanna.sock). Keeps its files in
$KANNA_STATE_DIR (default: that socket's folder). Gets its ptys from the
keeper of that folder, starting $KANNA_KEEPER (default: the kanna-keeper
beside this binary) if none runs.

  --uses-keeper   exits 0: this daemon gets its ptys from a keeper";

const APP_TIMEOUT: Duration = Duration::from_secs(10);

/// What the main thread acts on.
pub enum Event {
    /// A control-protocol request and where its reply goes.
    Request(Value, Box<dyn FnOnce(Value) + Send>),
    /// The core has work to do.
    Wake,
    Output { pane: String, serial: u64 },
    Drained { pane: String, serial: u64 },
    KeeperReadable,
    KeeperExits,
    KeeperLost(String),
    AppHello { conn: u64, hello: Value, out: Outbox, stream: UnixStream },
    AppCaughtUp { conn: u64 },
    AppMessage { conn: u64, message: Value },
    AppGone { conn: u64 },
    Quit(i32),
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => {}
        Some("--uses-keeper") => std::process::exit(0),
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

    let mut server = SocketServer::new(
        &socket_path,
        Arc::new({
            let requests = events.clone();
            move |request, reply| {
                let _ = requests.send(Event::Request(request, reply));
            }
        }),
    );
    server.take_over(HELLO, app_link::takeover(events.clone()));
    server.start().map_err(|e| e.message)?;

    let keeper_bin = keeper::binary();
    let keeper = match keeper::Keeper::connect(&state_dir, &keeper_bin, events.clone()) {
        Ok(keeper) => Rc::new(keeper),
        Err(error) => {
            server.stop();
            return Err(format!("can't reach the keeper ({}): {error}", keeper_bin.display()));
        }
    };
    match keeper.close_leftovers() {
        Ok(0) => {}
        Ok(n) => eprintln!("kanna-daemon: closed {n} terminal(s) an earlier daemon left in the keeper"),
        Err(error) => eprintln!("kanna-daemon: couldn't close what an earlier daemon left in the keeper: {error}"),
    }
    if let Err(error) = keeper.watch() {
        server.stop();
        return Err(format!("can't watch the keeper: {error}"));
    }
    eprintln!("kanna-daemon: using the keeper of {} (pid {})", state_dir.display(), keeper.pid());

    let env = vec![("KANNA_SOCKET".to_string(), socket_path.clone())];
    let terminals = Rc::new(Terminals::new(keeper.clone(), events.clone(), env));
    *terminals.core.borrow_mut() = Some(core.weak());
    core.set_host(Some(terminals.clone()));

    let app_timeout = std::env::var("KANNA_APP_TIMEOUT_MS").ok().and_then(|ms| ms.parse().ok()).map(Duration::from_millis).unwrap_or(APP_TIMEOUT);
    let app = Rc::new(AppLink::new(core.weak(), terminals.clone(), app_timeout));
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

    let pid_file = state_dir.join("daemon.pid");
    let _ = std::fs::write(&pid_file, format!("{}\n", std::process::id()));
    eprintln!("kanna-daemon: listening on {socket_path} (pid {})", std::process::id());

    let quit = events.clone();
    let stopping = Arc::new(AtomicI32::new(0));
    let stop = stopping.clone();
    std::thread::Builder::new()
        .name("kanna.signals".into())
        .spawn(move || loop {
            let mut signal = 0;
            // SAFETY: a valid signal set and out-pointer.
            if unsafe { libc::sigwait(&stop_signals, &mut signal) } == 0 {
                stop.store(signal, Ordering::SeqCst);
                let _ = quit.send(Event::Quit(signal));
                return;
            }
        })
        .map_err(|e| e.to_string())?;
    drop(events);

    let mut failure = None;
    loop {
        let signal = stopping.load(Ordering::SeqCst);
        if signal != 0 {
            eprintln!("kanna-daemon: stopping (signal {signal})");
            break;
        }
        app.expire(Instant::now());
        let deadline = [core.poll(), app.next_deadline()].into_iter().flatten().min();
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
            Event::Output { pane, serial } => match terminals.output(&pane, serial) {
                Some(Taken::Bytes(bytes)) => app.output(&pane, &bytes),
                Some(Taken::Overflowed) => app.resync(&pane),
                None => {}
            },
            Event::Drained { pane, serial } => {
                if let Some(code) = terminals.drained(&pane, serial) {
                    app.exited(&pane, code);
                }
            }
            Event::KeeperReadable | Event::KeeperExits => {
                let exits = keeper.take_exits();
                if matches!(event, Event::KeeperReadable) {
                    keeper.resume_watch();
                }
                match exits {
                    Ok(exits) => {
                        for exit in exits {
                            let code = exit.status.shell_status();
                            if let Some(code) = terminals.exited(&exit.id, exit.pid, code) {
                                app.exited(&exit.id, code);
                            }
                        }
                    }
                    Err(reason) => {
                        failure = Some(reason);
                        break;
                    }
                }
            }
            Event::KeeperLost(reason) => {
                failure = Some(reason);
                break;
            }
            Event::AppHello { conn, hello, out, stream } => app.hello(conn, hello, out, stream),
            Event::AppMessage { conn, message } => app.message(conn, message),
            Event::AppGone { conn } => app.gone(conn),
            Event::AppCaughtUp { conn } => app.caught_up(conn),
            Event::Quit(_) => continue,
        }
    }

    server.stop();
    app.close();
    terminals.close_all();
    let _ = std::fs::remove_file(&pid_file);
    match failure {
        Some(reason) => Err(format!("{reason}; stopping")),
        None => Ok(()),
    }
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
