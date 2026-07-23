//! The response-body streaming seam (feature-matrix row 16 / §5.11; design:
//! `docs/design/streaming-seam.md` §3a–3c, ruled 2026-07-21 / Q1).
//!
//! A streamed response body re-enters the store-owned core as a sequence of **typed inputs** —
//! one [`BodyChunk`] per delivery, then exactly one [`BodyEnd`] terminal — rather than an
//! adapter-owned buffer handed over at the end. Chunks are inputs, so a streamed response
//! participates in replay/determinism exactly like a completed one (§3a).
//!
//! [`BodyStream`] is the **core-owned, per-response ingest**: it verifies each chunk's `seq`
//! (ascending and gapless — order is a *checked* invariant, never a trusted one), buffers
//! undrained chunks in a **bounded ring** whose capacity is core-owned
//! ([`BodyStream::RING_CAPACITY`], never a shell literal), and closes the **completeness gate**
//! at the terminal (declared `total` bytes must equal the bytes actually ingested).
//!
//! ## Sans-io, lock-free, driver-owned mutation
//!
//! This crate has no async runtime and the contract types hold no lock. [`BodyStream`] is a plain
//! owned value the **driver** (the rung-2 shell/adapter pair, streaming-seam §3d) owns for the
//! life of one streaming response: it threads chunk delivery through `&mut self` and closes the
//! stream by *consuming* it at the terminal. There is no interior mutability and no `Mutex` — the
//! single-live-subscription-per-response discipline (§3d) makes `&mut` delivery sound, and it is
//! what makes "exactly one terminal" hold **by construction**: [`BodyStream::finish`] takes
//! `self` by value, so a second terminal (or any chunk after the terminal) cannot be written.
//!
//! Cross-FFI, an adapter pushes each chunk into driver code (M2+), which is where the `&mut`
//! ingest lives; the FFI subscription itself never reaches app code (§3d).

use std::collections::VecDeque;

use crate::error::HttpError;

/// One response-body chunk crossing the seam (design §3a). `seq` is stamped by the ingest
/// counter contract and **verified on arrival** by [`BodyStream::deliver_chunk`] (ascending,
/// gapless); `bytes` are the decoded body bytes for this chunk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BodyChunk {
    /// The chunk's sequence number. The first chunk of a response is `0`; each subsequent chunk
    /// is exactly one greater. A hole or a repeat is a typed failure, not a tolerated event.
    pub seq: u64,
    /// The decoded body bytes carried by this chunk.
    pub bytes: Vec<u8>,
}

impl BodyChunk {
    /// A chunk at `seq` carrying `bytes`.
    #[must_use]
    pub fn new(seq: u64, bytes: Vec<u8>) -> Self {
        BodyChunk { seq, bytes }
    }
}

/// The terminal input that ends a streamed body (design §3c) — a **separate** re-entry from
/// chunk delivery, not a `last` flag on the final chunk (a flag cannot carry the failure arm, and
/// makes "terminal chunk lost" indistinguishable from "still streaming").
///
/// `#[non_exhaustive]`: the terminal taxonomy may grow (e.g. a distinct truncation terminal) —
/// see the `Transport`-mapping note on [`BodyStream::finish`].
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BodyEnd {
    /// The body completed. `total` is the adapter's declared decoded byte count; the completeness
    /// gate ([`BodyStream::finish`]) fails the response unless it equals the bytes actually
    /// ingested — truncation cannot masquerade as success.
    Complete {
        /// The declared total decoded body length, in bytes.
        total: u64,
    },
    /// The body failed mid-stream, with a typed reason (the ended-vs-failed terminal the FFI
    /// mechanism lacks): a mid-body transport error arrives as data, not as a stream that just
    /// stops.
    Failed(HttpError),
}

