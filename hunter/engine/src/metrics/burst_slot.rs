//! `m_burst_slot` — this token, this slot so far, this print's build template.
//!
//! Static (no window). Fingerprint-scoped: the working-template **list** lives on
//! the fingerprint so the group stays reusable. One slot prefix on the token,
//! reset when `slot` changes. Unconfigured fingerprints read `NaN`.
//!
//! See `hunter/docs/plans/strategies/ix-live-rule.md`.

use serde_json::Value;

use crate::hash::{HashedMap, HashedSet};

use super::template_grain::grain_id_hash;
use super::{MetricId, Side, TradeLite};

/// The config key this group reads, inside `fingerprints.metric_config`.
pub const CONFIG_KEY: &str = "m_burst_slot";

// ── Patterns ─────────────────────────────────────────────────────────────────

/// Compiled working-template list for one fingerprint
/// (`m_burst_slot.working_templates`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BurstPatterns {
    hashes: HashedSet,
}

impl BurstPatterns {
    pub fn new(hashes: HashedSet) -> Self {
        Self { hashes }
    }

    /// Parse `metric_config["m_burst_slot"]`. `None` = key absent or unusable ⇒
    /// the group is unconfigured and every metric reads `NaN`.
    pub fn from_metric_config(cfg: &Value) -> Option<Self> {
        let obj = cfg.get(CONFIG_KEY)?;
        if !obj.is_object() {
            return None;
        }
        let arr = obj.get("working_templates")?.as_array()?;
        let mut hashes = HashedSet::default();
        for row in arr {
            let id = row.as_str()?;
            if !id.is_empty() {
                hashes.insert(grain_id_hash(id));
            }
        }
        Some(Self { hashes })
    }

    pub fn validate_metric_config(cfg: &Value) -> Result<(), String> {
        let Some(obj) = cfg.get(CONFIG_KEY) else {
            return Ok(());
        };
        let Some(map) = obj.as_object() else {
            return Err(format!("{CONFIG_KEY} must be an object"));
        };
        let Some(arr) = map.get("working_templates") else {
            return Err(format!("{CONFIG_KEY} carries no working_templates"));
        };
        let Some(rows) = arr.as_array() else {
            return Err(format!(
                "{CONFIG_KEY}.working_templates must be an array of grain-id strings"
            ));
        };
        for row in rows {
            if !row.is_string() {
                return Err(format!(
                    "{CONFIG_KEY}.working_templates entry must be a string"
                ));
            }
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.hashes.is_empty()
    }

    fn contains(&self, hash: u64) -> bool {
        self.hashes.contains(&hash)
    }
}

// ── Per-template running totals this slot ────────────────────────────────────

#[derive(Debug, Clone, Default)]
struct TemplateRun {
    count: u32,
    sol: f64,
    wallets: HashedSet,
    new_wallets: HashedSet,
}

// ── Slot prefix ──────────────────────────────────────────────────────────────

/// One token's current-slot buy prefix + the ever-seen buyer set.
#[derive(Debug, Clone)]
pub struct BurstSlotState {
    slot: u64,
    buy_count: u32,
    buy_sol: f64,
    wallets: HashedSet,
    templates: HashedSet,
    by_template: HashedMap<TemplateRun>,
    /// Min tx_index among buys this slot; `None` until a buy with a known index.
    first_tx: Option<u32>,
    /// Last buy's tx_index this slot (packed reads this, not a sell's index).
    last_buy_tx: Option<u32>,
    /// Any buy this slot arrived without `tx_index` ⇒ `packed` is NaN.
    missing_tx: bool,
    /// Wallets that have bought this mint in a *previous* slot.
    ever: HashedSet,
    /// Wallets that bought this mint in the current slot (moved into `ever` on
    /// the next slot change).
    this_slot_buyers: HashedSet,
    pre_slot_liquidity: f64,
    pre_print_trail: f64,
    this_template: Option<u64>,
}

impl Default for BurstSlotState {
    fn default() -> Self {
        Self {
            slot: 0,
            buy_count: 0,
            buy_sol: 0.0,
            wallets: HashedSet::default(),
            templates: HashedSet::default(),
            by_template: HashedMap::default(),
            first_tx: None,
            last_buy_tx: None,
            missing_tx: false,
            ever: HashedSet::default(),
            this_slot_buyers: HashedSet::default(),
            pre_slot_liquidity: f64::NAN,
            pre_print_trail: f64::NAN,
            this_template: None,
        }
    }
}

impl BurstSlotState {
    fn reset_prefix(&mut self, new_slot: u64, pre_liq: f64) {
        for w in self.this_slot_buyers.drain() {
            self.ever.insert(w);
        }
        self.slot = new_slot;
        self.buy_count = 0;
        self.buy_sol = 0.0;
        self.wallets.clear();
        self.templates.clear();
        self.by_template.clear();
        self.first_tx = None;
        self.last_buy_tx = None;
        self.missing_tx = false;
        self.pre_slot_liquidity = pre_liq;
    }

