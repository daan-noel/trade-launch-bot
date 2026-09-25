//! `m_print_wallet` — facts about the WALLET behind the print being decided on, on
//! this token (static metrics).
//!
//! * `since_buy` — seconds since that wallet last BOUGHT this token, before this
//!   print. On a sell it is how long the seller held since their last buy: the
//!   hot-tape rule's "a sell from a wallet whose last buy is 30 s old or less"
//!   (evidence 1.22).
//!
//! **A print fact, never a tick fact.** The reading belongs to the print folded
//! last, and a tick clears it to `NaN`, so no rule can fire on a tick off a print
//! that already decided. `NaN` also when the wallet never bought this token.
//!
//! **Opened only when a loaded rule reads it.** The state is one wallet -> last-buy
//! map per token, which on a busy coin holds every buyer it ever had; a track opens
//! it through [`TokenTrack::ensure_print_wallet`](super::track::TokenTrack::ensure_print_wallet)
//! only when some rule names the group, so a rule set that does not pays nothing.
//! The map needs the wallet column ([`Metric::needs_wallet_identity`]).

use crate::hash::HashedMap;

use super::flow_window::is_foldable;
use super::{secs_between, Metric, Side, TradeLite, Ts};

/// One token's wallet -> last-buy map and the reading for the print folded last.
#[derive(Debug, Clone)]
pub struct PrintWalletState {
    /// When each wallet last bought this token, in fold order.
    last_buy: HashedMap<Ts>,
    /// `since_buy` for the print folded last; `NaN` after a tick.
    since_buy: f64,
}

impl Default for PrintWalletState {
    fn default() -> Self {
        Self { last_buy: HashedMap::default(), since_buy: f64::NAN }
    }
}

impl PrintWalletState {
    /// Fold one print: read the wallet's last buy BEFORE this print, then record
    /// this print if it is a buy.
    pub fn on_trade(&mut self, t: &TradeLite) {
        self.since_buy = self
            .last_buy
            .get(&t.wallet_hash)
            .map_or(f64::NAN, |&at| secs_between(at, t.at));
        if t.side == Side::Buy && is_foldable(t.sol) {
            self.last_buy.insert(t.wallet_hash, t.at);
        }
    }

    /// A tick is not a print: the reading belongs to the print folded last.
    pub fn on_tick(&mut self) {
        self.since_buy = f64::NAN;
    }

    /// Value of one `m_print_wallet` metric.
    pub fn value(&self, id: Metric) -> f64 {
        match id {
            Metric::SinceBuySec => self.since_buy,
            _ => f64::NAN,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};

    fn ts(secs: f64) -> Ts {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap() + Duration::milliseconds((secs * 1000.0) as i64)
    }

    fn print(side: Side, wallet: u64, secs: f64) -> TradeLite {
        TradeLite { side, sol: 1.0, wallet_hash: wallet, at: ts(secs), ..TradeLite::default() }
    }

    #[test]
    fn a_sell_reads_the_time_since_that_wallets_last_buy() {
        let mut s = PrintWalletState::default();
        s.on_trade(&print(Side::Buy, 7, 1.0));
        s.on_trade(&print(Side::Buy, 8, 2.0));
        s.on_trade(&print(Side::Buy, 7, 4.5));
        s.on_trade(&print(Side::Sell, 7, 30.0));
        assert_eq!(s.value(Metric::SinceBuySec), 25.5, "the LAST buy, not the first");
        // A sell never moves the clock; the next sell reads the same buy.
        s.on_trade(&print(Side::Sell, 7, 40.0));
        assert_eq!(s.value(Metric::SinceBuySec), 35.5);
    }

    #[test]
    fn a_buy_reads_the_buy_before_it() {
        let mut s = PrintWalletState::default();
        s.on_trade(&print(Side::Buy, 7, 1.0));
        assert!(s.value(Metric::SinceBuySec).is_nan(), "no earlier buy");
        s.on_trade(&print(Side::Buy, 7, 3.0));
        assert_eq!(s.value(Metric::SinceBuySec), 2.0);
    }

    #[test]
    fn a_wallet_that_never_bought_and_a_tick_read_nan() {
        let mut s = PrintWalletState::default();
        s.on_trade(&print(Side::Buy, 7, 1.0));
        s.on_trade(&print(Side::Sell, 9, 2.0));
        assert!(s.value(Metric::SinceBuySec).is_nan(), "wallet 9 never bought");
        s.on_trade(&print(Side::Sell, 7, 3.0));
        assert_eq!(s.value(Metric::SinceBuySec), 2.0);
        s.on_tick();
        assert!(s.value(Metric::SinceBuySec).is_nan(), "a tick is not a print");
    }
}