/// The core-owned ingest of one streaming response body: the bounded ring, the `seq` verifier,
/// and the completeness gate. Owned by the driver for the response's lifetime (see the module
/// note); dropping it before a terminal is the deterministic close path.
///
/// Delivery is `&mut self`; the terminal [`BodyStream::finish`] takes `self` by value, so the
/// "chunks then exactly one terminal" shape is enforced by the type — a second terminal does not
/// compile.
#[derive(Debug)]
pub struct BodyStream {
    /// The next `seq` a delivered chunk must carry (ascending, gapless).
    next_seq: u64,
    /// Cumulative decoded bytes that have entered the core — the completeness-gate numerator.
    /// Monotonic: draining the ring never decrements it.
    ingested_bytes: u64,
    /// Undrained chunks, bounded by [`BodyStream::RING_CAPACITY`]. A consumer that never
    /// [`drains`](BodyStream::drain) fills it; the next delivery then overflows loudly.
    ring: VecDeque<BodyChunk>,
}

impl Default for BodyStream {
    fn default() -> Self {
        Self::new()
    }
}

impl BodyStream {
    /// The bounded ring's capacity — **core-owned** (design §3b). A shell or adapter reads this
    /// value from the core; it must never copy the literal. Overflowing it is the typed
    /// [`HttpError::StreamOverflow`], never silent loss.
    ///
    /// The value mirrors the F1 subscription capacity the streaming probe validated at 200/200
    /// under saturation (streaming-seam §1); it is a working default, not a tuned constant.
    pub const RING_CAPACITY: usize = 256;

    /// A fresh ingest for a new streaming response (expects `seq` 0 first, zero bytes ingested).
    #[must_use]
    pub fn new() -> Self {
        BodyStream {
            next_seq: 0,
            ingested_bytes: 0,
            ring: VecDeque::new(),
        }
    }

    /// Deliver the next body chunk (the `deliver_chunk`-shaped re-entry, design §3a).
    ///
    /// Verified on arrival:
    /// 1. **`seq` ascending and gapless** — `chunk.seq` must equal the expected next `seq`. A hole,
    ///    a repeat, or a reordering is [`HttpError::Transport`] (a broken chunk stream is a
    ///    mid-body integrity failure; see [`HttpError::Transport`]).
    /// 2. **Ring not full** — if [`RING_CAPACITY`](BodyStream::RING_CAPACITY) undrained chunks are
    ///    already buffered, the delivery is [`HttpError::StreamOverflow`] carrying the capacity and
    ///    the offending `seq`. This is the back-pressure ceiling: a conformant adapter pauses
    ///    reading before it is hit (the capability-shaped extension, M2); a broken one gets a loud,
    ///    typed failure instead of silent drop.
    ///
    /// On success the chunk enters the ring, `ingested_bytes` advances by its length, and the
    /// expected `seq` advances by one. On either failure the ingest state is left unchanged and the
    /// response is expected to fail (no further delivery).
    ///
    /// # Errors
    /// [`HttpError::Transport`] on a `seq` violation; [`HttpError::StreamOverflow`] on ring
    /// overflow.
    pub fn deliver_chunk(&mut self, chunk: BodyChunk) -> Result<(), HttpError> {
        // (1) seq is checked before capacity: a corrupt sequence is an integrity failure regardless
        // of how full the ring is, and diagnosing it first keeps the two failure modes disjoint.
        //
        // REVISIT (step-27 M1 decision): a seq violation maps to the existing `Transport` key
        // ("truncated mid-body"), not a new one — the step authorised exactly one new variant
        // (`StreamOverflow`). If M2's row 12 needs to distinguish truncation from a generic
        // transport reset to make its red case unambiguous, a dedicated key is minted THEN, with
        // that evidence — not preemptively.
        if chunk.seq != self.next_seq {
            return Err(HttpError::Transport);
        }
        // (2) capacity ceiling — loud, typed, never a silent drop.
        if self.ring.len() >= Self::RING_CAPACITY {
            return Err(HttpError::StreamOverflow {
                capacity: Self::RING_CAPACITY,
                seq: chunk.seq,
            });
        }
        self.ingested_bytes = self.ingested_bytes.saturating_add(chunk.bytes.len() as u64);
        self.next_seq += 1;
        self.ring.push_back(chunk);
        Ok(())
    }

    /// Drain every buffered chunk, in delivery order, emptying the ring (relieving back-pressure).
    /// `ingested_bytes` is unaffected — the completeness gate counts what entered the core, not
    /// what remains buffered.
    #[must_use = "the drained chunks are the delivered body; discarding them loses data"]
    pub fn drain(&mut self) -> Vec<BodyChunk> {
        self.ring.drain(..).collect()
    }

