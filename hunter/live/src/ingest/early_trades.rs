//! Trades that reach the consumer before their token's `TokenCreated`.
//!
//! Creates and trades decode on two lanes that feed one event channel, so a buy
//! bundled right behind a create (same slot, next transaction) can be decoded
//! first and arrive while its mint is not in `token_cache` yet. The consumer drops
//! a trade for an unknown mint, which loses exactly that buy. Instead it parks the
//! trade here; the create releases it, and it is replayed right after the creation
//! transaction's own trade, so the token sees its trades in chain order.
//!
//! A parked trade lives at most [`EARLY_TRADE_SLOTS`] slots past the newest slot
//! seen. One whose create never arrives belongs to a mint this bot does not track
//! (an excluded Mayhem token, a token evicted from the cache) and expires, which is
//! the old drop. The queue is bounded by [`EARLY_TRADE_CAP`]; overflowing it evicts
//! the oldest trade and is counted by the caller.

use std::collections::VecDeque;

use ingest_pumpfun::event::Trade as IlTrade;

/// How many slots past the newest seen slot a trade waits for its create. The two
/// decode lanes differ by milliseconds, so ~1.6 s is ample, and short enough that
/// the queue holds only the untracked trades of the last few slots.
pub const EARLY_TRADE_SLOTS: u64 = 4;

/// Hard cap on parked trades.
pub const EARLY_TRADE_CAP: usize = 2048;

/// Trades a create released, held until the creation transaction's own trade has
/// been applied so they land after it.
struct Held {
    mint: String,
    create_signature: String,
    create_slot: u64,
    trades: Vec<IlTrade>,
}

#[derive(Default)]
pub struct EarlyTrades {
    parked: VecDeque<IlTrade>,
    held: Vec<Held>,
    newest_slot: u64,
}

fn chain_order(trades: &mut [IlTrade]) {
    trades.sort_by_key(|t| (t.slot, t.tx_index, t.leg_index));
}

impl EarlyTrades {
    /// Advance the feed clock. Called on every trade.
    pub fn observe_slot(&mut self, slot: u64) {
        self.newest_slot = self.newest_slot.max(slot);
    }

    fn is_expired(&self, slot: u64) -> bool {
        slot + EARLY_TRADE_SLOTS < self.newest_slot
    }

    /// Park a trade whose mint is unknown. Returns `true` when the queue was full
    /// and the oldest parked trade was evicted to make room.
    pub fn park(&mut self, trade: IlTrade) -> bool {
        while self.parked.front().is_some_and(|t| self.is_expired(t.slot)) {
            self.parked.pop_front();
        }
        let evicted = self.parked.len() >= EARLY_TRADE_CAP;
        if evicted {
            self.parked.pop_front();
        }
        self.parked.push_back(trade);
        evicted
    }

    /// A tracked token was created: take its parked trades. They are returned for
    /// replay now when the creation transaction's own trade cannot follow (the
    /// create carried no dev buy, or that trade was itself parked); otherwise they
    /// are held for [`Self::release_after`].
    pub fn on_create(
        &mut self,
        mint: &str,
        create_signature: &str,
        create_slot: u64,
        has_dev_buy: bool,
    ) -> Vec<IlTrade> {
        if !self.parked.iter().any(|t| t.mint == mint) {
            return Vec::new();
        }
        let mut trades = Vec::new();
        let mut keep = VecDeque::with_capacity(self.parked.len());
        for t in self.parked.drain(..) {
            if t.mint == mint {
                trades.push(t);
            } else {
                keep.push_back(t);
            }
        }
        self.parked = keep;
        chain_order(&mut trades);
        let dev_buy_parked = trades.iter().any(|t| t.signature == create_signature);
        if !has_dev_buy || dev_buy_parked {
            return trades;
        }
        self.held.push(Held {
            mint: mint.to_owned(),
            create_signature: create_signature.to_owned(),
            create_slot,
            trades,
        });
        Vec::new()
    }

    /// Called before a trade of a known mint is applied. When that trade is the
    /// creation transaction's own, returns the held trades to replay right after
    /// it. Free when nothing is held, which is almost always.
    pub fn release_after(&mut self, mint: &str, signature: &str) -> Vec<IlTrade> {
        if self.held.is_empty() {
            return Vec::new();
        }
        match self
            .held
            .iter()
            .position(|h| h.mint == mint && h.create_signature == signature)
        {
            Some(i) => self.held.swap_remove(i).trades,
            None => Vec::new(),
        }
    }

