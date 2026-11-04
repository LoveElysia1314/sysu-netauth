use crate::engine::State;
use std::time::{Duration, Instant};

const OUTAGE_DELAY: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Notice {
    CurrentState,
    Disconnected,
    Recovered,
}
impl Notice {
    pub(crate) fn message(self, current_status: &str) -> &str {
        match self {
            Self::CurrentState => current_status,
            Self::Disconnected => "连接已中断，请检查网络或认证设置",
            Self::Recovered => "连接已恢复，互联网连通",
        }
    }
}

struct Outage {
    since: Instant,
    warned: bool,
}

/// Keep one five-second deadline across every retry in an established connection's outage.
#[derive(Default)]
pub(crate) struct StateNotifications {
    previous: Option<State>,
    manual_connect: bool,
    outage: Option<Outage>,
}
impl StateNotifications {
    pub(crate) fn connect_requested(&mut self) {
        self.manual_connect = true;
    }
    pub(crate) fn service_connect_requested(&mut self, current: &State) {
        self.connect_requested();
        // Polling can first see an explicit request after its result is ready.
        if matches!(current, State::Authenticated | State::Failed) {
            self.previous = Some(State::Authenticating);
        }
    }
    pub(crate) fn cancel_connect_request(&mut self) {
        self.manual_connect = false;
    }

    pub(crate) fn observe(
        &mut self,
        current: &State,
        paused: bool,
        enabled: bool,
        now: Instant,
    ) -> Option<Notice> {
        let previous = self.previous.replace(current.clone());
        if paused {
            self.outage = None;
            self.manual_connect = false;
            return None;
        }
        if *current == State::Authenticated {
            let outage = self.outage.take();
            // Opening the GUI or reattaching to an online service stays silent.
            if previous.is_none() || previous == Some(State::Authenticated) {
                return None;
            }
            let manual = std::mem::take(&mut self.manual_connect);
            let notice = match outage {
                _ if manual => Some(Notice::CurrentState),
                None => Some(Notice::CurrentState),
                Some(outage) if now.duration_since(outage.since) >= OUTAGE_DELAY => {
                    Some(Notice::Recovered)
                }
                Some(_) => None,
            };
            return notice.filter(|_| enabled);
        }
        if previous == Some(State::Authenticated) && !self.manual_connect {
            self.outage = Some(Outage {
                since: now,
                warned: false,
            });
        }
        // Initial/explicit authentication failures still show their actual reason.
        // During a reconnect, the outage deadline covers all intermediate states.
        if self.outage.is_none()
            && *current == State::Failed
            && previous.is_some_and(|state| state != State::Failed)
        {
            self.manual_connect = false;
            return enabled.then_some(Notice::CurrentState);
        }
        self.poll(enabled, now)
    }

