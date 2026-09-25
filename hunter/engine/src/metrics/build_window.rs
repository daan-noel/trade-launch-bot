//! `m_build_window` — trailing-window BUILD RECIPE counts (dynamic metrics).
//!
//! * `unique_builds` — distinct build recipes ([`TradeLite::build_hash`]) among the
//!   prints in the window, the print being read included.
//!
//! A recipe is a transaction's ordered instruction labels without account setup,
//! teardown and memos ([`build_hash`](super::trade_keys::build_hash)), so it names the
//! tool or bot that built the transaction rather than the wallet that signed it.
//! Many recipes printing at once is many independent machines reacting to the same
//! tape: the hot-tape rule's "15 or more recipes in 5 s" (evidence 1.22).
//!
//! **Its own buffer, for the reason `m_crowd_window` has one:** a group's buffer is a
//! load obligation, and this one's is the `ix_labels` column. The distinct-count
//! mechanism is shared ([`DistinctWindow`]); the state is not.
//!
//! A print whose labels are missing has no recipe and adds nothing. The admission
//! guard is otherwise `m_flow_window`'s ([`is_foldable`]), so a print this window
//! counts is a print every other window counts.

use super::distinct_window::DistinctWindow;
use super::flow_window::is_foldable;
use super::{Metric, WindowSpec};

/// One trailing-window recipe counter for a single [`WindowSpec`].
#[derive(Debug, Clone)]
pub struct BuildWindowState {
    win: DistinctWindow,
}

impl BuildWindowState {
    pub fn new(spec: WindowSpec) -> Self {
        Self { win: DistinctWindow::new(spec) }
    }

    /// The span this counter tracks.
    pub fn spec(&self) -> WindowSpec {
        self.win.spec()
    }

    /// Fold one print at `pos`, then drop anything that fell out of the window as of
    /// `now_pos`. A print with no recipe still evicts: the window's low end moves
    /// with time, not with what the print carries.
    pub fn on_trade(&mut self, sol: f64, build: Option<u64>, pos: i64, now_pos: i64) {
        match build {
            Some(b) if is_foldable(sol) => self.win.push(b, pos, now_pos),
            _ => self.win.evict(now_pos),
        }
    }

    /// Drop entries that fell off the low end of the window as of `now_pos`.
    pub fn evict(&mut self, now_pos: i64) {
        self.win.evict(now_pos);
    }

    /// Value of one `m_build_window` metric over the window at `now_pos`.
    pub fn value(&self, id: Metric, now_pos: i64) -> f64 {
        match id {
            Metric::UniqueIxShapes => self.win.distinct(now_pos),
            _ => f64::NAN,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::Ts;
    use chrono::{Duration, TimeZone, Utc};

    fn p(secs: f64) -> i64 {
        let t: Ts = Utc.timestamp_opt(1_700_000_000, 0).unwrap()
            + Duration::milliseconds((secs * 1000.0) as i64);
        t.timestamp_millis()
    }

    /// The window is closed at both ends and holds the print being read: a recipe
    /// exactly `size` seconds old still counts, one a millisecond older does not.
    #[test]
    fn counts_distinct_recipes_in_the_closed_window_including_the_print() {
        let mut w = BuildWindowState::new(WindowSpec::secs(5.0));
        w.on_trade(0.1, Some(1), p(0.0), p(0.0));
        w.on_trade(0.1, Some(2), p(1.0), p(1.0));
        w.on_trade(0.1, Some(1), p(2.0), p(2.0));
        assert_eq!(w.value(Metric::UniqueIxShapes, p(2.0)), 2.0);
        // At t=5.0 the t=0 print sits exactly on the low bound: still in.
        w.on_trade(0.1, Some(3), p(5.0), p(5.0));
        assert_eq!(w.value(Metric::UniqueIxShapes, p(5.0)), 3.0, "the print read is counted");
        // At t=5.001 recipe 1 is still held by its t=2 print; nothing is lost yet.
        assert_eq!(w.value(Metric::UniqueIxShapes, p(5.001)), 3.0);
        // At t=6.001 recipe 2 (t=1) is out.
        w.evict(p(6.001));
        assert_eq!(w.value(Metric::UniqueIxShapes, p(6.001)), 2.0);
    }

    /// A print without labels has no recipe: it adds nothing, but time still moves.
    #[test]
    fn a_print_with_no_recipe_adds_nothing_and_still_evicts() {
        let mut w = BuildWindowState::new(WindowSpec::secs(5.0));
        w.on_trade(0.1, Some(9), p(0.0), p(0.0));
        w.on_trade(0.1, None, p(10.0), p(10.0));
        assert_eq!(w.value(Metric::UniqueIxShapes, p(10.0)), 0.0);
    }
}
