use super::protocol::LAMPORTS_PER_SOL;

// ---------------------------------------------------------------------------
// Trade slippage
// ---------------------------------------------------------------------------

// Slippage is a **blank-or-a-number** knob, one key per side, and a typed number
// is honored LITERALLY — nothing sits between the percent the operator typed and
// the `min_out` the trader encodes. Blank is what carries the per-side policy:
//
//   | field            | blank                     | a typed number    |
//   | Buy slippage %   | `DEFAULT_SLIPPAGE_BPS`    | used as typed     |
//   | Sell slippage %  | no floor (min_out = 1)    | used as typed     |
//
// The asymmetry is deliberate: a buy with no opinion still gets protection, a
// sell with no opinion dumps (exits must clear during a rapid dump). `0` is NOT a
// spelling of "no floor" — it is rejected with a 400 at every write door (see
// [`validate_slippage_bps`]), because under literal handling `0` would mean
// "revert on any movement at all".

/// Default buy-side slippage in basis points (100 = 1%), used only when the
/// Settings buy field is **blank**. 2500 = 25% — an untouched setting still lands
/// an entry on a fast-moving new token ("I have to buy even at some loss").
pub const DEFAULT_SLIPPAGE_BPS: u64 = 2_500;
/// Hard ceiling applied to any client-supplied slippage, to guard against a
/// fat-finger or hostile value. 5000 bps = 50%. A ceiling only ever *loosens* a
/// floor, so it can never turn a fill into a revert — which is why it survives
/// while the old `SLIPPAGE_MIN_BPS` floor does not: that one inverted the meaning
/// of `0` (from "accept any fill" into the tightest possible floor).
pub const SLIPPAGE_MAX_BPS: u64 = 5_000;

/// The ONE validator for a client-supplied slippage value, shared by the settings
/// write and the manual trade endpoints. `Some(0)` is rejected: blank is how you
/// say "no floor", so a literal `0` can only be a mistake. Returns the 400 body's
/// message on rejection.
pub fn validate_slippage_bps(field: &str, value: Option<u64>) -> Result<(), String> {
    if value == Some(0) {
        return Err(format!(
            "{field} must be greater than 0 — leave it blank for no limit"
        ));
    }
    Ok(())
}

/// Resolve buy slippage: per-request → `buy_slippage_bps` → [`DEFAULT_SLIPPAGE_BPS`].
/// Always `Some` — a buy with no opinion still gets protection. Only the max
/// ceiling is applied; the typed value is otherwise passed through untouched.
pub fn resolve_buy_slippage_bps(buy_setting: Option<u64>, request: Option<u64>) -> Option<u64> {
    Some(
        request
            .or(buy_setting)
            .unwrap_or(DEFAULT_SLIPPAGE_BPS)
            .min(SLIPPAGE_MAX_BPS),
    )
}

/// Resolve sell slippage: per-request → `sell_slippage_bps` → **no floor**.
/// `None` = `min_out = 1`, always fills — the default so bot exits clear at any
/// price during a rapid dump rather than stalling on repeated slippage reverts.
/// Only the max ceiling is applied to a typed value.
pub fn resolve_sell_slippage_bps(sell_setting: Option<u64>, request: Option<u64>) -> Option<u64> {
    request.or(sell_setting).map(|bps| bps.min(SLIPPAGE_MAX_BPS))
}

/// Per-trade SOL ceiling on the manual buy API (`POST /api/solana/wallet/buy`).
/// A fat-finger ("buy 1000 SOL") or hostile value is rejected with a 400 before
/// any on-chain work. The `pump_trader` crate enforces its own `MAX_BUY_SOL`
/// backstop one layer down regardless.
pub const MAX_MANUAL_BUY_SOL: f64 = 5.0;

/// Age past which an unresolved real `BuySubmitted` needs manual review (B3):
/// the reaper could neither adopt a fill nor prove every submitted sig reverted.
/// SSOT for the reaper's flag, the SSE `needs_review` marker, and the
/// `PositionResponse` derivation — one window everywhere.
pub const BUY_SUBMITTED_REVIEW_SECS: u64 = 600;

// ---------------------------------------------------------------------------
// Strategy thresholds (hot path — read per-event; must stay as const)
// ---------------------------------------------------------------------------

/// Worst-case paper/backtest fill window (entry and exit) — used by
/// [`crate::strategies::paper_fill`]: the fill candidates are the trigger/fire
/// slot S plus the next observed slot after S, provided that slot is within this
/// many slots of S. If the next slot is farther away only slot S is used.
pub const MAX_FILL_WAIT_SLOTS: u64 = 3; // ≈ 1 s at 400 ms/slot


