// ---------------------------------------------------------------------------
// On-chain event discriminators (emitted via `emit!` in "Program data:" logs)
// ---------------------------------------------------------------------------
//
// The bonding-curve / buy-variant / admin / pump_amm *instruction*
// discriminators, plus the `TradeEvent` / `CreateEvent` / anchor-CPI *event*
// tags, live in the decoupled `ingest-laserstream` + `pump-trader` crates (their
// own copies). `trading_core` only needs the two PumpSwap swap-event tags below.

// ── PumpSwap (pump_amm) post-migration swap events ───────────────────────────
// Only the leading fields (amounts, pool, user) are read; trailing fields added
// in later program versions are tolerated by Borsh deserialization.
/// PumpSwap `BuyEvent` discriminator.
pub const PUMP_SWAP_BUY_EVENT_DISCRIMINATOR: [u8; 8] = [103, 244, 82, 31, 44, 245, 119, 119];
/// PumpSwap `SellEvent` discriminator.
pub const PUMP_SWAP_SELL_EVENT_DISCRIMINATOR: [u8; 8] = [62, 47, 55, 10, 165, 3, 220, 42];
