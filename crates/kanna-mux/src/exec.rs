//! A small single-threaded executor for the core's commands, with timers.
//! It runs only when the embedder calls in (`Core::poll`, or any call that
//! changes the core); see DESIGN.md. Wakers are thread-safe: waking from
//! another thread queues the task and calls the embedder's wakeup, which
//! should arrange for `Core::poll` on the core's thread.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::time::{Duration, Instant};

pub(crate) type Task = Pin<Box<dyn Future<Output = ()>>>;

/// Called from any thread when there is work for `Core::poll`.
pub type Wakeup = Arc<dyn Fn() + Send + Sync>;

/// Tasks ready to run. Shared with wakers, which may be on other threads.
#[derive(Default)]
struct Ready {
    queue: Mutex<Vec<usize>>,
    signal: Condvar,
    wakeup: Mutex<Option<Wakeup>>,
}

impl Ready {
    fn push(&self, id: usize) {
        let first = {
            let mut queue = self.queue.lock().unwrap();
            queue.push(id);
            queue.len() == 1
        };
        self.signal.notify_all();
        // One wakeup per batch: the poll it causes drains the queue.
        if first {
            let wakeup = self.wakeup.lock().unwrap().clone();
            if let Some(wakeup) = wakeup {
                wakeup();
            }
        }
    }
}

struct TaskWaker {
    id: usize,
    ready: Arc<Ready>,
}

impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) {
        self.ready.push(self.id);
    }
}

#[derive(Default)]
pub(crate) struct Executor {
    tasks: RefCell<Vec<Option<Task>>>,
    free: RefCell<Vec<usize>>,
    ready: Arc<Ready>,
    timers: RefCell<Vec<(u64, Instant, Waker)>>,
    next_timer: Cell<u64>,
    running: Cell<bool>,
}

impl Executor {
    pub(crate) fn set_wakeup(&self, wakeup: Option<Wakeup>) {
        *self.ready.wakeup.lock().unwrap() = wakeup;
    }

    /// Adds a task; it first runs on the next `run`.
    pub(crate) fn spawn(&self, task: Task) {
        let id = match self.free.borrow_mut().pop() {
            Some(id) => {
                self.tasks.borrow_mut()[id] = Some(task);
                id
            }
            None => {
                let mut tasks = self.tasks.borrow_mut();
                tasks.push(Some(task));
                tasks.len() - 1
            }
        };
        self.ready.queue.lock().unwrap().push(id);
    }

    /// Runs ready tasks and due timers until nothing is ready. A call from
    /// inside a task (re-entrant) returns at once; the outer run continues.
    pub(crate) fn run(&self) {
        if self.running.replace(true) {
            return;
        }
        struct Done<'a>(&'a Cell<bool>);
        impl Drop for Done<'_> {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }
        let _done = Done(&self.running);
        loop {
            self.fire_timers(Instant::now());
            let ids = std::mem::take(&mut *self.ready.queue.lock().unwrap());
            if ids.is_empty() {
                break;
            }
            for id in ids {
                let task = self.tasks.borrow_mut().get_mut(id).and_then(Option::take);
                let Some(mut task) = task else { continue };
                let waker = Waker::from(Arc::new(TaskWaker { id, ready: self.ready.clone() }));
                if task.as_mut().poll(&mut Context::from_waker(&waker)).is_pending() {
                    self.tasks.borrow_mut()[id] = Some(task);
                } else {
                    self.free.borrow_mut().push(id);
                }
            }
        }
    }

    fn fire_timers(&self, now: Instant) {
        let due: Vec<Waker> = {
            let mut timers = self.timers.borrow_mut();
            let (due, later): (Vec<_>, Vec<_>) = timers.drain(..).partition(|(_, at, _)| *at <= now);
            *timers = later;
            due.into_iter().map(|(_, _, waker)| waker).collect()
        };
        due.into_iter().for_each(Waker::wake);
    }

    /// When the next timer is due.
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.timers.borrow().iter().map(|(_, at, _)| *at).min()
    }

    pub(crate) fn has_ready(&self) -> bool {
        !self.ready.queue.lock().unwrap().is_empty()
    }

    /// Blocks until a task is woken or `deadline` passes.
    pub(crate) fn wait(&self, deadline: Option<Instant>) {
        let mut queue = self.ready.queue.lock().unwrap();
        while queue.is_empty() {
            match deadline {
                None => queue = self.ready.signal.wait(queue).unwrap(),
                Some(deadline) => {
                    let now = Instant::now();
                    if now >= deadline {
                        return;
                    }
                    queue = self.ready.signal.wait_timeout(queue, deadline - now).unwrap().0;
                }
            }
        }
    }

    /// Drops every task and timer.
    pub(crate) fn clear(&self) {
        let tasks = std::mem::take(&mut *self.tasks.borrow_mut());
        self.free.borrow_mut().clear();
        self.timers.borrow_mut().clear();
        drop(tasks);
    }
}

/// Finishes `duration` after it is first polled.
pub(crate) fn sleep(executor: Rc<Executor>, duration: Duration) -> impl Future<Output = ()> {
    Sleep { executor, deadline: Instant::now() + duration, timer: None }
}

struct Sleep {
    executor: Rc<Executor>,
    deadline: Instant,
    timer: Option<u64>,
}

impl Sleep {
    fn cancel(&mut self) {
        if let Some(id) = self.timer.take() {
            self.executor.timers.borrow_mut().retain(|(t, _, _)| *t != id);
        }
    }
}

impl Future for Sleep {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<()> {
        self.cancel();
        if Instant::now() >= self.deadline {
            return Poll::Ready(());
        }
        let id = self.executor.next_timer.get() + 1;
        self.executor.next_timer.set(id);
        let deadline = self.deadline;
        self.executor.timers.borrow_mut().push((id, deadline, cx.waker().clone()));
        self.timer = Some(id);
        Poll::Pending
    }
}

impl Drop for Sleep {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// A one-shot flag a task can wait on: set by the core's thread.
#[derive(Default)]
pub(crate) struct Signal {
    done: Cell<bool>,
    waker: RefCell<Option<Waker>>,
}

impl Signal {
    pub(crate) fn set(&self) {
        self.done.set(true);
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }

    pub(crate) fn wait(self: Rc<Self>) -> impl Future<Output = ()> {
        std::future::poll_fn(move |cx| {
            if self.done.get() {
                Poll::Ready(())
            } else {
                *self.waker.borrow_mut() = Some(cx.waker().clone());
                Poll::Pending
            }
        })
    }
}
