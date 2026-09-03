//! `m_burst_slot` — this token, this slot so far, this print's build template.
//!
//! Static (no window). Fingerprint-scoped: the working-template **list** lives on
//! the fingerprint so the group stays reusable. One slot prefix on the token,
//! reset when `slot` changes. Unconfigured fingerprints read `NaN`.
//!
//! A **member** is a curve buy with a template grain, not a launch create, that
//! joins the current-slot prefix. `member_template_count` is distinct grains on
//! the WHOLE prefix (SQL `run_ntmpl`); `working_*` counts only members whose
//! grain is on the fingerprint list; `same_*` only those sharing this print's
//! grain. `working_buy_share` is the working count over the whole prefix, so
//! 100 is a PURE pack - and a pure pack makes the working family and the whole
//! prefix read the same number. The 5-slot buy quiet the rule also needs is NOT here: it is
//! `m_flow_window.buy_count == 0` on a lagged slot window (`4sl@1`).
//! See `hunter/docs/plans/strategies/ix-live-rule.md`.

use serde_json::Value;

use crate::hash::{HashedMap, HashedSet};

use super::template_grain::{grain_id_hash, program_id_hash};
use super::{MetricId, Side, TradeLite};

/// The config key this group reads, inside `fingerprints.metric_config`.
pub const CONFIG_KEY: &str = "m_burst_slot";

// ── Patterns ─────────────────────────────────────────────────────────────────

/// Compiled working-template and/or working-program lists for one fingerprint
/// (`m_burst_slot.working_templates` / `working_programs`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BurstPatterns {
    hashes: HashedSet,
    programs: HashedSet,
}

impl BurstPatterns {
    pub fn new(hashes: HashedSet) -> Self {
        Self {
            hashes,
            programs: HashedSet::default(),
        }
    }

    pub fn with_programs(mut self, programs: HashedSet) -> Self {
        self.programs = programs;
        self
    }

    /// Parse `metric_config["m_burst_slot"]`. `None` = key absent or both lists
    /// empty ⇒ the group is unconfigured and every metric reads `NaN`.
    pub fn from_metric_config(cfg: &Value) -> Option<Self> {
        let obj = cfg.get(CONFIG_KEY)?;
        if !obj.is_object() {
            return None;
        }
        let mut hashes = HashedSet::default();
        if let Some(arr) = obj.get("working_templates").and_then(|v| v.as_array()) {
            for row in arr {
                let id = row.as_str()?;
                if !id.is_empty() {
                    hashes.insert(grain_id_hash(id));
                }
            }
        }
        let mut programs = HashedSet::default();
        if let Some(arr) = obj.get("working_programs").and_then(|v| v.as_array()) {
            for row in arr {
                let id = row.as_str()?;
                if !id.is_empty() {
                    programs.insert(program_id_hash(id));
                }
            }
        }
        if hashes.is_empty() && programs.is_empty() {
            return None;
        }
        Some(Self { hashes, programs })
    }