// ── Dead-token detection ─────────────────────────────────────────────────────
// A token is "dead" when BOTH conditions hold simultaneously:
//   1. Real SOL reserves are below `DEAD_MAX_LIQUIDITY_SOL` — liquidity is gone.
//   2. No meaningful trade (≥ `DEAD_MEANINGFUL_TRADE_SOL`) has arrived for at
//      least `DEAD_QUIET_SECS` — activity has permanently ceased.
// The quiet requirement means a token that temporarily dips in reserves but then
// recovers will NOT be flagged dead (a new meaningful trade resets the clock).
// The verdict flips to true exactly once and stays there.
//
// The constants + the `is_dead_verdict` predicate live in the pure engine crate
// (`hunter_engine::deadness`, the SSOT) and are re-exported here so every
// existing `config::constants::DEAD_*` path keeps working unchanged.
pub use hunter_engine::deadness::{
    DEAD_MAX_LIQUIDITY_SOL, DEAD_MEANINGFUL_TRADE_SOL, DEAD_QUIET_SECS,
};

// ---------------------------------------------------------------------------
// Ingest / cache sizing (restart required to change)
// ---------------------------------------------------------------------------

/// Trades below this size are dust (bot noise / probe txs) and are not ingested.
pub const MIN_TRADE_LAMPORTS: u64 = 10_000;
pub const MIN_TRADE_SOL: f64 = MIN_TRADE_LAMPORTS as f64 / LAMPORTS_PER_SOL as f64;

/// A migrated token's PumpSwap pool is included in the live subscription set
/// only if it has traded within this window. Quiet pools are re-added when
/// fresh activity appears. Tune up to keep slower pools live.
pub const POOL_SUBSCRIBE_ACTIVITY_WINDOW_SECONDS: i64 = 3 * 3600; // 3 hours

/// Upper bound on how many tokens the live startup cache seed pulls from the
/// `tokens` table into the in-RAM strategy `TokenCache`. Startup time + resident
/// memory stay bounded as `tokens` grows.
///
/// This is the LIVE (EC2) **tracking**-seed cap — it governs how many tokens the
/// live box tracks in RAM for the strategy hot path, and must NOT be raised on the
/// server (4 GB guardrail). It does NOT bound the `GET /api/tokens` list: `live`
/// pages that list straight from Postgres (see `api::handlers::tokens::sql`), so the
/// full token universe is visible regardless of this cap. Named `TRACKING` (not the
/// old `TOKEN`) to make that separation explicit — it seeds the tracking cache, not
/// the table.
pub const SEED_TRACKING_LIMIT: i64 = 25_000;
/// Lab's in-RAM token-list snapshot base cap. `lab` runs on the workstation (big
/// RAM, speed-critical analysis) and wants the WHOLE token universe resident, so its
/// snapshot loads up to this many rows — bounded-but-huge, well past the expected
/// 100K+, rather than literally unbounded (a backstop if `tokens` ever grows to
/// millions locally). Lab-only; never used by the live box.
pub const LAB_TOKEN_LIST_LIMIT: i64 = 1_000_000;
/// Only tokens created within this window are pulled into the startup cache seed.
/// Tokens older than this aren't tracked live until they trade again.
pub const SEED_ACTIVITY_WINDOW_DAYS: i64 = 7;
/// Lab's token-list snapshot window: how far back the full in-RAM list base reaches.
/// Wider than the live tracking seed window because lab analyzes historical tokens,
/// not just the live-tracked recent set. Tune up for deeper local history.
pub const LAB_TOKEN_LIST_WINDOW_DAYS: i64 = 90;
/// Hard cap on retained in-memory trade history per token. The live token cache
/// (`state::token_cache`, which re-exports this) keeps only the most recent
/// `MAX_TRADES_RETAINED` trades; the oldest are trimmed from the front once the vec
/// exceeds the cap by `TRADES_TRIM_SLACK` (batched so the O(n) front-drain amortizes
/// to O(1) per trade).
///
/// SAFETY — why a fixed cap doesn't corrupt any trade/exit decision: every consumer
/// that walks `trades` either needs only the tail (the exit re-walk/memo for an open
/// position whose entry is within the window) or treats it as a display sample
/// (`unique_wallets`, swing analysis, the trades API). For the sniper use case a
/// position's whole entry→exit span is a tiny fraction of this window, so the cap
/// never reaches a trade that an open position still needs. The exit memo folds
/// against an *absolute* count (`CachedExitState::consumed_abs`) mapped through
/// `trades_base`, so front-trims can never skip or double-fold a trade.
/// Backtest/paper sims read full history from the DB, not this cache, so they are
/// unaffected.
///
/// Lives in `config` (not `state::token_cache`) so it stays single-source for the
/// seed cap below without `config` depending on `state`.
pub const MAX_TRADES_RETAINED: usize = 2_500;
/// Per-mint cap on trade history pulled at seed time. Matches the live retained
/// cap (`MAX_TRADES_RETAINED`) so a high-volume token reads only its newest window.
pub const SEED_TRADES_PER_MINT: i64 = MAX_TRADES_RETAINED as i64;

