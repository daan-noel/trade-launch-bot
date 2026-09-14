//! Trader Analysis **episode ledger**: one wallet's transactions on one mint,
//! folded into round trips (episodes) on the exact wallet basis.
//!
//! An episode opens on a transaction while the wallet holds none of the mint and
//! closes on the sell that brings the held amount back down to what it came in
//! with, plus dust ([`DUST_DIVISOR`]). Its SOL is what the wallet moved: each
//! transaction's payer net flow (`trades.payer_net_lamports`), every fee, tip and
//! venue charge included. There is no modeled fallback: a transaction without an
//! exact flow marks its episode `missing_flow`, and the episode counts in no PnL
//! figure. Plan: `docs/roadmap/trader-analysis-episode-ledger-plan.md`.

use serde::Serialize;

use crate::config::constants::lamports_to_sol;
use crate::models::MarkQuote;
use crate::strategies::kernel::{sell_value_proceeds, weighted_return_pct, CostModel};

/// An episode closes once the tokens still held are at most `1 / DUST_DIVISOR`
/// (0.1 %) of what it bought, above what it came in with.
pub const DUST_DIVISOR: i64 = 1_000;

/// One transaction of one wallet on one mint, its legs already collapsed: the
/// flow is per transaction, so it cannot be split across legs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WalletTx {
    pub slot: i64,
    pub tx_index: i32,
    pub time_ms: i64,
    /// Tokens the wallet bought minus tokens it sold in this transaction (raw
    /// units). A transaction that nets zero counts on the sell side.
    pub token_delta: i64,
    /// Signed lamports the transaction moved in the wallet (negative = paid out).
    /// `None` when it is not exact: not recorded, paid by another account, or
    /// shared with another mint or wallet.
    pub flow_lamports: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EpisodeStatus {
    /// Sold down to dust with every flow exact: the only status PnL counts.
    Closed,
    /// Still holding after the last transaction read.
    Open,
    /// Sold down, but a flow is missing or a sell exceeded what the ledger saw
    /// bought. Counted, never summed.
    Incomplete,
}

/// One round trip. SOL figures are `None` rather than a guess whenever a flow
/// is missing.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WalletEpisode {
    pub status: EpisodeStatus,
    /// A transaction in the episode has no exact flow.
    pub missing_flow: bool,
    /// The wallet sold more than the ledger saw it buy: tokens that arrived by
    /// transfer, or a buy older than the data read.
    pub unseen_buy: bool,
    pub entry_slot: i64,
    pub entry_tx_index: i32,
    pub entry_ms: i64,
    /// The closing sell; `None` while open. A later sell of the left-over dust
    /// adds its SOL to the episode without moving the exit.
    pub exit_slot: Option<i64>,
    pub exit_tx_index: Option<i32>,
    pub exit_ms: Option<i64>,
    pub buy_count: u32,
    pub sell_count: u32,
    pub bought_tokens: i64,
    pub sold_tokens: i64,
    /// Tokens held after the episode's last transaction (dust once closed).
    pub held_tokens: i64,
    /// SOL the buying transactions took from the wallet.
    pub sol_in: Option<f64>,
    /// SOL the selling transactions returned to the wallet.
    pub sol_out: Option<f64>,
    /// `sol_out - sol_in`: what the round trip left in the wallet. `Closed` only.
    pub net_sol: Option<f64>,
    /// `net_sol / sol_in x 100`, the house return rule. `Closed` with a buy only.
    pub pnl_pct: Option<f64>,
    /// `Open` only: what the tokens held would sell for now ([`open_mark_sol`]).
    /// Never summed with an exact figure.
    pub mark_sol: Option<f64>,
    /// `Open` only: SOL moved so far + `mark_sol`. An estimate.
    pub open_pnl_sol: Option<f64>,
}

/// Tape position and time of one transaction.
#[derive(Clone, Copy)]
struct Pos {
    slot: i64,
    tx_index: i32,
    time_ms: i64,
}

impl Pos {
    fn of(tx: &WalletTx) -> Self {
        Self { slot: tx.slot, tx_index: tx.tx_index, time_ms: tx.time_ms }
    }
}

/// An episode while it folds, in exact lamports.
struct Acc {
    carried_in: i64,
    entry: Pos,
    exit: Option<Pos>,
    buy_count: u32,
    sell_count: u32,
    bought: i64,
    sold: i64,
    held: i64,
    in_lamports: i64,
    out_lamports: i64,
    missing_flow: bool,
    unseen_buy: bool,
}