    pub fn validate_metric_config(cfg: &Value) -> Result<(), String> {
        let Some(obj) = cfg.get(CONFIG_KEY) else {
            return Ok(());
        };
        let Some(map) = obj.as_object() else {
            return Err(format!("{CONFIG_KEY} must be an object"));
        };
        let has_templates = map.contains_key("working_templates");
        let has_programs = map.contains_key("working_programs");
        if !has_templates && !has_programs {
            return Err(format!(
                "{CONFIG_KEY} carries no working_templates or working_programs"
            ));
        }
        if has_templates {
            let Some(rows) = map.get("working_templates").and_then(|v| v.as_array()) else {
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
        }
        if has_programs {
            let Some(rows) = map.get("working_programs").and_then(|v| v.as_array()) else {
                return Err(format!(
                    "{CONFIG_KEY}.working_programs must be an array of program-name strings"
                ));
            };
            for row in rows {
                if !row.is_string() {
                    return Err(format!(
                        "{CONFIG_KEY}.working_programs entry must be a string"
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.hashes.is_empty() && self.programs.is_empty()
    }

    pub(crate) fn contains(&self, hash: u64) -> bool {
        self.hashes.contains(&hash)
    }

    /// Grain on `working_templates` or program on `working_programs`.
    pub(crate) fn matches(&self, grain: Option<u64>, program: Option<u64>) -> bool {
        grain.is_some_and(|h| self.hashes.contains(&h))
            || program.is_some_and(|h| self.programs.contains(&h))
    }
}

// ── Per-template running totals this slot ────────────────────────────────────

#[derive(Debug, Clone, Default)]
struct TemplateRun {
    count: u32,
    sol: f64,
    wallets: HashedSet,
    /// True when any wallet in this run is first-on-mint this slot.
    has_new: bool,
}

// ── Slot prefix ──────────────────────────────────────────────────────────────

/// One token's current-slot member prefix + the ever-seen buyer set
/// and the mint-lifetime template-grain set (`working_templates_seen`).
#[derive(Debug, Clone)]
pub struct BurstSlotState {
    slot: u64,
    member_count: u32,
    /// Min tx_index among members this slot; `None` until a member with a known index.
    first_tx: Option<u32>,
    /// Last member's tx_index this slot (packed reads this, not a sell's index).
    last_member_tx: Option<u32>,
    /// Any member this slot arrived without `tx_index` ⇒ `packed` is NaN.
    missing_tx: bool,
    has_unknown: bool,
    /// Wallets that have bought this mint in a *previous* slot.
    ever: HashedSet,
    /// Wallets that bought this mint in the current slot (moved into `ever` on
    /// the next slot change). Updated on every buy with a wallet, members or not.
    this_slot_buyers: HashedSet,
    /// Template grains on curve buys this mint, surviving slot reset.
    seen_templates: HashedSet,
    by_template: HashedMap<TemplateRun>,
    pre_slot_liquidity: f64,
    pre_print_trail: f64,
    this_template: Option<u64>,
    this_member: bool,
}

impl Default for BurstSlotState {
    fn default() -> Self {
        Self {
            slot: 0,
            member_count: 0,
            first_tx: None,
            last_member_tx: None,
            missing_tx: false,
            has_unknown: false,
            ever: HashedSet::default(),
            this_slot_buyers: HashedSet::default(),
            seen_templates: HashedSet::default(),
            by_template: HashedMap::default(),
            pre_slot_liquidity: f64::NAN,
            pre_print_trail: f64::NAN,
            this_template: None,
            this_member: false,
        }
    }
}

impl BurstSlotState {
    fn reset_prefix(&mut self, new_slot: u64, pre_liq: f64) {
        for w in self.this_slot_buyers.drain() {
            self.ever.insert(w);
        }
        self.slot = new_slot;
        self.member_count = 0;
        self.first_tx = None;
        self.last_member_tx = None;
        self.missing_tx = false;
        self.has_unknown = false;
        self.by_template.clear();
        self.pre_slot_liquidity = pre_liq;
    }

    /// A tick is not a print — `this_member` must not survive into a later
    /// `can_enter` on a clock advance.
    pub fn on_tick(&mut self) {
        self.this_member = false;
    }

    /// Snapshot trail, maybe roll the slot, then fold this print. `prev_liquidity`
    /// is the real reserve **before** this trade (last print of the previous slot
    /// when the slot just changed).
    pub fn on_trade(&mut self, t: &TradeLite, pre_trail: f64, prev_liquidity: f64) {
        self.pre_print_trail = pre_trail;
        self.this_template = t.template_hash;
        self.this_member = false;

        if t.slot != 0 && t.slot != self.slot {
            self.reset_prefix(t.slot, prev_liquidity);
        }

        if t.side != Side::Buy {
            return;
        }

        // First-on-mint: every buy with a wallet, including non-members
        // (launch, AMM, organic). Decide BEFORE inserting this print, or the
        // current wallet would never count as new. Wallet 0 is unknown, not an
        // identity.
        let is_new = t.wallet_hash != 0
            && !self.ever.contains(&t.wallet_hash)
            && !self.this_slot_buyers.contains(&t.wallet_hash);
        if t.wallet_hash != 0 {
            self.this_slot_buyers.insert(t.wallet_hash);
        }

        // Lifetime working-templates-seen: every curve buy with a grain, including
        // this print. Slot prefix still resets; this set does not.
        if t.on_curve {
            if let Some(h) = t.template_hash {
                self.seen_templates.insert(h);
            }
        }

        if !is_member(t) {
            return;
        }

        self.this_member = true;
        self.member_count = self.member_count.saturating_add(1);
        if t.wallet_hash == 0 {
            self.has_unknown = true;
        }

        match t.tx_index {
            None => self.missing_tx = true,
            Some(idx) => {
                if self.first_tx.is_none() {
                    self.first_tx = Some(idx);
                }
                self.last_member_tx = Some(idx);
            }
        }

        let Some(h) = t.template_hash else {
            return;
        };
        let run = self.by_template.entry(h).or_default();
        run.count = run.count.saturating_add(1);
        run.sol += t.sol;
        if t.wallet_hash != 0 {
            run.wallets.insert(t.wallet_hash);
        }
        run.has_new |= is_new;
    }

    fn packed(&self) -> f64 {
        if self.missing_tx {
            return f64::NAN;
        }
        match (self.first_tx, self.last_member_tx) {
            (Some(first), Some(last)) if self.member_count > 0 => {
                f64::from(u8::from(
                    last.saturating_sub(first).saturating_add(1) == self.member_count,
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
            ThisMember => f64::from(u8::from(self.this_member)),
            ThisWorking => match self.this_template {
                Some(h) => f64::from(u8::from(p.contains(h))),
                None => 0.0,
            },
            SameBuyCount => self.this_run().map(|r| f64::from(r.count)).unwrap_or(f64::NAN),
            SameBuySol => self.this_run().map(|r| r.sol).unwrap_or(f64::NAN),
            SameWalletCount => {
                self.this_run().map(|r| r.wallets.len() as f64).unwrap_or(f64::NAN)
            }
            MemberTemplateCount => {
                if self.member_count == 0 {
                    f64::NAN
                } else {
                    self.by_template.len() as f64
                }
            }
            WorkingBuyCount => self.working_count(p),
            WorkingBuySol => self.working_sol(p),
            WorkingWalletCount => self.working_wallets(p),
            WorkingTemplateCount => self.working_template_count(p),
            WorkingTemplatesSeen => self.working_templates_seen(p),
            WorkingBuyShare => {
                if self.member_count == 0 {
                    f64::NAN
                } else {
                    100.0 * self.working_count(p) / f64::from(self.member_count)
                }
            }
            HasNew => f64::from(u8::from(self.has_new(p))),
            HasUnknown => f64::from(u8::from(self.has_unknown)),
            Packed => self.packed(),
            PreSlotLiquidity => self.pre_slot_liquidity,
            PrePrintTrail => self.pre_print_trail,
            _ => f64::NAN,
        }
    }

    fn working_count(&self, p: &BurstPatterns) -> f64 {
        let mut n = 0u32;
        for (h, run) in &self.by_template {
            if p.contains(*h) {
                n = n.saturating_add(run.count);
            }
        }
        f64::from(n)
    }

    fn working_sol(&self, p: &BurstPatterns) -> f64 {
        let mut s = 0.0;
        for (h, run) in &self.by_template {
            if p.contains(*h) {
                s += run.sol;
            }
        }
        s
    }

    fn working_wallets(&self, p: &BurstPatterns) -> f64 {
        let mut w = HashedSet::default();
        for (h, run) in &self.by_template {
            if p.contains(*h) {
                w.extend(run.wallets.iter().copied());
            }
        }
        w.len() as f64
    }

    fn working_template_count(&self, p: &BurstPatterns) -> f64 {
        self.by_template
            .keys()
            .filter(|h| p.contains(**h))
            .count() as f64
    }

    fn working_templates_seen(&self, p: &BurstPatterns) -> f64 {
        self.seen_templates
            .iter()
            .filter(|h| p.contains(**h))
            .count() as f64
    }

    fn has_new(&self, p: &BurstPatterns) -> bool {
        self.by_template.iter().any(|(h, run)| p.contains(*h) && run.has_new)
    }
}

pub(crate) fn is_member(t: &TradeLite) -> bool {
    t.side == Side::Buy && t.on_curve && !t.is_launch && t.template_hash.is_some()
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
            on_curve: true,
            is_launch: false,
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
        assert_eq!(s.value(MetricId::SameBuyCount, Some(&p)), 2.0);
    }

    #[test]
    fn has_new_and_slot_change() {
        let p = patterns(&["A|CU|F"]);
        let h = grain_id_hash("A|CU|F");
        let mut s = BurstSlotState::default();
        s.on_trade(&buy(10, Some(1), 7, Some(h), 0.5), 15.0, f64::NAN);
        assert_eq!(s.value(MetricId::HasNew, Some(&p)), 1.0);
        assert_eq!(s.value(MetricId::PrePrintTrail, Some(&p)), 15.0);
        assert!(s.value(MetricId::PreSlotLiquidity, Some(&p)).is_nan());

        s.on_trade(&buy(11, Some(1), 7, Some(h), 0.5), 20.0, 12.0);
        // Same wallet, now a repeat — first-on-mint was slot 10.
        assert_eq!(s.value(MetricId::HasNew, Some(&p)), 0.0);
        assert_eq!(s.value(MetricId::PreSlotLiquidity, Some(&p)), 12.0);
        assert_eq!(s.value(MetricId::SameBuyCount, Some(&p)), 1.0);

        s.on_trade(&buy(11, Some(2), 8, Some(h), 0.4), 20.0, 12.0);
        assert_eq!(s.value(MetricId::HasNew, Some(&p)), 1.0);
        assert_eq!(s.value(MetricId::SameWalletCount, Some(&p)), 2.0);
        assert_eq!(s.value(MetricId::SameBuySol, Some(&p)), 0.9);
    }

    #[test]
    fn working_templates_seen_survives_slot_reset() {
        let p = patterns(&[
            "Axiom Trade|CU|ATA|F",
            "Photon|CU|ATA|F",
            "Terminal|CU|ATA|F",
            "GMGN|CU|ATA|F",
        ]);
        let ax = grain_id_hash("Axiom Trade|CU|ATA|F");
        let ph = grain_id_hash("Photon|CU|ATA|F");
        let te = grain_id_hash("Terminal|CU|ATA|F");
        let gm = grain_id_hash("GMGN|CU|ATA|F");
        let mut s = BurstSlotState::default();
        s.on_trade(&buy(10, Some(1), 1, Some(ax), 0.5), 0.0, f64::NAN);
        s.on_trade(&buy(11, Some(1), 2, Some(ph), 0.5), 0.0, 10.0);
        s.on_trade(&buy(12, Some(1), 3, Some(te), 0.5), 0.0, 10.0);
        assert_eq!(s.value(MetricId::WorkingTemplatesSeen, Some(&p)), 3.0);
        assert_eq!(s.value(MetricId::WorkingTemplateCount, Some(&p)), 1.0);
        s.on_trade(&buy(13, Some(1), 4, Some(gm), 0.5), 0.0, 10.0);
        assert_eq!(s.value(MetricId::WorkingTemplatesSeen, Some(&p)), 4.0);
        assert_eq!(s.value(MetricId::WorkingTemplateCount, Some(&p)), 1.0);
    }

    #[test]
    fn unconfigured_is_nan_not_zero() {
        let mut s = BurstSlotState::default();
        s.on_trade(&buy(10, Some(1), 1, Some(1), 1.0), 0.0, f64::NAN);
        assert!(s.value(MetricId::SameBuyCount, None).is_nan());
        assert!(s.value(MetricId::ThisWorking, None).is_nan());
    }

    #[test]
    fn this_working_is_this_prints_grain() {
        let p = patterns(&["Axiom Trade|CU|ATA|F"]);
        let work = grain_id_hash("Axiom Trade|CU|ATA|F");
        let dead = grain_id_hash("Pump.Fun");
        let mut s = BurstSlotState::default();
        s.on_trade(&buy(10, Some(1), 1, Some(work), 1.0), 0.0, f64::NAN);
        assert_eq!(s.value(MetricId::ThisWorking, Some(&p)), 1.0);
        assert_eq!(s.value(MetricId::ThisMember, Some(&p)), 1.0);
        s.on_trade(&buy(10, Some(2), 2, Some(dead), 1.0), 0.0, 10.0);
        assert_eq!(s.value(MetricId::ThisWorking, Some(&p)), 0.0);
        assert_eq!(s.value(MetricId::ThisMember, Some(&p)), 1.0);
        // Organic Pump.Fun is a member but not working — mixed size ignores it.
        assert_eq!(s.value(MetricId::WorkingTemplateCount, Some(&p)), 1.0);
        assert_eq!(s.value(MetricId::MemberTemplateCount, Some(&p)), 2.0);
        assert_eq!(s.value(MetricId::WorkingBuySol, Some(&p)), 1.0);
        assert_eq!(s.value(MetricId::SameBuySol, Some(&p)), 1.0); // this print's grain = Pump.Fun
    }

    #[test]
    fn launch_and_amm_do_not_join_the_prefix() {
        let p = patterns(&["Axiom Trade|CU|ATA|F"]);
        let h = grain_id_hash("Axiom Trade|CU|ATA|F");
        let mut s = BurstSlotState::default();
        let mut launch = buy(10, Some(1), 1, Some(h), 2.0);
        launch.is_launch = true;
        s.on_trade(&launch, 0.0, f64::NAN);
        assert_eq!(s.value(MetricId::ThisMember, Some(&p)), 0.0);
        assert_eq!(s.value(MetricId::WorkingBuyCount, Some(&p)), 0.0);

        let mut amm = buy(10, Some(2), 2, Some(h), 2.0);
        amm.on_curve = false;
        s.on_trade(&amm, 0.0, 10.0);
        assert_eq!(s.value(MetricId::ThisMember, Some(&p)), 0.0);
        assert_eq!(s.value(MetricId::WorkingBuyCount, Some(&p)), 0.0);

        s.on_trade(&buy(10, Some(3), 3, Some(h), 1.0), 0.0, 10.0);
        assert_eq!(s.value(MetricId::ThisMember, Some(&p)), 1.0);
        assert_eq!(s.value(MetricId::WorkingBuyCount, Some(&p)), 1.0);
        // Launch/AMM wallets still mark ever — next slot they are repeats.
        s.on_trade(&buy(11, Some(1), 1, Some(h), 1.0), 0.0, 10.0);
        assert_eq!(s.value(MetricId::HasNew, Some(&p)), 0.0);
    }

    #[test]
    fn same_slot_prior_buy_is_not_new() {
        let p = patterns(&["Axiom Trade|CU|ATA|F"]);
        let h = grain_id_hash("Axiom Trade|CU|ATA|F");
        let mut s = BurstSlotState::default();
        let mut launch = buy(10, Some(1), 1, Some(h), 2.0);
        launch.is_launch = true;
        s.on_trade(&launch, 0.0, f64::NAN);
        s.on_trade(&buy(10, Some(2), 1, Some(h), 1.0), 0.0, 10.0);
        assert_eq!(s.value(MetricId::HasNew, Some(&p)), 0.0);
        s.on_trade(&buy(10, Some(3), 2, Some(h), 1.0), 0.0, 10.0);
        assert_eq!(s.value(MetricId::HasNew, Some(&p)), 1.0);
    }

    #[test]
    fn sell_and_tick_clear_this_member() {
        let p = patterns(&["A|CU|F"]);
        let h = grain_id_hash("A|CU|F");
        let mut s = BurstSlotState::default();
        s.on_trade(&buy(10, Some(1), 1, Some(h), 1.0), 0.0, f64::NAN);
        assert_eq!(s.value(MetricId::ThisMember, Some(&p)), 1.0);
        s.on_tick();
        assert_eq!(s.value(MetricId::ThisMember, Some(&p)), 0.0);

        s.on_trade(&buy(10, Some(2), 2, Some(h), 1.0), 0.0, 10.0);
        assert_eq!(s.value(MetricId::ThisMember, Some(&p)), 1.0);
        let sell = TradeLite {
            side: Side::Sell,
            sol: 0.5,
            slot: 10,
            template_hash: Some(h),
            on_curve: true,
            ..Default::default()
        };
        s.on_trade(&sell, 20.0, 10.0);
        assert_eq!(s.value(MetricId::ThisMember, Some(&p)), 0.0);
        assert_eq!(s.value(MetricId::WorkingBuyCount, Some(&p)), 2.0);
        assert_eq!(s.value(MetricId::PrePrintTrail, Some(&p)), 20.0);
    }

    #[test]
    fn mixed_working_ignores_organic_sol() {
        let p = patterns(&["Axiom Trade|CU|ATA|F", "Photon|CU|ATA|F"]);
        let ax = grain_id_hash("Axiom Trade|CU|ATA|F");
        let ph = grain_id_hash("Photon|CU|ATA|F");
        let pf = grain_id_hash("Pump.Fun");
        let mut s = BurstSlotState::default();
        s.on_trade(&buy(10, Some(1), 1, Some(ax), 0.5), 0.0, f64::NAN);
        s.on_trade(&buy(10, Some(2), 2, Some(pf), 3.0), 0.0, 10.0);
        s.on_trade(&buy(10, Some(3), 3, Some(ph), 0.5), 0.0, 10.0);
        assert_eq!(s.value(MetricId::WorkingTemplateCount, Some(&p)), 2.0);
        assert_eq!(s.value(MetricId::MemberTemplateCount, Some(&p)), 3.0);
        assert_eq!(s.value(MetricId::WorkingBuySol, Some(&p)), 1.0);
        assert_eq!(s.value(MetricId::WorkingWalletCount, Some(&p)), 2.0);
        assert_eq!(s.value(MetricId::ThisWorking, Some(&p)), 1.0);
        assert_eq!(s.value(MetricId::HasNew, Some(&p)), 1.0);
        assert_eq!(s.value(MetricId::HasUnknown, Some(&p)), 0.0);
    }

    #[test]
    fn unknown_wallet_sets_has_unknown_and_does_not_count_as_new() {
        let p = patterns(&["A|CU|F"]);
        let h = grain_id_hash("A|CU|F");
        let mut s = BurstSlotState::default();
        s.on_trade(&buy(10, Some(1), 0, Some(h), 1.0), 0.0, f64::NAN);
        assert_eq!(s.value(MetricId::HasUnknown, Some(&p)), 1.0);
        assert_eq!(s.value(MetricId::HasNew, Some(&p)), 0.0);
        assert_eq!(s.value(MetricId::SameWalletCount, Some(&p)), 0.0);
    }

    #[test]
    fn the_working_slice_is_not_the_whole_prefix() {
        let p = patterns(&["A|CU|ATA|F"]);
        let a = grain_id_hash("A|CU|ATA|F");
        let x = grain_id_hash("Other|CU|F");
        let mut s = BurstSlotState::default();
        s.on_trade(&buy(10, Some(1), 1, Some(a), 1.0), 20.0, 12.0);
        s.on_trade(&buy(10, Some(2), 2, Some(a), 0.5), 20.0, 12.0);
        s.on_trade(&buy(10, Some(3), 3, Some(x), 0.25), 20.0, 12.0);

        // The whole pack: two grains, so this is not a same-template pack even
        // though every working-list buy in it shares one.
        assert_eq!(s.value(MetricId::MemberTemplateCount, Some(&p)), 2.0);
        // The working slice of it.
        assert_eq!(s.value(MetricId::WorkingBuyCount, Some(&p)), 2.0);
        assert_eq!(s.value(MetricId::WorkingBuySol, Some(&p)), 1.5);
        assert_eq!(s.value(MetricId::WorkingWalletCount, Some(&p)), 2.0);
        // This print's grain only.
        assert_eq!(s.value(MetricId::SameBuyCount, Some(&p)), 1.0);
        assert_eq!(s.value(MetricId::SameBuySol, Some(&p)), 0.25);
    }

    #[test]
    fn working_buy_share_is_100_only_on_a_pure_pack() {
        let p = patterns(&["A|CU|ATA|F"]);
        let a = grain_id_hash("A|CU|ATA|F");
        let x = grain_id_hash("Other|CU|F");
        let mut s = BurstSlotState::default();
        // Empty prefix has no share.
        assert!(s.value(MetricId::WorkingBuyShare, Some(&p)).is_nan());

        s.on_trade(&buy(10, Some(1), 1, Some(a), 1.0), 20.0, 12.0);
        s.on_trade(&buy(10, Some(2), 2, Some(a), 1.0), 20.0, 12.0);
        assert_eq!(s.value(MetricId::WorkingBuyShare, Some(&p)), 100.0);

        // One uncatalogued buyer joins and the pack stops being pure.
        s.on_trade(&buy(10, Some(3), 3, Some(x), 1.0), 20.0, 12.0);
        let share = s.value(MetricId::WorkingBuyShare, Some(&p));
        assert!((share - 200.0 / 3.0).abs() < 1e-9, "{share}");
        assert!(share < 100.0);
    }




}
