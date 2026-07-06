//! Venue trait contracts — Phase 3. The seam that lets a launchpad (pump.fun,
//! raydium_launchlab, …) plug in a decoder + market/quote resolution behind a
//! stable interface, so a new launchpad is a dimension *row* + a trait impl, not a
//! schema migration. The pump.fun impl lives in `launcher`/`ingest-host` (live
//! only) — this crate defines only the contract. Placeholder now.
