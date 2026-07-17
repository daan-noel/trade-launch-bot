//! Paper execution (plan 4.4) — a transaction-free fill model that turns a
//! `SubmitBuy`/`SubmitSell` into a `FillConfirmed` the engine folds exactly like a
//! real fill. Fills price at the token's **canonical spot** at the instant the
//! engine decided to act, which keeps paper decisions identical to the
//! simulate/replay path (both fill at the spot carried by the triggering event).
//!
//! There is no on-chain identity for a paper fill, so the executor stashes no
//! signatures — the sink's `record_entry_fill`/`close` just see an empty sig list.
//!
//! PnL is purely price-ratio based (exit_sol = entry_sol · exit_price/entry_price),
//! so the (cosmetic) raw `token_amount` uses pump.fun's 6-decimal scaling.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::mpsc;

use hunter_engine::event::{Event, Fill, FillFailReason, IntentId};

use trading_core::state::token_cache::TokenCache;

/// pump.fun SPL token decimals (raw units per whole token). Paper `token_amount`
/// is cosmetic — PnL is price-ratio based — so this only needs to be consistent.
const TOKEN_SCALE: f64 = 1_000_000.0;
const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

/// How long to wait for a finite spot price before giving up a paper entry (a rule
/// that entered on arm at creation may briefly precede the first priced trade).
const PRICE_WAIT: Duration = Duration::from_secs(2);
const PRICE_POLL: Duration = Duration::from_millis(100);

/// Fill a paper buy at the current spot and report it back to the engine.
pub async fn run_entry(
    fill_tx: mpsc::Sender<Event>,
    token_cache: Arc<TokenCache>,
    intent: IntentId,
    mint: String,
    lamports: u64,
) {
    let event = match wait_for_price(&token_cache, &mint).await {
        Some(price) => {
            let sol = lamports as f64 / LAMPORTS_PER_SOL;
            let token_amount = ((sol / price) * TOKEN_SCALE).round().max(0.0) as u64;
            Event::FillConfirmed {
                intent,
                fill: Fill { price, sol, token_amount, at: Utc::now() },
            }
        }
        None => Event::FillFailed { intent, reason: FillFailReason::Timeout },
    };
    let _ = fill_tx.send(event).await;
}

/// Fill a paper sell of the held `token_amount` at the current spot.
pub async fn run_exit(
    fill_tx: mpsc::Sender<Event>,
    token_cache: Arc<TokenCache>,
    intent: IntentId,
    mint: String,
    entry_token_amount: u64,
) {
    let event = match wait_for_price(&token_cache, &mint).await {
        Some(price) => {
            let sol = (entry_token_amount as f64 / TOKEN_SCALE) * price;
            Event::FillConfirmed {
                intent,
                fill: Fill { price, sol, token_amount: entry_token_amount, at: Utc::now() },
            }
        }
        // A dead/quiet token can lack a fresh price; a paper sell still "clears" at
        // the last known price rather than stranding the position — but if we never
        // had a price at all, fail it (nothing to price against).
        None => Event::FillFailed { intent, reason: FillFailReason::Timeout },
    };
    let _ = fill_tx.send(event).await;
}

/// Poll the token cache for a finite canonical spot price, up to [`PRICE_WAIT`].
async fn wait_for_price(token_cache: &Arc<TokenCache>, mint: &str) -> Option<f64> {
    let deadline = tokio::time::Instant::now() + PRICE_WAIT;
    loop {
        if let Some(p) = current_price(token_cache, mint) {
            return Some(p);
        }
        if tokio::time::Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(PRICE_POLL).await;
    }
}

fn current_price(token_cache: &Arc<TokenCache>, mint: &str) -> Option<f64> {
    token_cache
        .get(mint)
        .and_then(|e| e.value().current_price)
        .filter(|p| p.is_finite() && *p > 0.0)
}