    /// Cumulative decoded bytes that have entered the core (the completeness-gate numerator).
    #[must_use]
    pub fn ingested_bytes(&self) -> u64 {
        self.ingested_bytes
    }

    /// How many chunks are currently buffered (undrained). `0`..=[`RING_CAPACITY`](BodyStream::RING_CAPACITY).
    #[must_use]
    pub fn buffered(&self) -> usize {
        self.ring.len()
    }

    /// Close the stream with its terminal (design §3c), **consuming** the ingest — so exactly one
    /// terminal is written per response, enforced by the type (this is the step-24 one-shot
    /// completion discipline extended to the stream: chunks, then one terminal).
    ///
    /// - [`BodyEnd::Complete { total }`](BodyEnd::Complete) closes the **completeness gate**: on
    ///   `total == ingested_bytes` it returns `Ok(total)` (the verified byte count); otherwise it
    ///   fails the response with [`HttpError::Transport`] — a declared length that disagrees with
    ///   what was ingested is a truncation (or over-run), and [`HttpError::Transport`] is the
    ///   documented "truncated mid-body" outcome. (A dedicated completeness key is a contract-
    ///   surface decision left to a planning session; the seq check and the gate are what *verify*,
    ///   which key they report is secondary.)
    /// - [`BodyEnd::Failed(e)`](BodyEnd::Failed) returns `Err(e)` — the mid-body failure is the
    ///   terminal outcome.
    ///
    /// # Errors
    /// The gate's [`HttpError::Transport`] on a total/ingested mismatch, or the carried error of a
    /// [`BodyEnd::Failed`].
    pub fn finish(self, end: BodyEnd) -> Result<u64, HttpError> {
        match end {
            BodyEnd::Complete { total } => {
                if total == self.ingested_bytes {
                    Ok(total)
                } else {
                    // REVISIT (step-27 M1 decision): the completeness-gate failure maps to the
                    // existing `Transport` key ("truncated mid-body"), not a new one. If M2's row 12
                    // needs truncation observably distinct from a generic transport failure to make
                    // its red case unambiguous, a dedicated key is minted THEN, with that evidence.
                    Err(HttpError::Transport)
                }
            }
            BodyEnd::Failed(err) => Err(err),
        }
    }
}