    pub(crate) fn deadline(&self, enabled: bool) -> Option<Instant> {
        self.outage
            .as_ref()
            .filter(|outage| enabled && !outage.warned)
            .map(|outage| outage.since + OUTAGE_DELAY)
    }
    pub(crate) fn poll(&mut self, enabled: bool, now: Instant) -> Option<Notice> {
        let outage = self.outage.as_mut()?;
        if !enabled || outage.warned || now.duration_since(outage.since) < OUTAGE_DELAY {
            return None;
        }
        outage.warned = true;
        Some(Notice::Disconnected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_requests_survive_a_fast_connection_with_no_intermediate_snapshot() {
        let mut notices = StateNotifications::default();
        let now = Instant::now();
        assert_eq!(
            notices.observe(&State::Authenticated, false, true, now),
            None
        );
        notices.service_connect_requested(&State::Authenticated);
        assert_eq!(
            notices.observe(&State::Authenticated, false, true, now),
            Some(Notice::CurrentState)
        );
        assert_eq!(
            notices.observe(&State::Authenticated, false, true, now),
            None
        );
    }
    struct Scenario {
        notices: StateNotifications,
        start: Instant,
    }
    impl Scenario {
        fn online() -> Self {
            let mut s = Self {
                notices: StateNotifications::default(),
                start: Instant::now(),
            };
            assert_eq!(s.state(State::Authenticated, 0), None);
            s
        }
        fn at(&self, seconds: u64) -> Instant {
            self.start + Duration::from_secs(seconds)
        }
        fn state(&mut self, state: State, seconds: u64) -> Option<Notice> {
            self.notices.observe(&state, false, true, self.at(seconds))
        }
        fn poll(&mut self, seconds: u64) -> Option<Notice> {
            self.notices.poll(true, self.at(seconds))
        }
    }
    #[test]
    fn repeated_short_reconnections_are_silent_even_hours_after_last_success() {
        let mut s = Scenario::online();
        for down in [3600, 3610, 3620, 4000] {
            assert_eq!(s.state(State::Idle, down), None);
            assert_eq!(s.state(State::Authenticating, down + 1), None);
            assert_eq!(s.state(State::Authenticated, down + 4), None);
            assert_eq!(s.notices.deadline(true), None);
            assert_eq!(s.poll(down + 5), None);
        }
    }
    #[test]
    fn retries_do_not_reset_deadline_and_a_long_outage_only_warns_once() {
        let mut s = Scenario::online();
        assert_eq!(s.state(State::Idle, 10), None);
        assert_eq!(s.state(State::Authenticating, 11), None);
        assert_eq!(s.state(State::Failed, 12), None);
        assert_eq!(s.state(State::Idle, 14), None);
        assert_eq!(s.notices.deadline(true), Some(s.at(15)));
        assert_eq!(s.poll(14), None);
        assert_eq!(s.poll(15), Some(Notice::Disconnected));
        assert_eq!(s.poll(400), None);
        assert_eq!(s.state(State::Failed, 410), None);
        assert_eq!(s.state(State::Authenticated, 420), Some(Notice::Recovered));
        assert_eq!(s.state(State::Authenticated, 421), None);
    }
    #[test]
    fn delayed_timer_still_reports_recovery_at_the_five_second_boundary() {
        let mut s = Scenario::online();
        s.state(State::Idle, 1);
        assert_eq!(s.state(State::Authenticated, 6), Some(Notice::Recovered));
        assert_eq!(s.poll(7), None);
    }
    #[test]
    fn initial_and_explicit_connections_notify_without_a_spurious_outage() {
        let mut s = Scenario {
            notices: StateNotifications::default(),
            start: Instant::now(),
        };
        assert_eq!(s.state(State::Idle, 0), None);
        assert_eq!(s.state(State::Authenticating, 1), None);
        assert_eq!(s.poll(90), None);
        assert_eq!(
            s.state(State::Authenticated, 100),
            Some(Notice::CurrentState)
        );
        s.notices.connect_requested();
        assert_eq!(s.state(State::Authenticated, 101), None); // Settings saved before reconnect.
        assert_eq!(s.state(State::Authenticating, 102), None);
        assert_eq!(s.poll(110), None);
        assert_eq!(
            s.state(State::Authenticated, 111),
            Some(Notice::CurrentState)
        );
    }
    #[test]
    fn initial_failure_notifies_once_with_its_actual_message() {
        let mut s = Scenario {
            notices: StateNotifications::default(),
            start: Instant::now(),
        };
        s.state(State::Authenticating, 0);
        assert_eq!(s.state(State::Failed, 1), Some(Notice::CurrentState));
        assert_eq!(s.state(State::Failed, 2), None);
        assert_eq!(
            Notice::CurrentState.message("example error"),
            "example error"
        );
    }
    #[test]
    fn deliberate_disconnect_and_suspend_cancel_pending_warnings() {
        let mut s = Scenario::online();
        s.state(State::Idle, 1);
        assert_eq!(s.notices.observe(&State::Idle, true, true, s.at(2)), None);
        assert_eq!(s.notices.deadline(true), None);
        assert_eq!(s.poll(100), None);
    }
    #[test]
    fn disabled_notifications_keep_correct_history() {
        let mut s = Scenario::online();
        assert_eq!(s.notices.observe(&State::Idle, false, false, s.at(1)), None);
        assert_eq!(s.notices.deadline(false), None);
        assert_eq!(s.notices.poll(false, s.at(6)), None);
        assert_eq!(s.notices.poll(true, s.at(7)), Some(Notice::Disconnected));
        assert_eq!(
            s.notices
                .observe(&State::Authenticated, false, false, s.at(8)),
            None
        );
        assert_eq!(s.state(State::Authenticated, 9), None);
        assert_eq!(s.poll(100), None);
    }
    #[test]
    fn cancelled_manual_request_does_not_leak_into_automatic_recovery() {
        let mut s = Scenario::online();
        s.notices.connect_requested();
        s.notices.cancel_connect_request();
        assert_eq!(s.state(State::Idle, 5), None);
        assert_eq!(s.state(State::Authenticated, 9), None);
    }
}
