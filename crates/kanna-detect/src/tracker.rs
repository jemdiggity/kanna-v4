use crate::rules::Status;
use std::time::{Duration, Instant};

pub const SETTLE: Duration = Duration::from_millis(60);
pub const STALE: Duration = Duration::from_secs(1);
pub const HOLD_LIMIT: Duration = Duration::from_secs(1);
pub const CONFIRM_IDLE: Duration = Duration::from_millis(300);

#[derive(Clone, Copy, Debug, Default)]
pub struct Activity {
    pub changed: Option<Instant>,
    pub dirty_since: Option<Instant>,
    pub held_since: Option<Instant>,
}

impl Activity {
    pub fn settled(&self, now: Instant) -> bool {
        if self.held_since.is_some_and(|t| now.duration_since(t) < HOLD_LIMIT) {
            return false;
        }
        let quiet = self.changed.is_none_or(|t| now.duration_since(t) >= SETTLE);
        let stale = self.dirty_since.is_some_and(|t| now.duration_since(t) >= STALE);
        quiet || stale
    }

    pub fn wake(&self, now: Instant) -> Option<Instant> {
        let held = self.held_since.filter(|t| now.duration_since(*t) < HOLD_LIMIT).map(|t| t + HOLD_LIMIT);
        let quiet = self.changed.map(|t| t + SETTLE);
        held.into_iter().chain(quiet).max().filter(|t| *t > now)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Tracker {
    status: Option<Status>,
    idle_since: Option<Instant>,
}

impl Tracker {
    pub fn status(&self) -> Option<Status> {
        self.status
    }

    pub fn judge(&mut self, verdict: Status, now: Instant) -> Option<Status> {
        if self.status == Some(Status::Working) && verdict == Status::Idle {
            let since = *self.idle_since.get_or_insert(now);
            if now.duration_since(since) < CONFIRM_IDLE {
                return None;
            }
        }
        self.idle_since = None;
        (self.status != Some(verdict)).then(|| {
            self.status = Some(verdict);
            verdict
        })
    }

    pub fn recheck(&self) -> Option<Instant> {
        self.idle_since.map(|t| t + CONFIRM_IDLE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_after_work_must_hold_before_it_shows() {
        let t0 = Instant::now();
        let mut tracker = Tracker::default();
        assert_eq!(tracker.judge(Status::Working, t0), Some(Status::Working));
        assert_eq!(tracker.judge(Status::Idle, t0 + Duration::from_millis(10)), None);
        assert_eq!(tracker.judge(Status::Working, t0 + Duration::from_millis(100)), None);
        assert_eq!(tracker.judge(Status::Idle, t0 + Duration::from_millis(200)), None);
        assert_eq!(tracker.recheck(), Some(t0 + Duration::from_millis(500)));
        assert_eq!(tracker.judge(Status::Idle, t0 + Duration::from_millis(500)), Some(Status::Idle));
        assert_eq!(tracker.judge(Status::NeedsYou, t0 + Duration::from_millis(510)), Some(Status::NeedsYou));
        assert_eq!(tracker.judge(Status::Idle, t0 + Duration::from_millis(520)), Some(Status::Idle));
    }

    #[test]
    fn a_screen_settles_when_quiet_and_not_held() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let busy = Activity { changed: Some(t0), dirty_since: Some(t0), held_since: None };
        assert!(!busy.settled(ms(10)));
        assert!(busy.settled(ms(60)));
        let streaming = Activity { changed: Some(ms(1190)), dirty_since: Some(t0), held_since: None };
        assert!(streaming.settled(ms(1200)));
        let held = Activity { changed: Some(t0), dirty_since: Some(t0), held_since: Some(t0) };
        assert!(!held.settled(ms(500)));
        assert!(held.settled(ms(1000)));
        assert_eq!(held.wake(ms(500)), Some(ms(1000)));
    }
}