    /// Snapshot trail, maybe roll the slot, then fold this print. `prev_liquidity`
    /// is the real reserve **before** this trade (last print of the previous slot
    /// when the slot just changed).
    pub fn on_trade(&mut self, t: &TradeLite, pre_trail: f64, prev_liquidity: f64) {
        self.pre_print_trail = pre_trail;
        self.this_template = t.template_hash;

        if t.slot != 0 && t.slot != self.slot {
            self.reset_prefix(t.slot, prev_liquidity);
        }

        if t.side != Side::Buy {
            return;
        }

        self.buy_count = self.buy_count.saturating_add(1);
        self.buy_sol += t.sol;
        self.wallets.insert(t.wallet_hash);
        self.this_slot_buyers.insert(t.wallet_hash);

        let is_new = !self.ever.contains(&t.wallet_hash);

        match t.tx_index {
            None => self.missing_tx = true,
            Some(idx) => {
                if self.first_tx.is_none() {
                    self.first_tx = Some(idx);
                }
                self.last_buy_tx = Some(idx);
            }
        }

        if let Some(h) = t.template_hash {
            self.templates.insert(h);
            let run = self.by_template.entry(h).or_default();
            run.count = run.count.saturating_add(1);
            run.sol += t.sol;
            run.wallets.insert(t.wallet_hash);
            if is_new {
                run.new_wallets.insert(t.wallet_hash);
            }
        }
    }

    fn packed(&self) -> f64 {
        if self.missing_tx {
            return f64::NAN;
        }
        match (self.first_tx, self.last_buy_tx) {
            (Some(first), Some(last)) if self.buy_count > 0 => {
                f64::from(u8::from(
                    last.saturating_sub(first).saturating_add(1) == self.buy_count,
                ))
            }
            _ => f64::NAN,
        }
    }

    fn this_run(&self) -> Option<&TemplateRun> {
        self.this_template.and_then(|h| self.by_template.get(&h))
    }

