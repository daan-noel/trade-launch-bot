//! **Top holders at a seat** — the Entry Context page's holder reads: how far the
//! price falls if a coin's biggest holders sold their whole bags at once, read at a
//! tape seat (his buy, the signal he followed, a target buy of the scan).
//!
//! A bag is everything a wallet bought minus everything it sold, so the book needs
//! the coin's whole history: each mint's legs are read from its creation up to its
//! last seat ([`TradeRepo::holder_tape`], the studied wallet's legs left out, as
//! every Entry Context read leaves them out), folded once in tape order through the
//! engine's [`HolderBookState`], and read at every seat on the way. A mint created
//! before the oldest tape `trades` holds has no read: its early bags are gone.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

use hunter_engine::metrics::holder_book::{HolderBookState, TopHolders};
use hunter_engine::metrics::{Side, TradeLite};
use trading_core::config::constants::lamports_to_sol;
use trading_core::storage::repositories::token_repo::TokenRepo;
use trading_core::storage::repositories::trade_repo::{HolderPrint, SlotWindow, TradeRepo};

/// Mints per tape query: a median mint has a few dozen legs, the busiest a few
/// thousand a day.
const MINTS_PER_QUERY: usize = 100;

/// Slack on the `block_time` bounds, which only prune chunks.
const TIME_SLACK_SECS: i64 = 60;

/// The price drop, in percent, if each group of the biggest holders sold its whole
/// bags at once ([`HolderBookState::top_dump_drop_pct`]).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct HolderRead {
    /// Wallets holding more than zero tokens.
    pub holders: u32,
    pub top1_drop_pct: f64,
    pub top10_drop_pct: f64,
    pub top1pct_drop_pct: f64,
    pub top10pct_drop_pct: f64,
}

/// A tape seat on one mint: the book is read after every leg before `(slot, tx_index)`.
pub type Seat = (i64, i32);

/// One read asked for: the mint, the seat, and the seat's block time (bounds the read).
pub struct HolderAsk {
    pub mint: String,
    pub seat: Seat,
    pub at: DateTime<Utc>,
}

/// The read at every asked seat whose mint has its whole history on the tape.
pub async fn holders_at(
    repo: &TradeRepo,
    wallet: &str,
    asks: &[HolderAsk],
    tape_floor: Option<DateTime<Utc>>,
) -> anyhow::Result<HashMap<(String, Seat), HolderRead>> {
    let mut per_mint: HashMap<&str, (Vec<Seat>, DateTime<Utc>)> = HashMap::new();
    for a in asks {
        let e = per_mint.entry(a.mint.as_str()).or_insert_with(|| (Vec::new(), a.at));
        e.0.push(a.seat);
        e.1 = e.1.max(a.at);
    }
    let mints: Vec<String> = per_mint.keys().map(|m| m.to_string()).collect();
    let created = TokenRepo::new(repo.pool().clone()).find_symbols_for(&mints).await?;
    let slack = Duration::seconds(TIME_SLACK_SECS);
    let windows: Vec<SlotWindow> = per_mint
        .iter()
        .filter_map(|(mint, (seats, last))| {
            let (_, born) = created.get(*mint)?;
            if tape_floor.is_some_and(|f| *born < f) {
                return None;
            }
            Some(SlotWindow {
                mint_address: mint.to_string(),
                lo_slot: 0,
                hi_slot: seats.iter().map(|s| s.0).max()?,
                lo_time: *born - slack,
                hi_time: *last + slack,
            })
        })
        .collect();

    let mut out = HashMap::new();
    for chunk in windows.chunks(MINTS_PER_QUERY) {
        let tape = repo.holder_tape(chunk, Some(wallet)).await?;
        for legs in tape.chunk_by(|a, b| a.mint_address == b.mint_address) {
            let mint = &legs[0].mint_address;
            let mut seats = per_mint[mint.as_str()].0.clone();
            for (seat, read) in fold(legs, &mut seats) {
                out.insert((mint.clone(), seat), read);
            }
        }
    }
    Ok(out)
}