impl Acc {
    fn open(tx: &WalletTx, carried_in: i64) -> Self {
        Self {
            carried_in,
            entry: Pos::of(tx),
            exit: None,
            buy_count: 0,
            sell_count: 0,
            bought: 0,
            sold: 0,
            held: carried_in,
            in_lamports: 0,
            out_lamports: 0,
            missing_flow: false,
            unseen_buy: false,
        }
    }

    /// Book one transaction's tokens and flow. `held` is the caller's to move.
    fn add(&mut self, tx: &WalletTx) {
        let buying = tx.token_delta > 0;
        if buying {
            self.buy_count += 1;
            self.bought += tx.token_delta;
        } else {
            self.sell_count += 1;
            self.sold -= tx.token_delta;
        }
        match tx.flow_lamports {
            Some(f) if buying => self.in_lamports -= f,
            Some(f) => self.out_lamports += f,
            None => self.missing_flow = true,
        }
    }

    fn finish(self, mark: Option<MarkQuote>) -> WalletEpisode {
        let status = match self.exit {
            None => EpisodeStatus::Open,
            Some(_) if self.missing_flow || self.unseen_buy => EpisodeStatus::Incomplete,
            Some(_) => EpisodeStatus::Closed,
        };
        let exact = |l: i64| (!self.missing_flow).then(|| lamports_to_sol(l));
        let (sol_in, sol_out) = (exact(self.in_lamports), exact(self.out_lamports));
        let moved = exact(self.out_lamports - self.in_lamports);
        let closed = status == EpisodeStatus::Closed;
        let net_sol = moved.filter(|_| closed);
        let pnl_pct = match (net_sol, sol_in) {
            (Some(net), Some(cost)) if cost > 0.0 => Some(weighted_return_pct(net, cost)),
            _ => None,
        };
        let is_open = status == EpisodeStatus::Open;
        let mark_sol =
            mark.filter(|_| is_open).map(|q| open_mark_sol(self.held, &q, !self.missing_flow));
        let open_pnl_sol = moved.zip(mark_sol).map(|(m, mark)| m + mark);
        WalletEpisode {
            status,
            missing_flow: self.missing_flow,
            unseen_buy: self.unseen_buy,
            entry_slot: self.entry.slot,
            entry_tx_index: self.entry.tx_index,
            entry_ms: self.entry.time_ms,
            exit_slot: self.exit.map(|p| p.slot),
            exit_tx_index: self.exit.map(|p| p.tx_index),
            exit_ms: self.exit.map(|p| p.time_ms),
            buy_count: self.buy_count,
            sell_count: self.sell_count,
            bought_tokens: self.bought,
            sold_tokens: self.sold,
            held_tokens: self.held,
            sol_in,
            sol_out,
            net_sol,
            pnl_pct,
            mark_sol,
            open_pnl_sol,
        }
    }
}

/// What an open episode's `held` tokens would sell for in the pool `mark` quotes:
/// the venue's sell of the bag — its fee and our impact on the pool's depth
/// ([`CostModel::venue_only`]), without the wallet's own transaction cost, which
/// is its choice. An episode with a missing flow (opened before the flow was
/// recorded) keeps the plain `held x spot` estimate it was always shown with.
pub fn open_mark_sol(held: i64, mark: &MarkQuote, exact: bool) -> f64 {
    let value = held as f64 * mark.price;
    if !exact {
        return value;
    }
    let costs = CostModel::venue_only().at_venue_fee(mark.venue_fee_bps);
    sell_value_proceeds(value, mark.reserve_sol, &costs, false)
}