    /// Read one metric. `patterns` is this fingerprint's working list; `None` ⇒
    /// unconfigured ⇒ NaN.
    pub fn value(&self, id: MetricId, patterns: Option<&BurstPatterns>) -> f64 {
        let Some(p) = patterns else {
            return f64::NAN;
        };
        use MetricId::*;
        match id {
            WorkingTemplate => match self.this_template {
                Some(h) => f64::from(u8::from(p.contains(h))),
                None => 0.0,
            },
            TemplateBuyCount => self.this_run().map(|r| f64::from(r.count)).unwrap_or(f64::NAN),
            TemplateBuySol => self.this_run().map(|r| r.sol).unwrap_or(f64::NAN),
            TemplateWalletCount => {
                self.this_run().map(|r| r.wallets.len() as f64).unwrap_or(f64::NAN)
            }
            SlotBuyCount => f64::from(self.buy_count),
            SlotBuySol => self.buy_sol,
            SlotWalletCount => self.wallets.len() as f64,
            SlotTemplateCount => self.templates.len() as f64,
            NewOnMintWallets => self
                .this_run()
                .map(|r| r.new_wallets.len() as f64)
                .unwrap_or(f64::NAN),
            Packed => self.packed(),
            PreSlotLiquidity => self.pre_slot_liquidity,
            PrePrintTrail => self.pre_print_trail,
            _ => f64::NAN,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::{Side, TradeLite};
    use chrono::{TimeZone, Utc};

    fn ts(secs: i64) -> super::super::Ts {
        Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap()
    }

    fn buy(slot: u64, tx: Option<u32>, wallet: u64, tmpl: Option<u64>, sol: f64) -> TradeLite {
        TradeLite {
            side: Side::Buy,
            sol,
            price: 1.0,
            reserve_sol: 10.0,
            priced_reserve_sol: 40.0,
            at: ts(slot as i64),
            slot,
            tx_index: tx,
            template_hash: tmpl,
            wallet_hash: wallet,
            ..Default::default()
        }
    }

    fn patterns(ids: &[&str]) -> BurstPatterns {
        let mut h = HashedSet::default();
        for id in ids {
            h.insert(grain_id_hash(id));
        }
        BurstPatterns::new(h)
    }

    #[test]
    fn packed_consecutive_vs_hole_and_nan_on_missing() {
        let p = patterns(&["Axiom Trade|CU|ATA|F"]);
        let mut s = BurstSlotState::default();
        let h = grain_id_hash("Axiom Trade|CU|ATA|F");

        s.on_trade(&buy(10, Some(5), 1, Some(h), 1.0), 0.0, f64::NAN);
        s.on_trade(&buy(10, Some(6), 2, Some(h), 1.0), 0.0, 10.0);
        s.on_trade(&buy(10, Some(7), 3, Some(h), 1.0), 0.0, 10.0);
        assert_eq!(s.value(MetricId::Packed, Some(&p)), 1.0);

        let mut hole = BurstSlotState::default();
        hole.on_trade(&buy(10, Some(5), 1, Some(h), 1.0), 0.0, f64::NAN);
        hole.on_trade(&buy(10, Some(7), 2, Some(h), 1.0), 0.0, 10.0);
        assert_eq!(hole.value(MetricId::Packed, Some(&p)), 0.0);

        let mut miss = BurstSlotState::default();
        miss.on_trade(&buy(10, Some(5), 1, Some(h), 1.0), 0.0, f64::NAN);
        miss.on_trade(&buy(10, None, 2, Some(h), 1.0), 0.0, 10.0);
        assert!(miss.value(MetricId::Packed, Some(&p)).is_nan());
    }

    #[test]
    fn tx_index_zero_is_a_valid_first() {
        let p = patterns(&["Axiom Trade|CU|ATA|F"]);
        let h = grain_id_hash("Axiom Trade|CU|ATA|F");
        let mut s = BurstSlotState::default();
        s.on_trade(&buy(10, Some(0), 1, Some(h), 1.0), 0.0, f64::NAN);
        s.on_trade(&buy(10, Some(1), 2, Some(h), 1.0), 0.0, 10.0);
        assert_eq!(s.value(MetricId::Packed, Some(&p)), 1.0);
        assert_eq!(s.value(MetricId::SlotBuyCount, Some(&p)), 2.0);
    }

    #[test]
    fn new_on_mint_and_slot_change() {
        let p = patterns(&["A|CU|F"]);
        let h = grain_id_hash("A|CU|F");
        let mut s = BurstSlotState::default();
        s.on_trade(&buy(10, Some(1), 7, Some(h), 0.5), 15.0, f64::NAN);
        assert_eq!(s.value(MetricId::NewOnMintWallets, Some(&p)), 1.0);
        assert_eq!(s.value(MetricId::PrePrintTrail, Some(&p)), 15.0);
        assert!(s.value(MetricId::PreSlotLiquidity, Some(&p)).is_nan());

        s.on_trade(&buy(11, Some(1), 7, Some(h), 0.5), 20.0, 12.0);
        // Same wallet, now a repeat — first-on-mint was slot 10.
        assert_eq!(s.value(MetricId::NewOnMintWallets, Some(&p)), 0.0);
        assert_eq!(s.value(MetricId::PreSlotLiquidity, Some(&p)), 12.0);
        assert_eq!(s.value(MetricId::SlotBuyCount, Some(&p)), 1.0);

        s.on_trade(&buy(11, Some(2), 8, Some(h), 0.4), 20.0, 12.0);
        assert_eq!(s.value(MetricId::NewOnMintWallets, Some(&p)), 1.0);
        assert_eq!(s.value(MetricId::TemplateWalletCount, Some(&p)), 2.0);
        assert_eq!(s.value(MetricId::TemplateBuySol, Some(&p)), 0.9);
    }

    #[test]
    fn unconfigured_is_nan_not_zero() {
        let mut s = BurstSlotState::default();
        s.on_trade(&buy(10, Some(1), 1, Some(1), 1.0), 0.0, f64::NAN);
        assert!(s.value(MetricId::SlotBuyCount, None).is_nan());
        assert!(s.value(MetricId::WorkingTemplate, None).is_nan());
    }

    #[test]
    fn working_template_is_this_prints_grain() {
        let p = patterns(&["Axiom Trade|CU|ATA|F"]);
        let work = grain_id_hash("Axiom Trade|CU|ATA|F");
        let dead = grain_id_hash("Axiom Trade|CU|F");
        let mut s = BurstSlotState::default();
        s.on_trade(&buy(10, Some(1), 1, Some(work), 1.0), 0.0, f64::NAN);
        assert_eq!(s.value(MetricId::WorkingTemplate, Some(&p)), 1.0);
        s.on_trade(&buy(10, Some(2), 2, Some(dead), 1.0), 0.0, 10.0);
        assert_eq!(s.value(MetricId::WorkingTemplate, Some(&p)), 0.0);
        assert_eq!(s.value(MetricId::SlotTemplateCount, Some(&p)), 2.0);
    }
}