    /// Held trades whose creation trade never came within the window: replay them
    /// anyway rather than lose them. Free when nothing is held.
    pub fn release_overdue(&mut self) -> Vec<IlTrade> {
        if self.held.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.held.len() {
            if self.is_expired(self.held[i].create_slot) {
                out.append(&mut self.held.swap_remove(i).trades);
            } else {
                i += 1;
            }
        }
        chain_order(&mut out);
        out
    }

    #[cfg(test)]
    fn parked_len(&self) -> usize {
        self.parked.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use ingest_pumpfun::event::{Reserves, Side, Venue};

    fn trade(mint: &str, sig: &str, slot: u64, tx_index: u32) -> IlTrade {
        IlTrade {
            mint: mint.into(),
            wallet: "W".into(),
            payer: "W".into(),
            is_proxied: Some(false),
            side: Side::Buy,
            sol: 1.0,
            sol_lamports: 1_000_000_000,
            tokens: 1,
            price: 1.0,
            fee_lamports: None,
            cu_limit: None,
            cu_price: None,
            tip_lamports: None,
            payer_net_lamports: None,
            venue_fee_bps: None,
            signature: sig.into(),
            tx_index,
            leg_index: 0,
            slot,
            block_time: Utc::now(),
            received_at: Utc::now(),
            reserves: Reserves::default(),
            venue: Venue::Curve,
            instruction_type: String::new(),
            instruction_labels: Vec::new(),
            amm_swap_accounts: None,
            curve_creator: None,
            swap_ix: None,
        }
    }

    fn sigs(ts: &[IlTrade]) -> Vec<&str> {
        ts.iter().map(|t| t.signature.as_str()).collect()
    }

    #[test]
    fn a_bundled_buy_before_its_create_replays_after_the_dev_buy() {
        let mut e = EarlyTrades::default();
        e.observe_slot(100);
        assert!(!e.park(trade("M", "bundle", 100, 412)));

        // The create holds it: the dev buy (same tx as the create) comes next.
        assert!(e.on_create("M", "create", 100, true).is_empty());
        // Another mint's trade releases nothing.
        assert!(e.release_after("X", "create").is_empty());
        assert!(e.release_after("M", "other").is_empty());
        // The dev buy releases it, to be applied right after.
        assert_eq!(sigs(&e.release_after("M", "create")), ["bundle"]);
        assert!(e.release_after("M", "create").is_empty());
    }

    #[test]
    fn a_create_without_a_dev_buy_replays_at_once_in_chain_order() {
        let mut e = EarlyTrades::default();
        e.park(trade("M", "b", 100, 9));
        e.park(trade("X", "x", 100, 5));
        e.park(trade("M", "a", 100, 3));
        assert_eq!(sigs(&e.on_create("M", "create", 100, false)), ["a", "b"]);
        assert_eq!(e.parked_len(), 1, "the other mint's trade stays parked");
    }

    #[test]
    fn a_parked_dev_buy_replays_everything_at_once() {
        let mut e = EarlyTrades::default();
        e.park(trade("M", "bundle", 100, 412));
        e.park(trade("M", "create", 100, 411));
        assert_eq!(sigs(&e.on_create("M", "create", 100, true)), ["create", "bundle"]);
    }

    #[test]
    fn an_untracked_mint_expires_after_the_window() {
        let mut e = EarlyTrades::default();
        e.observe_slot(100);
        e.park(trade("M", "a", 100, 1));
        e.observe_slot(100 + EARLY_TRADE_SLOTS + 1);
        e.park(trade("X", "x", 105, 1));
        assert_eq!(e.parked_len(), 1, "the trade past the window is gone");
        assert!(e.on_create("M", "create", 100, false).is_empty());
    }

    #[test]
    fn a_held_trade_whose_dev_buy_never_comes_is_released_overdue() {
        let mut e = EarlyTrades::default();
        e.observe_slot(100);
        e.park(trade("M", "bundle", 100, 412));
        assert!(e.on_create("M", "create", 100, true).is_empty());
        assert!(e.release_overdue().is_empty(), "still inside the window");
        e.observe_slot(100 + EARLY_TRADE_SLOTS + 1);
        assert_eq!(sigs(&e.release_overdue()), ["bundle"]);
    }

    #[test]
    fn a_full_queue_evicts_the_oldest() {
        let mut e = EarlyTrades::default();
        for i in 0..EARLY_TRADE_CAP as u32 {
            assert!(!e.park(trade("X", "x", 100, i)));
        }
        assert!(e.park(trade("M", "a", 100, 0)));
        assert_eq!(e.parked_len(), EARLY_TRADE_CAP);
    }
}
