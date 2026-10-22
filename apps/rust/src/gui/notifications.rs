use crate::engine::State;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct StateNotifications {
    authenticated: Option<Instant>,
    failed: Option<Instant>,
}

impl StateNotifications {
    pub(super) fn should_notify(
        &mut self,
        previous: Option<&State>,
        current: &State,
        enabled: bool,
        now: Instant,
    ) -> bool {
        if !enabled || previous.is_none_or(|previous| previous == current) {
            return false;
        }
        let (last, cooldown) = match current {
            State::Authenticated => (&mut self.authenticated, Duration::from_secs(180)),
            State::Failed => (&mut self.failed, Duration::from_secs(300)),
            _ => return false,
        };
        if last.is_some_and(|last| now.duration_since(last) < cooldown) {
            return false;
        }
        *last = Some(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alternating_states_keep_independent_cooldowns() {
        let mut notices = StateNotifications::default();
        let start = Instant::now();
        let mut transition = |previous, current, seconds| {
            notices.should_notify(
                Some(&previous),
                &current,
                true,
                start + Duration::from_secs(seconds),
            )
        };
        assert!(transition(State::Idle, State::Authenticated, 0));
        assert!(transition(State::Authenticated, State::Failed, 1));
        assert!(!transition(State::Failed, State::Authenticated, 2));
        assert!(!transition(State::Authenticated, State::Failed, 3));
        // Suppressed transitions must not postpone the next eligible notice.
        assert!(transition(State::Failed, State::Authenticated, 180));
        assert!(!transition(State::Authenticated, State::Failed, 300));
        assert!(transition(State::Idle, State::Failed, 301));
    }

    #[test]
    fn disabled_or_unchanged_snapshots_do_not_consume_cooldowns() {
        let mut notices = StateNotifications::default();
        let now = Instant::now();
        assert!(!notices.should_notify(None, &State::Authenticated, true, now));
        assert!(!notices.should_notify(
            Some(&State::Authenticated),
            &State::Authenticated,
            true,
            now,
        ));
        assert!(!notices.should_notify(Some(&State::Idle), &State::Failed, false, now));
        assert!(!notices.should_notify(Some(&State::Idle), &State::Authenticating, true, now));
        assert!(notices.should_notify(Some(&State::Idle), &State::Authenticated, true, now));
        assert!(notices.should_notify(Some(&State::Idle), &State::Failed, true, now));
    }
}
