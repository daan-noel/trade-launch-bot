// ============================================================
// The open `Venue` seam.
//
// A `Venue` is a launchpad/AMM that the engine can execute against. The engine
// (send / nonce / tip / confirm / sim) is venue-agnostic; a venue crate (e.g.
// `executor-pumpfun`) wraps an `Engine` and implements `Venue` to expose which
// launchpad it drives.
//
// Dispatch is **static** — `VenueId` is a plain enum matched at the call site,
// never a `Box<dyn Venue>`. Adding a launchpad you TRADE is a new leaf crate
// depending on `executor-core` only, plus one `VenueId` arm; it never touches
// this trait's existing implementors. The catalog / builder specifics live in
// the venue crate, keyed by `VenueId`.
// ============================================================

use crate::engine::Engine;

/// The launchpad/AMM a venue drives. A closed enum on purpose: it is the single
/// axis every `Operation`/`VariantSpec` keys off, so an unrepresentable venue is
/// a compile error, not a runtime string mismatch. Extend by adding an arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VenueId {
    /// pump.fun bonding curve + its migrated PumpSwap AMM pools.
    PumpFun,
}

impl VenueId {
    /// Stable lower-snake identifier for logs / persistence.
    pub fn as_str(self) -> &'static str {
        match self {
            VenueId::PumpFun => "pumpfun",
        }
    }
}

/// A launchpad a wrapping trader can execute against. Implemented by the venue
/// crate on its trader struct; the engine below it stays venue-agnostic.
pub trait Venue {
    /// Which launchpad this venue drives.
    fn venue_id(&self) -> VenueId;

    /// The venue-agnostic send engine this venue runs its trades through.
    fn engine(&self) -> &Engine;
}
