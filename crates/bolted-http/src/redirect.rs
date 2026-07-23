//! The redirect ceiling (feature-matrix §5.5, rows 6–7; ruled 2026-07-21 / Q2).
//!
//! The redirect-follow ceiling is **CFG** — a core-owned value set at the composition root — and
//! exhaustion is **core-counted**: the adapter's native redirect limit is set *above* the ceiling,
//! and the core counts the hops it recorded in the trace ([`crate::HttpResponse::hops`]) and emits
//! [`HttpError::TooManyRedirects`] itself. This removes the one classifier text-match a native
//! limit forced (OkHttp's `ProtocolException` message) and closes the honest-limit gap — no
//! platform documents its native ceiling as contract.
//!
//! The counting lives here, in the sans-io core, as a pure function over the hop count: it never
//! sees a socket, and the same value governs every adapter.

use crate::error::HttpError;
use crate::request::Url;

/// The composition-root redirect-follow ceiling: the maximum number of redirect hops a request may
/// follow before the core fails it with [`HttpError::TooManyRedirects`].
///
/// A `Copy` value the composition root constructs and the core consults; it is never seen on the
/// wire and never set from an adapter literal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RedirectCeiling(u32);

impl RedirectCeiling {
    /// The default ceiling (10 hops) — the working default the shipped adapters used before core
    /// counting (their `redirect_limit`).
    pub const DEFAULT: RedirectCeiling = RedirectCeiling(10);

    /// A ceiling of `max_hops` redirect hops.
    #[must_use]
    pub const fn new(max_hops: u32) -> Self {
        RedirectCeiling(max_hops)
    }

    /// The maximum number of hops this ceiling permits.
    #[must_use]
    pub const fn max_hops(self) -> u32 {
        self.0
    }

    /// Enforce the ceiling against a recorded redirect trace (design §5.5 / Q2): the core counts
    /// the hops and emits [`HttpError::TooManyRedirects`] when the count **exceeds** the ceiling.
    ///
    /// The count comes from the trace itself (`hops.len()`), so the check is identical on every
    /// adapter and needs no native-limit interpretation.
    ///
    /// # Errors
    /// [`HttpError::TooManyRedirects`] carrying the ceiling, when `hops.len() > max_hops`.
    pub fn enforce(self, hops: &[Url]) -> Result<(), HttpError> {
        self.enforce_count(hops.len())
    }

    /// [`enforce`](RedirectCeiling::enforce) over a raw hop count — the seam a manual-follow loop
    /// (which knows its count before it has pushed the next `Url`) uses.
    ///
    /// # Errors
    /// [`HttpError::TooManyRedirects`] carrying the ceiling, when `hop_count > max_hops`.
    pub fn enforce_count(self, hop_count: usize) -> Result<(), HttpError> {
        if hop_count as u64 > u64::from(self.0) {
            Err(HttpError::TooManyRedirects { limit: self.0 })
        } else {
            Ok(())
        }
    }
}

impl Default for RedirectCeiling {
    fn default() -> Self {
        RedirectCeiling::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hop(n: usize) -> Vec<Url> {
        (0..n)
            .map(|i| Url::https(&format!("https://hop{i}.test/")).expect("valid"))
            .collect()
    }

    #[test]
    fn default_ceiling_is_ten() {
        assert_eq!(RedirectCeiling::default().max_hops(), 10);
        assert_eq!(RedirectCeiling::DEFAULT.max_hops(), 10);
    }

    #[test]
    fn within_the_ceiling_is_ok() {
        let ceiling = RedirectCeiling::new(3);
        assert_eq!(ceiling.enforce(&hop(0)), Ok(()));
        assert_eq!(ceiling.enforce(&hop(2)), Ok(()));
        // Exactly at the ceiling is still permitted (the ceiling is the max, not a strict bound).
        assert_eq!(ceiling.enforce(&hop(3)), Ok(()));
    }

    #[test]
    fn exceeding_the_ceiling_by_trace_count_is_typed() {
        let ceiling = RedirectCeiling::new(3);
        assert_eq!(
            ceiling.enforce(&hop(4)),
            Err(HttpError::TooManyRedirects { limit: 3 })
        );
    }

    #[test]
    fn enforce_count_matches_enforce() {
        let ceiling = RedirectCeiling::new(5);
        assert_eq!(ceiling.enforce_count(5), Ok(()));
        assert_eq!(
            ceiling.enforce_count(6),
            Err(HttpError::TooManyRedirects { limit: 5 })
        );
    }
}