/// Floor for the ingest watchdog stall window. Kept generous because the watchdog
/// only ever fires on a *genuine* downstream wedge — the stall predicate is gated
/// on "work is pending" (the DB queue is non-empty), so a quiet upstream or an
/// in-progress reconnect can never trip it regardless of the window. The settings
/// API clamps writes here and the watchdog re-applies it defensively every tick.
pub const WATCHDOG_STALL_TIMEOUT_FLOOR_SECS: u64 = 90;
/// Floor for the watchdog check cadence — a `0`/tiny interval would busy-spin the
/// OS thread for no detection benefit.
pub const WATCHDOG_CHECK_INTERVAL_FLOOR_SECS: u64 = 5;

/// Maximum age of a newly-created token that the snipe entry gate will buy.
/// A `TokenCreated` event older than this is rejected before criteria matching
/// — prevents gap-replayed 10h-old creates from being sniped. Requires A3
/// (accurate `created_at` on replayed creates) to be effective.
pub const MAX_SNIPE_AGE_SECS: i64 = 30;

/// Keyset page size for analysis scans (tpsl matched / simulate) that stream
/// the whole `tokens` table one page at a time.
pub const ANALYSIS_SCAN_PAGE: i64 = 5_000;

/// How often the runtime token-cache eviction sweep runs.
pub const TOKEN_CACHE_EVICT_INTERVAL_SECONDS: u64 = 120; // 2 minutes
/// A tracked token inactive for at least this long with no open position is
/// evicted from the in-memory cache. A mint with an open position is always
/// exempt so an open exit never strands.
pub const TOKEN_CACHE_EVICT_IDLE_SECONDS: i64 = 2700; // 45 min

/// How often the background task refreshes the DB-backed token-list snapshot.
pub const TOKEN_LIST_DB_REFRESH_SECS: u64 = 120;

#[cfg(test)]
mod slippage_tests {
    use super::*;

    /// The guard: what the operator types is what the trader uses. Nothing (no
    /// floor, no rounding, no sentinel decode) may sit between the two. This test
    /// fails the moment a `clamp`/`Some(0) => None` arm is reintroduced anywhere on
    /// the resolve path.
    #[test]
    fn a_typed_percent_reaches_the_trader_unchanged() {
        for pct in [0.01_f64, 0.1, 1.0, 5.0, 12.34, 50.0] {
            // The frontend's percent → bps conversion (`Math.round(pct * 100)`).
            let bps = (pct * 100.0).round() as u64;
            assert!(validate_slippage_bps("buy_slippage_bps", Some(bps)).is_ok());
            assert_eq!(
                resolve_buy_slippage_bps(Some(bps), None),
                Some(bps),
                "buy {pct}% must resolve to exactly {bps} bps"
            );
            assert_eq!(
                resolve_sell_slippage_bps(Some(bps), None),
                Some(bps),
                "sell {pct}% must resolve to exactly {bps} bps"
            );
        }
    }

    /// Blank is what carries the per-side policy — buy defaults, sell dumps.
    #[test]
    fn blank_buy_defaults_and_blank_sell_has_no_floor() {
        assert_eq!(
            resolve_buy_slippage_bps(None, None),
            Some(DEFAULT_SLIPPAGE_BPS)
        );
        assert_eq!(resolve_sell_slippage_bps(None, None), None);
    }

    /// A per-request value (manual sell) still wins over the persisted setting.
    #[test]
    fn request_overrides_the_persisted_setting() {
        assert_eq!(resolve_buy_slippage_bps(Some(100), Some(700)), Some(700));
        assert_eq!(resolve_sell_slippage_bps(Some(100), Some(700)), Some(700));
        assert_eq!(resolve_sell_slippage_bps(None, Some(700)), Some(700));
    }

    /// The ceiling only ever loosens a floor, so it stays; it can't cause a revert.
    #[test]
    fn only_the_max_ceiling_still_applies() {
        assert_eq!(
            resolve_buy_slippage_bps(Some(99_999), None),
            Some(SLIPPAGE_MAX_BPS)
        );
        assert_eq!(
            resolve_sell_slippage_bps(Some(99_999), None),
            Some(SLIPPAGE_MAX_BPS)
        );
    }

    /// `0` is retired as a sentinel: it is a 400 at every write door, never a
    /// silently-rewritten value.
    #[test]
    fn zero_is_rejected_not_rewritten() {
        assert!(validate_slippage_bps("sell_slippage_bps", Some(0)).is_err());
        assert!(validate_slippage_bps("sell_slippage_bps", None).is_ok());
        assert!(validate_slippage_bps("sell_slippage_bps", Some(1)).is_ok());
    }
}
