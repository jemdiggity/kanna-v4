use crate::reader::Feed;
use crate::Event;
use kanna_detect::install;
use kanna_detect::program::{self, Program};
use kanna_detect::{Screen, Status, Store, Tracker, Verdict};
use std::collections::HashMap;
use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const TICK: Duration = Duration::from_millis(100);
const IDLE_TICK: Duration = Duration::from_secs(1);
const LOOK_EVERY: Duration = Duration::from_secs(1);

struct Watch {
    serial: u64,
    feed: Arc<Mutex<Feed>>,
    master: OwnedFd,
    program: Option<Program>,
    version: Option<String>,
    version_known: bool,
    generation: u64,
    looked: Option<Instant>,
    tracker: Tracker,
    fresh: bool,
}

struct Shared {
    watches: Mutex<HashMap<String, Watch>>,
    store: Store,
    events: Sender<Event>,
    stop: AtomicBool,
    wake: (Mutex<bool>, Condvar),
}

pub struct Detector {
    shared: Arc<Shared>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[derive(Debug)]
pub struct Explained {
    pub agent: Option<String>,
    pub executable: Option<String>,
    pub verdict: Option<Verdict>,
    pub screen: Screen,
}

impl Detector {
    pub fn start(store: Store, events: Sender<Event>) -> Detector {
        let shared = Arc::new(Shared { watches: Mutex::default(), store, events, stop: AtomicBool::new(false), wake: (Mutex::new(false), Condvar::new()) });
        let running = shared.clone();
        let thread = std::thread::Builder::new().name("kanna.status".into()).spawn(move || run(&running)).expect("start agent status");
        Detector { shared, thread: Some(thread) }
    }

    pub fn store(&self) -> &Store {
        &self.shared.store
    }

    pub fn watch(&self, pane: &str, serial: u64, feed: Arc<Mutex<Feed>>, master: OwnedFd) {
        let watch = Watch { serial, feed, master, program: None, version: None, version_known: false, generation: 0, looked: None, tracker: Tracker::default(), fresh: true };
        lock(&self.shared.watches).insert(pane.into(), watch);
        self.poke();
    }

    pub fn forget(&self, pane: &str, serial: u64) {
        let mut watches = lock(&self.shared.watches);
        if watches.get(pane).is_some_and(|w| w.serial == serial) {
            watches.remove(pane);
        }
    }

    pub fn explain(&self, pane: &str) -> Option<Explained> {
        let watches = lock(&self.shared.watches);
        let watch = watches.get(pane)?;
        let screen = lock(&watch.feed).status_screen_peek();
        let rules = self.shared.store.rules();
        let agent = watch.program.as_ref().map(|p| p.agent.clone());
        let verdict = agent.as_deref().and_then(|a| rules.evaluate(a, &screen, watch.version.as_deref()));
        Some(Explained {
            agent,
            executable: watch.program.as_ref().and_then(|p| p.executable.as_ref()).map(|p| p.display().to_string()),
            verdict,
            screen,
        })
    }

    fn poke(&self) {
        let (flag, condvar) = &self.shared.wake;
        *lock(flag) = true;
        condvar.notify_all();
    }
}

impl Drop for Detector {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.poke();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

fn run(shared: &Shared) {
    while !shared.stop.load(Ordering::SeqCst) {
        let agents = tick(shared, Instant::now());
        let (flag, condvar) = &shared.wake;
        let guard = lock(flag);
        let (mut poked, _) = condvar.wait_timeout_while(guard, if agents { TICK } else { IDLE_TICK }, |p| !*p).unwrap_or_else(|e| e.into_inner());
        *poked = false;
    }
}

fn tick(shared: &Shared, now: Instant) -> bool {
    let rules = shared.store.rules();
    let generation = shared.store.generation();
    let mut watches = lock(&shared.watches);
    let mut any = false;
    for (pane, watch) in watches.iter_mut() {
        let serial = watch.serial;
        let send = |status: Option<Status>| {
            let _ = shared.events.send(Event::Status { pane: pane.clone(), serial, status: status.map(Status::as_str) });
        };
        if watch.looked.is_none_or(|t| now.duration_since(t) >= LOOK_EVERY) {
            watch.looked = Some(now);
            let found = program::foreground_group(watch.master.as_raw_fd()).and_then(|group| program::identify(group, &rules));
            let changed = found.as_ref().map(|p| (&p.agent, &p.executable)) != watch.program.as_ref().map(|p| (&p.agent, &p.executable));
            if changed {
                let had = watch.tracker.status().is_some();
                watch.program = found;
                watch.version = None;
                watch.version_known = false;
                watch.tracker = Tracker::default();
                watch.fresh = true;
                if had && watch.program.is_none() {
                    send(None);
                }
            }
        }
        let Some(program) = watch.program.clone() else { continue };
        let Some(agent) = rules.agent(&program.agent) else { continue };
        any = true;
        if !watch.version_known {
            watch.version = program.executable.as_deref().and_then(|exe| install::version(&program.agent, exe));
            watch.version_known = true;
            watch.fresh = true;
        }
        if watch.generation != generation {
            watch.generation = generation;
            watch.fresh = true;
        }
        let mut feed = lock(&watch.feed);
        let activity = feed.activity();
        let recheck = watch.tracker.recheck().is_some_and(|t| now >= t);
        if !(activity.dirty_since.is_some() || watch.fresh || recheck) || !activity.settled(now) {
            continue;
        }
        let screen = feed.status_screen();
        drop(feed);
        watch.fresh = false;
        let verdict = agent.evaluate(&screen, watch.version.as_deref());
        if let Some(status) = watch.tracker.judge(verdict.status, now) {
            send(Some(status));
        }
    }
    any
}