/// A streamed body's terminal fires **exactly once, enforced by the type**: [`BodyStream::finish`]
/// consumes `self`, so a second terminal cannot be written — it fails to compile with a
/// use-after-move, not at runtime (design §3c; the step-24 one-shot discipline extended).
///
/// ```compile_fail
/// use bolted_http::stream::{BodyStream, BodyEnd};
/// let stream = BodyStream::new();
/// let _first = stream.finish(BodyEnd::Complete { total: 0 });
/// // `stream` was moved into the terminal above; a second terminal does not compile.
/// let _second = stream.finish(BodyEnd::Complete { total: 0 });
/// ```
///
/// Likewise, no chunk can be delivered after the terminal:
///
/// ```compile_fail
/// use bolted_http::stream::{BodyStream, BodyChunk, BodyEnd};
/// let mut stream = BodyStream::new();
/// let _ = stream.finish(BodyEnd::Complete { total: 0 });
/// // `stream` is gone; delivery after the terminal does not compile.
/// let _ = stream.deliver_chunk(BodyChunk::new(0, vec![]));
/// ```
#[allow(dead_code)]
struct TerminalIsExactlyOnceByConstruction;

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(seq: u64, bytes: &[u8]) -> BodyChunk {
        BodyChunk::new(seq, bytes.to_vec())
    }

    #[test]
    fn happy_path_chunked_delivery() {
        let mut stream = BodyStream::new();
        stream.deliver_chunk(chunk(0, b"foo")).expect("seq 0");
        stream.deliver_chunk(chunk(1, b"bar")).expect("seq 1");
        stream.deliver_chunk(chunk(2, b"!")).expect("seq 2");
        assert_eq!(stream.ingested_bytes(), 7);
        let drained = stream.drain();
        assert_eq!(drained.len(), 3);
        assert_eq!(drained[0].bytes, b"foo");
        assert_eq!(drained[2].bytes, b"!");
        // total == ingested ⇒ Ok, returning the verified byte count.
        assert_eq!(stream.finish(BodyEnd::Complete { total: 7 }), Ok(7));
    }

    #[test]
    fn out_of_order_seq_is_rejected() {
        let mut stream = BodyStream::new();
        stream.deliver_chunk(chunk(0, b"a")).expect("seq 0");
        // Expected 1, got 2 — a hole. Typed failure, never tolerated.
        assert_eq!(
            stream.deliver_chunk(chunk(2, b"c")),
            Err(HttpError::Transport)
        );
    }

    #[test]
    fn first_chunk_must_be_seq_zero() {
        let mut stream = BodyStream::new();
        assert_eq!(
            stream.deliver_chunk(chunk(1, b"a")),
            Err(HttpError::Transport)
        );
    }

    #[test]
    fn repeated_seq_is_rejected() {
        let mut stream = BodyStream::new();
        stream.deliver_chunk(chunk(0, b"a")).expect("seq 0");
        assert_eq!(
            stream.deliver_chunk(chunk(0, b"a")),
            Err(HttpError::Transport)
        );
    }

    #[test]
    fn ring_overflow_is_a_typed_failure() {
        let mut stream = BodyStream::new();
        // Fill the ring exactly to capacity without draining.
        for seq in 0..BodyStream::RING_CAPACITY as u64 {
            stream
                .deliver_chunk(chunk(seq, b"x"))
                .expect("within capacity");
        }
        assert_eq!(stream.buffered(), BodyStream::RING_CAPACITY);
        // The next (correctly-sequenced) chunk overflows — loud, typed, carrying capacity + seq.
        let seq = BodyStream::RING_CAPACITY as u64;
        assert_eq!(
            stream.deliver_chunk(chunk(seq, b"x")),
            Err(HttpError::StreamOverflow {
                capacity: BodyStream::RING_CAPACITY,
                seq,
            })
        );
    }

    #[test]
    fn draining_relieves_back_pressure() {
        let mut stream = BodyStream::new();
        for seq in 0..BodyStream::RING_CAPACITY as u64 {
            stream
                .deliver_chunk(chunk(seq, b"x"))
                .expect("within capacity");
        }
        // Drain empties the ring; delivery can continue past RING_CAPACITY total chunks.
        let drained = stream.drain();
        assert_eq!(drained.len(), BodyStream::RING_CAPACITY);
        assert_eq!(stream.buffered(), 0);
        let seq = BodyStream::RING_CAPACITY as u64;
        stream
            .deliver_chunk(chunk(seq, b"x"))
            .expect("ring drained ⇒ no overflow");
        // The ring is a live buffer, not a total cap: ingested keeps counting every chunk.
        assert_eq!(
            stream.ingested_bytes(),
            BodyStream::RING_CAPACITY as u64 + 1
        );
    }

    #[test]
    fn completeness_gate_rejects_a_wrong_total() {
        let mut stream = BodyStream::new();
        stream.deliver_chunk(chunk(0, b"1234")).expect("seq 0");
        let _ = stream.drain();
        // Declared 5, ingested 4 ⇒ truncation, not success.
        assert_eq!(
            stream.finish(BodyEnd::Complete { total: 5 }),
            Err(HttpError::Transport)
        );
    }

    #[test]
    fn completeness_gate_accepts_the_exact_total() {
        let mut stream = BodyStream::new();
        stream.deliver_chunk(chunk(0, b"1234")).expect("seq 0");
        assert_eq!(stream.finish(BodyEnd::Complete { total: 4 }), Ok(4));
    }

    #[test]
    fn empty_body_completes_at_zero() {
        let stream = BodyStream::new();
        assert_eq!(stream.finish(BodyEnd::Complete { total: 0 }), Ok(0));
    }

    #[test]
    fn failed_terminal_propagates_the_error() {
        let mut stream = BodyStream::new();
        stream.deliver_chunk(chunk(0, b"partial")).expect("seq 0");
        assert_eq!(
            stream.finish(BodyEnd::Failed(HttpError::Timeout)),
            Err(HttpError::Timeout)
        );
    }
}