/// Fold one wallet's transactions on one mint, in the canonical tape order
/// `(slot, tx_index)`, into its episodes. `mark` (the mint's pool now, SOL per raw
/// token unit) marks a still-open episode.
pub fn wallet_episodes(txs: &[WalletTx], mark: Option<MarkQuote>) -> Vec<WalletEpisode> {
    let mut done: Vec<Acc> = Vec::new();
    let mut cur: Option<Acc> = None;
    // Tokens the ledger has seen the wallet hold, never below zero.
    let mut held: i64 = 0;
    for tx in txs {
        // A sell with no episode open belongs to the last one: the dust it left,
        // or more of a bag whose buy the ledger never saw.
        if cur.is_none() && tx.token_delta <= 0 {
            if let Some(last) = done.last_mut().filter(|l| l.unseen_buy || -tx.token_delta <= held) {
                last.add(tx);
                if last.unseen_buy {
                    last.exit = Some(Pos::of(tx));
                }
                held = (held + tx.token_delta).max(0);
                last.held = held;
                continue;
            }
        }
        let ep = cur.get_or_insert_with(|| Acc::open(tx, held));
        ep.add(tx);
        held += tx.token_delta;
        if held < 0 {
            ep.unseen_buy = true;
            held = 0;
        }
        ep.held = held;
        if tx.token_delta <= 0 && held <= ep.carried_in + ep.bought / DUST_DIVISOR {
            ep.exit = Some(Pos::of(tx));
            done.extend(cur.take());
        }
    }
    done.extend(cur);
    done.into_iter().map(|a| a.finish(mark)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOL: i64 = 1_000_000_000;

    /// One transaction at slot `n`, one second apart.
    fn tx(n: i64, token_delta: i64, flow_lamports: Option<i64>) -> WalletTx {
        WalletTx { slot: n, tx_index: 0, time_ms: n * 1_000, token_delta, flow_lamports }
    }

    fn close(a: Option<f64>, b: f64) -> bool {
        a.is_some_and(|a| (a - b).abs() < 1e-9)
    }

    /// A pool at `price` with no known depth.
    fn spot(price: f64) -> Option<MarkQuote> {
        Some(MarkQuote { price, reserve_sol: None, venue_fee_bps: None })
    }

    #[test]
    fn one_round_trip_books_what_the_wallet_moved() {
        let eps = wallet_episodes(&[tx(1, 1_000, Some(-SOL)), tx(2, -1_000, Some(SOL * 5 / 4))], None);
        assert_eq!(eps.len(), 1);
        let e = &eps[0];
        assert_eq!(e.status, EpisodeStatus::Closed);
        assert!(close(e.sol_in, 1.0) && close(e.sol_out, 1.25));
        assert!(close(e.net_sol, 0.25));
        assert!(close(e.pnl_pct, 25.0));
        assert_eq!((e.entry_slot, e.exit_slot, e.exit_ms), (1, Some(2), Some(2_000)));
        assert_eq!((e.buy_count, e.sell_count, e.held_tokens), (1, 1, 0));
    }

    #[test]
    fn a_re_entry_is_a_second_episode() {
        let eps = wallet_episodes(
            &[
                tx(1, 100, Some(-SOL)),
                tx(2, -100, Some(SOL / 2)),
                tx(3, 200, Some(-SOL)),
                tx(4, -200, Some(SOL * 2)),
            ],
            None,
        );
        assert_eq!(eps.len(), 2);
        assert!(close(eps[0].pnl_pct, -50.0));
        assert!(close(eps[1].pnl_pct, 100.0));
        assert_eq!(eps[1].entry_slot, 3);
    }

    #[test]
    fn partial_sells_stay_one_episode_until_flat() {
        let eps = wallet_episodes(
            &[tx(1, 1_000, Some(-SOL)), tx(2, -400, Some(SOL / 2)), tx(3, -600, Some(SOL * 4 / 5))],
            None,
        );
        assert_eq!(eps.len(), 1);
        assert_eq!(eps[0].sell_count, 2);
        assert!(close(eps[0].net_sol, 0.3));
        assert_eq!(eps[0].exit_slot, Some(3));
    }

    #[test]
    fn dust_closes_and_its_later_sale_joins_the_episode() {
        let eps = wallet_episodes(
            &[
                tx(1, 1_000_000, Some(-SOL)),
                tx(2, -999_500, Some(SOL)),
                tx(3, -500, Some(1_000)),
            ],
            None,
        );
        assert_eq!(eps.len(), 1);
        let e = &eps[0];
        assert_eq!(e.status, EpisodeStatus::Closed);
        assert_eq!(e.exit_slot, Some(2), "the dust sale does not move the exit");
        assert_eq!((e.sell_count, e.held_tokens), (2, 0));
        assert!(close(e.net_sol, lamports_to_sol(1_000)));
    }

    #[test]
    fn more_than_dust_left_keeps_the_episode_open() {
        let eps = wallet_episodes(&[tx(1, 1_000_000, Some(-SOL)), tx(2, -998_000, Some(SOL))], spot(1e-6));
        assert_eq!(eps[0].status, EpisodeStatus::Open);
        assert_eq!(eps[0].held_tokens, 2_000);
        assert_eq!(eps[0].net_sol, None, "an open episode has no realized figure");
    }

    #[test]
    fn carried_dust_does_not_hold_the_next_episode_open() {
        let eps = wallet_episodes(
            &[
                tx(1, 1_000_000, Some(-SOL)),
                tx(2, -999_500, Some(SOL)),
                tx(3, 1_000, Some(-SOL / 100)),
                tx(4, -1_000, Some(SOL / 50)),
            ],
            None,
        );
        assert_eq!(eps.len(), 2);
        assert_eq!(eps[1].status, EpisodeStatus::Closed);
        assert!(close(eps[1].pnl_pct, 100.0));
    }

    #[test]
    fn a_sell_without_a_seen_buy_is_one_incomplete_episode() {
        let eps = wallet_episodes(
            &[
                tx(1, -500, Some(SOL / 2)),
                tx(2, -300, Some(SOL / 4)),
                tx(3, 100, Some(-SOL)),
                tx(4, -100, Some(SOL)),
            ],
            None,
        );
        assert_eq!(eps.len(), 2);
        let first = &eps[0];
        assert_eq!(first.status, EpisodeStatus::Incomplete);
        assert!(first.unseen_buy);
        assert_eq!((first.sell_count, first.exit_slot), (2, Some(2)));
        assert_eq!(first.net_sol, None);
        assert_eq!(eps[1].status, EpisodeStatus::Closed);
    }

    #[test]
    fn selling_more_than_bought_mid_episode_is_incomplete() {
        let eps = wallet_episodes(&[tx(1, 100, Some(-SOL)), tx(2, -150, Some(SOL))], None);
        assert_eq!(eps[0].status, EpisodeStatus::Incomplete);
        assert!(eps[0].unseen_buy);
        assert_eq!(eps[0].held_tokens, 0);
    }

    #[test]
    fn a_missing_flow_leaves_every_sol_figure_unknown() {
        let eps = wallet_episodes(&[tx(1, 100, Some(-SOL)), tx(2, -100, None)], None);
        let e = &eps[0];
        assert_eq!(e.status, EpisodeStatus::Incomplete);
        assert!(e.missing_flow && !e.unseen_buy);
        assert_eq!((e.sol_in, e.sol_out, e.net_sol, e.pnl_pct), (None, None, None, None));
    }

    /// The bag is marked at what selling it returns: the 0.75 SOL it is worth at
    /// spot, less the venue fee, with no transaction cost of the wallet's.
    #[test]
    fn an_open_episode_is_marked_at_what_its_bag_sells_for() {
        let eps = wallet_episodes(&[tx(1, 1_000, Some(-SOL)), tx(2, -500, Some(SOL))], spot(0.0015));
        let e = &eps[0];
        assert_eq!(e.status, EpisodeStatus::Open);
        let want = 0.75 * (1.0 - 0.0125);
        assert!(close(e.mark_sol, want));
        assert!(close(e.open_pnl_sol, want), "0 SOL moved so far + the mark");
        assert_eq!(e.exit_slot, None);
        let unpriced = wallet_episodes(&[tx(1, 1_000, Some(-SOL))], None);
        assert_eq!((unpriced[0].mark_sol, unpriced[0].open_pnl_sol), (None, None));
    }

    /// A bag sells down the pool it marks against, and a PumpSwap pool charges
    /// its own fee.
    #[test]
    fn the_open_mark_sells_into_the_pools_depth_at_its_fee() {
        let pool = MarkQuote { price: 0.001, reserve_sol: Some(10.0), venue_fee_bps: None };
        let bag = open_mark_sol(1_000, &pool, true);
        assert!((bag - 1.0 / (1.0 + 1.0 / 10.0) * (1.0 - 0.0125)).abs() < 1e-12);
        let amm = MarkQuote { venue_fee_bps: Some(95.0), ..pool };
        assert!((open_mark_sol(1_000, &amm, true) - 1.0 / 1.1 * (1.0 - 0.0095)).abs() < 1e-12);
    }

    /// An episode opened before its flow was recorded keeps the `held x spot`
    /// estimate.
    #[test]
    fn a_missing_flow_episode_keeps_the_spot_estimate() {
        let eps = wallet_episodes(&[tx(1, 1_000, None)], spot(0.0015));
        assert!(eps[0].missing_flow);
        assert!(close(eps[0].mark_sol, 1.5));
        assert_eq!(eps[0].open_pnl_sol, None);
    }
}
