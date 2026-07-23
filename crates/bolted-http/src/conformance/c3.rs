//! C3 — the divergence matrix, **generated from the capability types** (feature-matrix §4, rung 3).
//!
//! The columns are the enumerated optional-capability set ([`Capability::ALL`]); each adapter's row
//! is read off its [`AdapterFactory`] capability self-report, whose `Some(..)` only type-checks if
//! the concrete adapter implements the trait. So the table can never diverge from the real impls —
//! hand-written prose matrices are the prior-art failure mode. The committed `EXPECTED_*` strings
//! are the drift check: change a capability impl and the exact-match test fails.

use super::AdapterFactory;
use crate::capability::MetricsTier;

/// An optional capability the divergence matrix reports on. Adding a variant here (and to
/// [`Capability::ALL`]) is how a new optional-capability trait joins the generated table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    /// Row 18 (CAP, tiered): reports request metrics.
    Metrics,
}

impl Capability {
    /// Every capability the matrix covers, in a stable order.
    pub const ALL: &'static [Capability] = &[Capability::Metrics];

    /// The stable slug used in the rendered table.
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Capability::Metrics => "metrics",
        }
    }
}

/// Whether an adapter has a capability, with optional detail (the metrics tier).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Presence {
    /// The adapter does not implement the capability trait.
    Absent,
    /// Implemented, no further detail.
    Present,
    /// Implemented, with a detail string (e.g. the metrics tier).
    PresentDetail(String),
}

impl Presence {
    fn render(&self) -> String {
        match self {
            Presence::Absent => "absent".to_string(),
            Presence::Present => "present".to_string(),
            Presence::PresentDetail(d) => format!("present ({d})"),
        }
    }
}

/// One row of the divergence matrix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DivergenceRow {
    /// The capability this row reports on.
    pub capability: Capability,
    /// Its presence on the adapter.
    pub presence: Presence,
}

/// The divergence matrix for one adapter — data first, rendered on demand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DivergenceTable(Vec<DivergenceRow>);

impl DivergenceTable {
    /// The rows, in [`Capability::ALL`] order.
    #[must_use]
    pub fn rows(&self) -> &[DivergenceRow] {
        &self.0
    }

    /// A fixed-format, human-readable table (the committed-expectation shape).
    #[must_use]
    pub fn render(&self) -> String {
        const W: usize = 14;
        let mut lines = Vec::with_capacity(self.0.len() + 2);
        lines.push(format!("{:<W$} | {}", "capability", "presence"));
        lines.push(format!("{}-+-{}", "-".repeat(W), "-".repeat(22)));
        for r in &self.0 {
            lines.push(format!(
                "{:<W$} | {}",
                r.capability.slug(),
                r.presence.render()
            ));
        }
        lines.join("\n")
    }
}

/// Generate the divergence matrix for `factory`, one row per [`Capability::ALL`] entry. The
/// present/absent decision comes from the factory's capability self-report — the type-checked seam.
#[must_use]
pub fn divergence(factory: &dyn AdapterFactory) -> DivergenceTable {
    let rows = Capability::ALL
        .iter()
        .map(|&capability| {
            let presence = match capability {
                Capability::Metrics => match factory.metrics() {
                    Some(m) => Presence::PresentDetail(tier_slug(m.tier()).to_string()),
                    None => Presence::Absent,
                },
            };
            DivergenceRow {
                capability,
                presence,
            }
        })
        .collect();
    DivergenceTable(rows)
}

const fn tier_slug(tier: MetricsTier) -> &'static str {
    match tier {
        MetricsTier::Phase => "Phase",
        MetricsTier::WholeRequest => "WholeRequest",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conformance::mock::MockFactory;
    use crate::conformance::netmock::SocketMockFactory;

    /// The socket mock: no priority honouring, whole-request metrics (the honest reqwest-like tier).
    const EXPECTED_SOCKET_MOCK: &str = "\
capability     | presence
---------------+-----------------------
metrics        | present (WholeRequest)";

    /// A scripted mock reports no optional capabilities — the all-absent baseline.
    const EXPECTED_SCRIPTED_MOCK: &str = "\
capability     | presence
---------------+-----------------------
metrics        | absent";

    #[test]
    fn table_covers_every_capability() {
        let table = divergence(&SocketMockFactory::correct([0u8; 32]));
        assert_eq!(table.rows().len(), Capability::ALL.len());
    }

    #[test]
    fn socket_mock_divergence_is_pinned() {
        let table = divergence(&SocketMockFactory::correct([0u8; 32]));
        assert_eq!(table.render(), EXPECTED_SOCKET_MOCK);
    }

    #[test]
    fn scripted_mock_divergence_is_pinned() {
        let table = divergence(&MockFactory::correct());
        assert_eq!(table.render(), EXPECTED_SCRIPTED_MOCK);
    }

    /// The three M1.5 additions (response-sink selector, upload-progress sink, `content_length`) are
    /// **CORE** request/response/callback surfaces, not optional capability traits — so the
    /// divergence columns are unchanged. `UploadProgressSink` is a per-request callback passed to
    /// `Http::send`, not an `AdapterFactory` capability; it must never grow a `Capability` column.
    #[test]
    fn m1_5_additions_are_core_not_capabilities() {
        assert_eq!(Capability::ALL.len(), 1);
        assert_eq!(Capability::ALL, &[Capability::Metrics]);
    }
}
