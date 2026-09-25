//! `m_slot` — this coin, this slot so far, this print's ix template.
//!
//! One slot prefix on the coin, reset when the slot changes. A **member** is a curve
//! buy with an ix template, not the create, that joins the current-slot prefix.
//! `template_count` is distinct templates on the WHOLE prefix; a tagged read
//! (`buy_count @working`) counts only members whose template carries the tag; the
//! `same_template_*` metrics only those sharing this print's template.
//! `buy_share_pct @working` is the tagged count over the whole prefix, so 100 is a PURE
//! pack. The tag is applied when a metric is READ ([`TemplatePatterns`]), so one buffer
//! per coin serves every fingerprint. The quiet slots before a burst are not here: they
//! are `m_flow.buy_count [4sl@1] = 0`.

use crate::hash::{HashedMap, HashedSet};

use super::registry::Metric;
use super::{Side, TradeLite};

// ── Patterns ─────────────────────────────────────────────────────────────────

/// One tag at the ix-template level (`TagPatterns::templates`): its `ix_template`
/// entries and its `program` entries. A program matches every template it ships.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TemplatePatterns {
    hashes: HashedSet,
    programs: HashedSet,
}

impl TemplatePatterns {
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

    pub fn is_empty(&self) -> bool {
        self.hashes.is_empty() && self.programs.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn contains(&self, hash: u64) -> bool {
        self.hashes.contains(&hash)
    }

    /// Grain id on the list, or this print's program on the list.
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
    program: Option<u64>,
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
    /// Grain → program for `seen_templates`, so a bare program name on the
    /// working list counts those grains as seen.
    seen_program: HashedMap<u64>,
    by_template: HashedMap<TemplateRun>,
    pre_slot_liquidity: f64,
    pre_print_trail: f64,
    this_template: Option<u64>,
    this_program: Option<u64>,
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
            seen_program: HashedMap::default(),
            by_template: HashedMap::default(),
            pre_slot_liquidity: f64::NAN,
            pre_print_trail: f64::NAN,
            this_template: None,
            this_program: None,
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
        self.this_program = t.program_hash;
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
                if let Some(prog) = t.program_hash {
                    self.seen_program.insert(h, prog);
                }
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
        run.program = t.program_hash;
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

    /// Read one metric. `tag` is the condition's tag at the template level: `Some` for a
    /// tagged read (`buy_count @working`), `None` for an untagged one. A metric that
    /// needs a tag reads `NaN` without one — never a count of nothing.
    pub fn value(&self, metric: Metric, tag: Option<&TemplatePatterns>) -> f64 {
        use Metric::*;
        match (metric, tag) {
            (SlotThisJoined, _) => f64::from(u8::from(self.this_member)),
            (SlotSameTemplateBuyCount, _) => self.this_run().map(|r| f64::from(r.count)).unwrap_or(f64::NAN),
            (SlotSameTemplateBuySol, _) => self.this_run().map(|r| r.sol).unwrap_or(f64::NAN),
            (SlotSameTemplateWalletCount, _) => self.this_run().map(|r| r.wallets.len() as f64).unwrap_or(f64::NAN),
            (SlotTemplateCount, None) => {
                if self.member_count == 0 {
                    f64::NAN
                } else {
                    self.by_template.len() as f64
                }
            }
            (SlotHasUnknownWallet, _) => f64::from(u8::from(self.has_unknown)),
            (SlotPacked, _) => self.packed(),
            (SlotLiquidityBeforeSol, _) => self.pre_slot_liquidity,
            (SlotTrailBeforePct, _) => self.pre_print_trail,
            (_, None) => f64::NAN,
            (SlotThisHasTag, Some(p)) => match self.this_template {
                Some(h) => f64::from(u8::from(p.matches(Some(h), self.this_program))),
                None => 0.0,
            },
            (SlotTemplateCount, Some(p)) => self.working_template_count(p),
            (SlotBuyCount, Some(p)) => self.working_count(p),
            (SlotBuySol, Some(p)) => self.working_sol(p),
            (SlotWalletCount, Some(p)) => self.working_wallets(p),
            (UniqueIxTemplates, Some(p)) => self.working_templates_seen(p),
            (SlotBuySharePct, Some(p)) => {
                if self.member_count == 0 {
                    f64::NAN
                } else {
                    100.0 * self.working_count(p) / f64::from(self.member_count)
                }
            }
            (SlotHasNewWallet, Some(p)) => f64::from(u8::from(self.has_new(p))),
            _ => f64::NAN,
        }
    }

    fn working_count(&self, p: &TemplatePatterns) -> f64 {
        let mut n = 0u32;
        for (h, run) in &self.by_template {
            if p.matches(Some(*h), run.program) {
                n = n.saturating_add(run.count);
            }
        }
        f64::from(n)
    }

    fn working_sol(&self, p: &TemplatePatterns) -> f64 {
        let mut s = 0.0;
        for (h, run) in &self.by_template {
            if p.matches(Some(*h), run.program) {
                s += run.sol;
            }
        }
        s
    }

    fn working_wallets(&self, p: &TemplatePatterns) -> f64 {
        let mut w = HashedSet::default();
        for (h, run) in &self.by_template {
            if p.matches(Some(*h), run.program) {
                w.extend(run.wallets.iter().copied());
            }
        }
        w.len() as f64
    }

    fn working_template_count(&self, p: &TemplatePatterns) -> f64 {
        self.by_template
            .keys()
            .filter(|h| {
                let prog = self.by_template.get(*h).and_then(|r| r.program);
                p.matches(Some(**h), prog)
            })
            .count() as f64
    }

    fn working_templates_seen(&self, p: &TemplatePatterns) -> f64 {
        self.seen_templates
            .iter()
            .filter(|h| p.matches(Some(**h), self.seen_program.get(*h).copied()))
            .count() as f64
    }

    fn has_new(&self, p: &TemplatePatterns) -> bool {
        self.by_template
            .iter()
            .any(|(h, run)| p.matches(Some(*h), run.program) && run.has_new)
    }
}

pub(crate) fn is_member(t: &TradeLite) -> bool {
    t.side == Side::Buy && t.on_curve && !t.is_launch && t.template_hash.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::template_grain::{grain_id_hash, program_id_hash};
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

    fn patterns(ids: &[&str]) -> TemplatePatterns {
        let mut h = HashedSet::default();
        for id in ids {
            h.insert(grain_id_hash(id));
        }
        TemplatePatterns::new(h)
    }

    #[test]
    fn packed_consecutive_vs_hole_and_nan_on_missing() {
        let mut s = BurstSlotState::default();
        let h = grain_id_hash("Axiom Trade|CU|ATA|F");

        s.on_trade(&buy(10, Some(5), 1, Some(h), 1.0), 0.0, f64::NAN);
        s.on_trade(&buy(10, Some(6), 2, Some(h), 1.0), 0.0, 10.0);
        s.on_trade(&buy(10, Some(7), 3, Some(h), 1.0), 0.0, 10.0);
        assert_eq!(s.value(Metric::SlotPacked, None), 1.0);

        let mut hole = BurstSlotState::default();
        hole.on_trade(&buy(10, Some(5), 1, Some(h), 1.0), 0.0, f64::NAN);
        hole.on_trade(&buy(10, Some(7), 2, Some(h), 1.0), 0.0, 10.0);
        assert_eq!(hole.value(Metric::SlotPacked, None), 0.0);

        let mut miss = BurstSlotState::default();
        miss.on_trade(&buy(10, Some(5), 1, Some(h), 1.0), 0.0, f64::NAN);
        miss.on_trade(&buy(10, None, 2, Some(h), 1.0), 0.0, 10.0);
        assert!(miss.value(Metric::SlotPacked, None).is_nan());
    }

    #[test]
    fn tx_index_zero_is_a_valid_first() {
        let h = grain_id_hash("Axiom Trade|CU|ATA|F");
        let mut s = BurstSlotState::default();
        s.on_trade(&buy(10, Some(0), 1, Some(h), 1.0), 0.0, f64::NAN);
        s.on_trade(&buy(10, Some(1), 2, Some(h), 1.0), 0.0, 10.0);
        assert_eq!(s.value(Metric::SlotPacked, None), 1.0);
        assert_eq!(s.value(Metric::SlotSameTemplateBuyCount, None), 2.0);
    }

    #[test]
    fn has_new_and_slot_change() {
        let p = patterns(&["A|CU|F"]);
        let h = grain_id_hash("A|CU|F");
        let mut s = BurstSlotState::default();
        s.on_trade(&buy(10, Some(1), 7, Some(h), 0.5), 15.0, f64::NAN);
        assert_eq!(s.value(Metric::SlotHasNewWallet, Some(&p)), 1.0);
        assert_eq!(s.value(Metric::SlotTrailBeforePct, None), 15.0);
        assert!(s.value(Metric::SlotLiquidityBeforeSol, None).is_nan());

        s.on_trade(&buy(11, Some(1), 7, Some(h), 0.5), 20.0, 12.0);
        // Same wallet, now a repeat — first-on-mint was slot 10.
        assert_eq!(s.value(Metric::SlotHasNewWallet, Some(&p)), 0.0);
        assert_eq!(s.value(Metric::SlotLiquidityBeforeSol, None), 12.0);
        assert_eq!(s.value(Metric::SlotSameTemplateBuyCount, None), 1.0);

        s.on_trade(&buy(11, Some(2), 8, Some(h), 0.4), 20.0, 12.0);
        assert_eq!(s.value(Metric::SlotHasNewWallet, Some(&p)), 1.0);
        assert_eq!(s.value(Metric::SlotSameTemplateWalletCount, None), 2.0);
        assert_eq!(s.value(Metric::SlotSameTemplateBuySol, None), 0.9);
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
        assert_eq!(s.value(Metric::UniqueIxTemplates, Some(&p)), 3.0);
        assert_eq!(s.value(Metric::SlotTemplateCount, Some(&p)), 1.0);
        s.on_trade(&buy(13, Some(1), 4, Some(gm), 0.5), 0.0, 10.0);
        assert_eq!(s.value(Metric::UniqueIxTemplates, Some(&p)), 4.0);
        assert_eq!(s.value(Metric::SlotTemplateCount, Some(&p)), 1.0);
    }

    /// A metric that needs a tag reads NaN without one; an untagged metric reads the slot.
    #[test]
    fn a_tagged_metric_without_a_tag_is_nan_not_zero() {
        let mut s = BurstSlotState::default();
        s.on_trade(&buy(10, Some(1), 1, Some(1), 1.0), 0.0, f64::NAN);
        assert_eq!(s.value(Metric::SlotSameTemplateBuyCount, None), 1.0);
        assert!(s.value(Metric::SlotThisHasTag, None).is_nan());
        assert!(s.value(Metric::SlotBuyCount, None).is_nan());
    }

    #[test]
    fn this_working_is_this_prints_grain() {
        let p = patterns(&["Axiom Trade|CU|ATA|F"]);
        let work = grain_id_hash("Axiom Trade|CU|ATA|F");
        let dead = grain_id_hash("Pump.Fun");
        let mut s = BurstSlotState::default();
        s.on_trade(&buy(10, Some(1), 1, Some(work), 1.0), 0.0, f64::NAN);
        assert_eq!(s.value(Metric::SlotThisHasTag, Some(&p)), 1.0);
        assert_eq!(s.value(Metric::SlotThisJoined, None), 1.0);
        s.on_trade(&buy(10, Some(2), 2, Some(dead), 1.0), 0.0, 10.0);
        assert_eq!(s.value(Metric::SlotThisHasTag, Some(&p)), 0.0);
        assert_eq!(s.value(Metric::SlotThisJoined, None), 1.0);
        // Organic Pump.Fun is a member but not working — mixed size ignores it.
        assert_eq!(s.value(Metric::SlotTemplateCount, Some(&p)), 1.0);
        assert_eq!(s.value(Metric::SlotTemplateCount, None), 2.0);
        assert_eq!(s.value(Metric::SlotBuySol, Some(&p)), 1.0);
        assert_eq!(s.value(Metric::SlotSameTemplateBuySol, None), 1.0); // this print's grain = Pump.Fun
    }

    #[test]
    fn bare_name_is_a_program_and_matches_every_grain() {
        let tags = crate::metrics::tags::config::compile_tags(&serde_json::json!({
            "working": { "match": { "program": ["Axiom Trade"] } }
        }));
        let p = tags[0].patterns.templates().expect("a program is template-level");
        let ata = grain_id_hash("Axiom Trade|ATA|F");
        let cu = grain_id_hash("Axiom Trade|CU|ATA|F");
        let pump = grain_id_hash("Pump.Fun");
        let axiom = program_id_hash("Axiom Trade");
        assert!(p.matches(Some(ata), Some(axiom)));
        assert!(p.matches(Some(cu), Some(axiom)));
        assert!(!p.matches(Some(pump), Some(program_id_hash("Pump.Fun"))));
        assert!(!p.contains(ata));

        let mut t = buy(10, Some(1), 1, Some(ata), 1.0);
        t.program_hash = Some(axiom);
        let mut s = BurstSlotState::default();
        s.on_trade(&t, 0.0, f64::NAN);
        assert_eq!(s.value(Metric::SlotThisHasTag, Some(&p)), 1.0);
        assert_eq!(s.value(Metric::SlotBuyCount, Some(&p)), 1.0);
    }

    #[test]
    fn launch_and_amm_do_not_join_the_prefix() {
        let p = patterns(&["Axiom Trade|CU|ATA|F"]);
        let h = grain_id_hash("Axiom Trade|CU|ATA|F");
        let mut s = BurstSlotState::default();
        let mut launch = buy(10, Some(1), 1, Some(h), 2.0);
        launch.is_launch = true;
        s.on_trade(&launch, 0.0, f64::NAN);
        assert_eq!(s.value(Metric::SlotThisJoined, None), 0.0);
        assert_eq!(s.value(Metric::SlotBuyCount, Some(&p)), 0.0);

        let mut amm = buy(10, Some(2), 2, Some(h), 2.0);
        amm.on_curve = false;
        s.on_trade(&amm, 0.0, 10.0);
        assert_eq!(s.value(Metric::SlotThisJoined, None), 0.0);
        assert_eq!(s.value(Metric::SlotBuyCount, Some(&p)), 0.0);

        s.on_trade(&buy(10, Some(3), 3, Some(h), 1.0), 0.0, 10.0);
        assert_eq!(s.value(Metric::SlotThisJoined, None), 1.0);
        assert_eq!(s.value(Metric::SlotBuyCount, Some(&p)), 1.0);
        // Launch/AMM wallets still mark ever — next slot they are repeats.
        s.on_trade(&buy(11, Some(1), 1, Some(h), 1.0), 0.0, 10.0);
        assert_eq!(s.value(Metric::SlotHasNewWallet, Some(&p)), 0.0);
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
        assert_eq!(s.value(Metric::SlotHasNewWallet, Some(&p)), 0.0);
        s.on_trade(&buy(10, Some(3), 2, Some(h), 1.0), 0.0, 10.0);
        assert_eq!(s.value(Metric::SlotHasNewWallet, Some(&p)), 1.0);
    }

    #[test]
    fn sell_and_tick_clear_this_member() {
        let p = patterns(&["A|CU|F"]);
        let h = grain_id_hash("A|CU|F");
        let mut s = BurstSlotState::default();
        s.on_trade(&buy(10, Some(1), 1, Some(h), 1.0), 0.0, f64::NAN);
        assert_eq!(s.value(Metric::SlotThisJoined, None), 1.0);
        s.on_tick();
        assert_eq!(s.value(Metric::SlotThisJoined, None), 0.0);

        s.on_trade(&buy(10, Some(2), 2, Some(h), 1.0), 0.0, 10.0);
        assert_eq!(s.value(Metric::SlotThisJoined, None), 1.0);
        let sell = TradeLite {
            side: Side::Sell,
            sol: 0.5,
            slot: 10,
            template_hash: Some(h),
            on_curve: true,
            ..Default::default()
        };
        s.on_trade(&sell, 20.0, 10.0);
        assert_eq!(s.value(Metric::SlotThisJoined, None), 0.0);
        assert_eq!(s.value(Metric::SlotBuyCount, Some(&p)), 2.0);
        assert_eq!(s.value(Metric::SlotTrailBeforePct, None), 20.0);
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
        assert_eq!(s.value(Metric::SlotTemplateCount, Some(&p)), 2.0);
        assert_eq!(s.value(Metric::SlotTemplateCount, None), 3.0);
        assert_eq!(s.value(Metric::SlotBuySol, Some(&p)), 1.0);
        assert_eq!(s.value(Metric::SlotWalletCount, Some(&p)), 2.0);
        assert_eq!(s.value(Metric::SlotThisHasTag, Some(&p)), 1.0);
        assert_eq!(s.value(Metric::SlotHasNewWallet, Some(&p)), 1.0);
        assert_eq!(s.value(Metric::SlotHasUnknownWallet, None), 0.0);
    }

    #[test]
    fn unknown_wallet_sets_has_unknown_and_does_not_count_as_new() {
        let p = patterns(&["A|CU|F"]);
        let h = grain_id_hash("A|CU|F");
        let mut s = BurstSlotState::default();
        s.on_trade(&buy(10, Some(1), 0, Some(h), 1.0), 0.0, f64::NAN);
        assert_eq!(s.value(Metric::SlotHasUnknownWallet, None), 1.0);
        assert_eq!(s.value(Metric::SlotHasNewWallet, Some(&p)), 0.0);
        assert_eq!(s.value(Metric::SlotSameTemplateWalletCount, None), 0.0);
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
        assert_eq!(s.value(Metric::SlotTemplateCount, None), 2.0);
        // The working slice of it.
        assert_eq!(s.value(Metric::SlotBuyCount, Some(&p)), 2.0);
        assert_eq!(s.value(Metric::SlotBuySol, Some(&p)), 1.5);
        assert_eq!(s.value(Metric::SlotWalletCount, Some(&p)), 2.0);
        // This print's grain only.
        assert_eq!(s.value(Metric::SlotSameTemplateBuyCount, None), 1.0);
        assert_eq!(s.value(Metric::SlotSameTemplateBuySol, None), 0.25);
    }

    #[test]
    fn working_buy_share_is_100_only_on_a_pure_pack() {
        let p = patterns(&["A|CU|ATA|F"]);
        let a = grain_id_hash("A|CU|ATA|F");
        let x = grain_id_hash("Other|CU|F");
        let mut s = BurstSlotState::default();
        // Empty prefix has no share.
        assert!(s.value(Metric::SlotBuySharePct, Some(&p)).is_nan());

        s.on_trade(&buy(10, Some(1), 1, Some(a), 1.0), 20.0, 12.0);
        s.on_trade(&buy(10, Some(2), 2, Some(a), 1.0), 20.0, 12.0);
        assert_eq!(s.value(Metric::SlotBuySharePct, Some(&p)), 100.0);

        // One uncatalogued buyer joins and the pack stops being pure.
        s.on_trade(&buy(10, Some(3), 3, Some(x), 1.0), 20.0, 12.0);
        let share = s.value(Metric::SlotBuySharePct, Some(&p));
        assert!((share - 200.0 / 3.0).abs() < 1e-9, "{share}");
        assert!(share < 100.0);
    }




}