/// Fold one mint's legs (tape order) and read the book at each seat; a seat whose
/// read is not whole (no priced leg yet, a leg without a token amount) is left out.
fn fold(legs: &[HolderPrint], seats: &mut [Seat]) -> Vec<(Seat, HolderRead)> {
    seats.sort_unstable();
    let mut book = HolderBookState::default();
    let mut out = Vec::with_capacity(seats.len());
    let mut next = 0;
    for p in legs {
        while next < seats.len() && seats[next] <= (p.slot, p.tx_index) {
            out.extend(read_at(&book).map(|r| (seats[next], r)));
            next += 1;
        }
        if let Some(t) = trade_lite(p) {
            book.on_trade(&t);
        }
    }
    for &seat in &seats[next..] {
        out.extend(read_at(&book).map(|r| (seat, r)));
    }
    out
}

/// The book's read now; `None` unless every group reads a number.
fn read_at(book: &HolderBookState) -> Option<HolderRead> {
    let [top1, top10, top1pct, top10pct] =
        [TopHolders::One, TopHolders::Ten, TopHolders::OnePct, TopHolders::TenPct].map(|g| book.top_dump_drop_pct(g));
    [top1, top10, top1pct, top10pct].iter().all(|v| v.is_finite()).then(|| HolderRead {
        holders: u32::try_from(book.holders()).unwrap_or(u32::MAX),
        top1_drop_pct: top1,
        top10_drop_pct: top10,
        top1pct_drop_pct: top1pct,
        top10pct_drop_pct: top10pct,
    })
}

/// A leg as the holder book reads it: who, which way, how many tokens, and the
/// reserve pair it left (`price` = SOL / tokens, `priced_reserve_sol` = SOL). `None`
/// for a proxied leg with no payer, whose holder is unknown.
fn trade_lite(p: &HolderPrint) -> Option<TradeLite> {
    let holder = p.holder_id?;
    let (sol, tokens) = match (p.reserve_lamports, p.reserve_token) {
        (Some(l), Some(t)) if l > 0 && t > 0 => (lamports_to_sol(l), t as f64),
        _ => (f64::NAN, f64::NAN),
    };
    Some(TradeLite {
        side: if p.is_buy { Side::Buy } else { Side::Sell },
        wallet_hash: u64::from(holder as u32),
        token_amount: p.token_amount as f64,
        slot: u64::try_from(p.slot).unwrap_or(0),
        price: sol / tokens,
        priced_reserve_sol: sol,
        ..TradeLite::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hunter_engine::metrics::holder_book::dump_drop_pct;

    /// A leg leaving `pool` raw tokens against 30 SOL.
    fn leg(slot: i64, tx: i32, holder: i32, is_buy: bool, tokens: i64, pool: i64) -> HolderPrint {
        HolderPrint {
            mint_address: "M".into(),
            slot,
            tx_index: tx,
            holder_id: Some(holder),
            is_buy,
            token_amount: tokens,
            reserve_lamports: Some(30_000_000_000),
            reserve_token: Some(pool),
            leg_index: 0,
        }
    }

    #[test]
    fn a_seat_reads_every_leg_before_it_and_none_after() {
        let legs = [leg(10, 0, 1, true, 300, 9_700), leg(10, 5, 2, true, 100, 9_600), leg(12, 0, 1, false, 300, 9_900)];
        // Before the second buy; between the slots; after everything.
        let mut seats = [(12, 0), (10, 5), (13, 0)];
        let got: HashMap<Seat, HolderRead> = fold(&legs, &mut seats).into_iter().collect();
        assert_eq!(got[&(10, 5)].holders, 1);
        assert!((got[&(10, 5)].top1_drop_pct - dump_drop_pct(9_700.0, 300.0)).abs() < 1e-9);
        assert!((got[&(12, 0)].top10_drop_pct - dump_drop_pct(9_600.0, 400.0)).abs() < 1e-9);
        assert_eq!(got[&(13, 0)].holders, 1);
        assert!((got[&(13, 0)].top1_drop_pct - dump_drop_pct(9_900.0, 100.0)).abs() < 1e-9);
    }

    #[test]
    fn a_seat_before_the_first_priced_leg_has_no_read() {
        let mut seats = [(10, 0)];
        assert!(fold(&[leg(10, 0, 1, true, 300, 9_700)], &mut seats).is_empty());
    }
}
