use crate::rules::Status;
use std::time::{Duration, Instant};

pub const TURN_SETTLE: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Turn {
    worked: bool,
    quiet: Option<(Instant, Status)>,
    reported: bool,
}

impl Turn {
    pub fn underway() -> Turn {
        Turn { worked: true, quiet: None, reported: false }
    }

    pub fn see(&mut self, status: Option<Status>, now: Instant) {
        match status {
            Some(Status::Working) => *self = Turn::underway(),
            Some(status) if self.worked => self.quiet = Some((self.quiet.map_or(now, |(since, _)| since), status)),
            _ => self.quiet = None,
        }
        self.reported = false;
    }

    pub fn reported_end(&mut self, status: Option<Status>, now: Instant) {
        self.see(status, now);
        self.reported = self.quiet.is_some();
    }

    pub fn ended(&self, now: Instant, settle: Duration) -> Option<Status> {
        self.quiet.filter(|(since, _)| self.reported || now.duration_since(*since) >= settle).map(|(_, status)| status)
    }

    pub fn due(&self, settle: Duration) -> Option<Instant> {
        self.quiet.map(|(since, _)| if self.reported { since } else { since + settle })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_ends_once_its_agent_stays_idle_or_needs_you_after_working() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let settle = Duration::from_millis(1000);
        let mut turn = Turn::default();
        turn.see(Some(Status::Idle), at(0));
        assert_eq!(turn.ended(at(5000), settle), None, "idle before any work is the agent starting");
        turn.see(Some(Status::Working), at(100));
        turn.see(Some(Status::Idle), at(2000));
        assert_eq!(turn.ended(at(2900), settle), None);
        turn.see(Some(Status::Working), at(2900));
        assert_eq!(turn.ended(at(4000), settle), None, "a short idle between tool calls");
        turn.see(Some(Status::Idle), at(5000));
        turn.see(Some(Status::NeedsYou), at(5500));
        assert_eq!(turn.due(settle), Some(at(6000)));
        assert_eq!(turn.ended(at(6000), settle), Some(Status::NeedsYou), "the latest status says why");
        turn.see(None, at(6100));
        assert_eq!(turn.ended(at(9000), settle), None, "no agent in the pane");
        let mut known = Turn::underway();
        known.see(Some(Status::Idle), at(0));
        assert_eq!(known.ended(at(1000), settle), Some(Status::Idle), "an agent running when Kanna started has worked");
    }

    #[test]
    fn a_reported_end_ends_the_turn_at_once() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let settle = Duration::from_millis(1000);
        let mut turn = Turn::default();
        turn.reported_end(Some(Status::Idle), at(0));
        assert_eq!(turn.ended(at(0), settle), None, "no work yet, so no turn to end");
        turn.see(Some(Status::Working), at(100));
        turn.reported_end(Some(Status::Idle), at(200));
        assert_eq!(turn.due(settle), Some(at(200)));
        assert_eq!(turn.ended(at(200), settle), Some(Status::Idle));
        turn.see(Some(Status::Working), at(300));
        assert_eq!(turn.ended(at(5000), settle), None, "a new turn starts over");
    }
}
