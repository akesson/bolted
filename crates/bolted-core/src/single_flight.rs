//! Deterministic single-flight modelling for async validation checks — with NO async runtime.
//!
//! Effects are data (sans-io): [`SingleFlight::begin`] yields a [`CheckToken`]; the driver (the
//! platform layer, or a test) later calls [`SingleFlight::complete`]. A newer `begin` supersedes
//! any in-flight check, and a completion carrying a superseded token is discarded. The core owns
//! ordering correctness; shells only choose *when* to trigger.

/// In-flight/settled state of the single check.
#[derive(Debug, Clone, PartialEq)]
pub enum CheckState<T> {
    Idle,
    Pending { seq: u64 },
    Done { verdict: T },
}

/// An opaque receipt for an in-flight check. Its sequence is private, so it can only be obtained
/// from [`SingleFlight::begin`] and cannot be forged; it is consumed by `complete`.
#[derive(Debug)]
pub struct CheckToken(u64);

/// Single-flight coordinator: at most one check in flight; the latest `begin` always wins.
#[derive(Debug)]
pub struct SingleFlight<T> {
    seq: u64,
    state: CheckState<T>,
}

impl<T> SingleFlight<T> {
    pub fn new() -> Self {
        SingleFlight {
            seq: 0,
            state: CheckState::Idle,
        }
    }

    /// Begin a check, superseding any in-flight one (whose token becomes stale).
    pub fn begin(&mut self) -> CheckToken {
        self.seq += 1;
        self.state = CheckState::Pending { seq: self.seq };
        CheckToken(self.seq)
    }

    /// Complete the check that `token` began. Returns `false` (and ignores `verdict`) if `token`
    /// was superseded or the check was already settled — the latest `begin` wins (conformance C10).
    pub fn complete(&mut self, token: CheckToken, verdict: T) -> bool {
        match self.state {
            CheckState::Pending { seq } if seq == token.0 => {
                self.state = CheckState::Done { verdict };
                true
            }
            _ => false,
        }
    }

    /// Cancel any in-flight or settled check, returning to [`CheckState::Idle`] and bumping `seq`
    /// (symmetric with [`begin`](Self::begin)) so any still-outstanding [`CheckToken`] is stale by
    /// sequence as well as by state — a completion arriving after a `reset` is discarded exactly
    /// as a superseded one is (conformance C10). This is the value-bound verdict reset: any change
    /// to the checked field's value invalidates the check (ARCHITECTURE §2/§8, conformance C13).
    pub fn reset(&mut self) {
        self.seq += 1;
        self.state = CheckState::Idle;
    }

    pub fn state(&self) -> &CheckState<T> {
        &self.state
    }
}

impl<T> Default for SingleFlight<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_begin_wins_and_stale_is_ignored() {
        let mut sf: SingleFlight<i32> = SingleFlight::new();
        let a = sf.begin();
        let b = sf.begin(); // supersedes a
        assert!(!sf.complete(a, 1)); // stale token -> ignored
        assert!(sf.complete(b, 2)); // latest wins
        assert_eq!(sf.state(), &CheckState::Done { verdict: 2 });

        // A fresh begin re-arms; completing the settled/old token is ignored.
        let c = sf.begin();
        assert!(matches!(sf.state(), CheckState::Pending { .. }));
        assert!(sf.complete(c, 3));
        assert_eq!(sf.state(), &CheckState::Done { verdict: 3 });
    }

    #[test]
    fn reset_returns_to_idle_and_supersedes_inflight() {
        let mut sf: SingleFlight<i32> = SingleFlight::new();
        let t = sf.begin();
        assert!(matches!(sf.state(), CheckState::Pending { .. }));
        sf.reset();
        assert_eq!(sf.state(), &CheckState::Idle);
        // a completion of the pre-reset token is stale -> ignored, and state stays Idle.
        assert!(!sf.complete(t, 1));
        assert_eq!(sf.state(), &CheckState::Idle);
    }
}
