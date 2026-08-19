//! The board payload.
//!
//! Shaped by charter D1: **the family is the unit of a run, the fingerprint the unit
//! of a result.** So every candidate row carries two numbers that must never be
//! confused — the pooled `fit_ret_pct` that produced the *ordering* and the
//! `target_ret_pct` that is the *level*. On the reference family every candidate is
//! negative on the fit set while the winner pays +31% on the held-out target; a board
//! that printed one number would be wrong either way.
//!
//! Three columns sit beside the draft. Two are legitimate because they are properties
//! of the **cohort** and exist before any rule does: the **ungated control** (what the
//! fingerprint pays with no gate) and the **oracle** (what money was available). The
//! third, an incumbent, is an artifact — off by default, and it touches nothing (D6).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::family_search::attribution::Attribution;
use crate::family_search::diagnose::{AlarmRegret, ClauseFill, EntryRedundancy, ThresholdLadder};
use crate::family_search::gates::{AxisDuplication, CostClearance, EntryTiming, Freshness};
use crate::family_search::oracle::Capture;
use crate::family_search::score::TermContribution;
use crate::family_search::Spread;

/// One family member and what the cohort itself pays, before any rule.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SiblingRow {
    pub fp_id: Uuid,
    pub name: String,
    /// The varied axis's value (SOL for a lamports axis). `None` on a family of one.
    pub axis_value: Option<f64>,
    /// The held-out cohort the reported level comes from.
    pub is_target: bool,
    /// The ungated control's win rate here — the bar an entry gate has to beat on the
    /// target, and context for how safe this cohort is at all.
    pub ungated_win_pct: Option<f64>,
    /// Tokens the fingerprint matched — check it against a hand count every run: an
    /// ix-labels-only approximation of one reference cohort takes 3,440 tokens where
    /// the engine takes 264, and two rules invert in rank between the two.
    pub n_matched: u64,
    /// The **ungated control** on this cohort: what it pays with no gate at all.
    pub ungated_ret_pct: Option<f64>,
}

/// One candidate, scored broad and narrow.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CandidateRow {
    /// Stable clause text — the row key.
    pub key: String,
    pub params: serde_json::Value,
    /// End-event families the exit draws on.
    pub families: Vec<String>,
    /// Non-empty ⇒ show the flag. A price-trail term always carries one.
    pub flags: Vec<String>,
    /// Pooled `Σpnl ÷ Σentry` across the **fit** cohorts. Rank-only: quoting it as a
    /// level is the one mistake the fit/validate split exists to prevent.
    pub fit_ret_pct: f64,
    /// Held-out **target** cohort return — the number to report.
    pub target_ret_pct: f64,
    pub target_pnl_sol: f64,
    pub target_n_tokens: u64,
    /// Share of the target cohort's matched tokens this candidate entered.
    pub target_enter_pct: f64,
    /// Held-out win rate — the **safety** half of the grade. Entry decides safety and
    /// exit decides profit, so a board that prints only return grades half the rule.
    pub target_win_pct: Option<f64>,
    pub target_n_closed: u64,
    /// Entry **ideas**, not clauses: a band (floor + ceiling on one quantity) counts
    /// once. This is why a rule showing five entry metrics carries three.
    pub n_entry_quantities: usize,
    /// Searched alarms in the exit OR, standing terms excluded.
    pub n_alarms: usize,
}

/// One entry clause measured against the family's varied fingerprint axis.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EntryGateRow {
    pub clause: String,
    pub rho: Option<f64>,
    pub refused: bool,
    pub reason: Option<String>,
}

impl From<AxisDuplication> for EntryGateRow {
    fn from(d: AxisDuplication) -> Self {
        Self {
            refused: d.duplicates(),
            reason: d.refuse_reason(),
            clause: d.clause,
            rho: d.rho,
        }
    }
}

/// One authored exit term's share of the finalist's outcome.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AlarmRowDto {
    pub slot: u8,
    pub label: Option<String>,
    pub n: u64,
    pub n_wins: u64,
    /// Wins over closes for this alarm. One large win hides a hundred small losses in
    /// a money column; this is the column that separates them.
    pub win_rate_pct: Option<f64>,
    /// A mechanical exit the operator always wants (sell at migration), not a
    /// discovered alarm — reported so the closes add up, labelled so it is never read
    /// as an edge.
    pub standing: bool,
    pub pnl_sol: f64,
    pub entry_sol: f64,
    /// Money over capital. A count-only table ranks a term that fires 200× for
    /// −0.4◎ level with one that fires 20× for +1.1◎.
    pub pnl_pct: f64,
    /// The threshold the rule authored.
    pub authored_level: Option<f64>,
    /// Mean **gross** return at the close — the same quantity `m_position.pnl` reads.
    pub realized_level_pct: Option<f64>,
    /// Only then may a board print the two side by side.
    pub level_is_return: bool,
    /// Percentage points past the authored level the term actually closed. `None`
    /// unless the two are one quantity. A `pnl <= -8` realizing −20 is a stop that
    /// does not stop: prints are sparse and price gaps straight through the level.
    pub level_overshoot_pp: Option<f64>,
}

/// The same rule and the same taken set, priced two ways (D8 corollary).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct SpreadDto {
    pub authority_ret_pct: f64,
    pub optimistic_ret_pct: f64,
    pub spread_pp: f64,
    pub n_common: u64,
    pub n_authority_only: u64,
    pub n_optimistic_only: u64,
    /// The two passes took the same positions, so both numbers describe one set.
    pub clean: bool,
    /// The edge is no larger than the run's own uncertainty about the fill.
    pub fill_luck: bool,
}

impl From<Spread> for SpreadDto {
    fn from(s: Spread) -> Self {
        Self {
            clean: s.clean(),
            fill_luck: s.fill_luck(),
            authority_ret_pct: s.authority_ret_pct,
            optimistic_ret_pct: s.optimistic_ret_pct,
            spread_pp: s.spread_pp,
            n_common: s.n_common,
            n_authority_only: s.n_authority_only,
            n_optimistic_only: s.n_optimistic_only,
        }
    }
}

/// Whether the cohort can pay for its own execution, measured before any rule exists.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CostClearanceDto {
    pub band_pct: f64,
    pub median_move_pct: Option<f64>,
    /// `median_move_pct ÷ band_pct` — round trips of headroom the best exit leaves.
    pub headroom: Option<f64>,
    pub n_priced: u64,
    pub n_with_upside: u64,
    pub margin: f64,
    /// The search never ran: this cohort is untradeable at this buy size.
    pub refused: bool,
    /// It clears, but by less than one round trip.
    pub thin: bool,
    pub reason: Option<String>,
}

impl From<CostClearance> for CostClearanceDto {
    fn from(c: CostClearance) -> Self {
        Self {
            headroom: c.headroom(),
            refused: c.refuses(),
            thin: c.thin(),
            reason: c.refuse_reason(),
            band_pct: c.band_pct,
            median_move_pct: c.median_move_pct,
            n_priced: c.n_priced,
            n_with_upside: c.n_with_upside,
            margin: c.margin,
        }
    }
}

/// One entry clause's effect on the entry **instant** (plan §4d). Diagnostic only.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EntryTimingRow {
    pub clause: String,
    pub delay_added_secs: f64,
    pub capture_delta_pp: Option<f64>,
    pub admit_delta_pct: f64,
    pub lagging: bool,
    pub note: Option<String>,
}

impl From<EntryTiming> for EntryTimingRow {
    fn from(t: EntryTiming) -> Self {
        Self {
            lagging: t.lagging(),
            note: t.note(),
            clause: t.clause,
            delay_added_secs: t.delay_added_secs,
            capture_delta_pp: t.capture_delta_pp,
            admit_delta_pct: t.admit_delta_pct,
        }
    }
}

/// One term's narrow contribution — the finalist re-scored with it dropped.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TermRow {
    pub label: String,
    /// Which side it sits on. The two are graded by different jobs, so a board must
    /// never rank them in one list: entry buys safety, exit buys profit.
    pub is_entry: bool,
    pub ret_full_pct: f64,
    pub ret_without_pct: f64,
    pub delta_pct: f64,
    pub win_full_pct: Option<f64>,
    pub win_without_pct: Option<f64>,
    /// Win-rate points the term is worth — the entry side's own currency.
    pub win_delta_pp: Option<f64>,
    /// Dropping it changed nothing at all on this cohort.
    pub inert: bool,
    /// It pays in the currency of its own side.
    pub earns_its_place: bool,
}

impl From<TermContribution> for TermRow {
    fn from(t: TermContribution) -> Self {
        Self {
            delta_pct: t.delta_pct(),
            win_delta_pp: t.win_delta_pp(),
            inert: t.inert(),
            earns_its_place: t.earns_its_place(),
            label: t.label,
            is_entry: t.is_entry,
            ret_full_pct: t.ret_full_pct,
            ret_without_pct: t.ret_without_pct,
            win_full_pct: t.win_full_pct,
            win_without_pct: t.win_without_pct,
        }
    }
}

/// One idea the enrich stage offered the skeleton, and what it did (D12).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EnrichRow {
    pub label: String,
    pub is_entry: bool,
    pub ret_before_pct: f64,
    pub ret_after_pct: f64,
    pub ret_delta_pct: f64,
    pub win_before_pct: Option<f64>,
    pub win_after_pct: Option<f64>,
    pub win_delta_pp: Option<f64>,
    pub n_closed_after: u64,
    pub accepted: bool,
    /// Why it was refused. `None` when accepted — density is never silent either way.
    pub refused: Option<String>,
}

impl From<crate::family_search::enrich::Trial> for EnrichRow {
    fn from(t: crate::family_search::enrich::Trial) -> Self {
        Self {
            ret_delta_pct: t.ret_delta_pct(),
            win_delta_pp: t.win_delta_pp(),
            refused: t.refused.map(str::to_string),
            label: t.label,
            is_entry: t.is_entry,
            ret_before_pct: t.ret_before_pct,
            ret_after_pct: t.ret_after_pct,
            win_before_pct: t.win_before_pct,
            win_after_pct: t.win_after_pct,
            n_closed_after: t.n_closed_after,
            accepted: t.accepted,
        }
    }
}

/// The two-sided bars and what they decided (D11).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelectionDto {
    /// The bar the entry side had to clear: the stricter of the operator's floor and
    /// the ungated control's own win rate.
    pub win_bar_pct: f64,
    pub control_win_pct: Option<f64>,
    pub floor_win_pct: f64,
    pub min_closed: u64,
    /// Candidates the bars turned away before one cleared.
    pub n_rejected: usize,
    /// Why the ranking's own head is not the draft. Empty when they agree.
    pub top_rejected: Vec<String>,
    /// Nothing cleared both bars — there is no draft, and the reason is above.
    pub none_cleared: bool,
    /// Lower bound of the 95% Wilson interval on the draft's held-out win rate.
    /// `None` when there is no draft or it closed nothing.
    #[serde(default)]
    pub draft_win_low_pct: Option<f64>,
    /// The draft cleared the win bar as a point estimate but its lower bound does
    /// not — the safety edge is inside the noise of this many closes. Diagnostic
    /// only: it never un-selects the draft, it says how much to trust the clearance.
    #[serde(default)]
    pub win_within_noise: bool,
}

/// One clause's threshold ladder — the finalist replayed at neighbouring cuts.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct LadderPointDto {
    pub threshold: f64,
    pub ret_pct: f64,
    pub win_pct: Option<f64>,
    pub n_closed: u64,
    pub chosen: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ThresholdLadderDto {
    pub clause: String,
    pub is_entry: bool,
    /// `flat` / `plateau` / `fragile` / `sparse`. A fragile cut is one lucky value:
    /// one step either way loses more than half the ladder's range.
    pub verdict: String,
    pub points: Vec<LadderPointDto>,
}

impl From<ThresholdLadder> for ThresholdLadderDto {
    fn from(l: ThresholdLadder) -> Self {
        Self {
            verdict: l.verdict().label().to_string(),
            points: l
                .points
                .iter()
                .map(|p| LadderPointDto {
                    threshold: p.threshold,
                    ret_pct: p.ret_pct,
                    win_pct: p.win_pct,
                    n_closed: p.n_closed,
                    chosen: p.chosen,
                })
                .collect(),
            clause: l.clause,
            is_entry: l.is_entry,
        }
    }
}

/// One alarm's closes against its two counterfactuals — the best exit still
/// available after each close, and holding to the last print.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AlarmRegretDto {
    pub slot: u8,
    pub label: Option<String>,
    pub standing: bool,
    pub n: u64,
    pub n_priced: u64,
    pub realized_ret_pct: f64,
    pub best_later_ret_pct: f64,
    /// Points the best still-available exit beat the realized close by.
    pub forfeit_pp: f64,
    pub n_terminal: u64,
    /// Realized minus hold-to-the-end. Positive ⇒ firing beat holding on.
    pub realized_vs_terminal_pp: f64,
    pub band_pct: f64,
    /// `timed` / `protective` / `premature` / `unmeasured`.
    pub verdict: String,
}

impl From<AlarmRegret> for AlarmRegretDto {
    fn from(r: AlarmRegret) -> Self {
        Self {
            forfeit_pp: r.forfeit_pp(),
            verdict: r.verdict().label().to_string(),
            slot: r.slot,
            label: r.label,
            standing: r.standing,
            n: r.n,
            n_priced: r.n_priced,
            realized_ret_pct: r.realized_ret_pct,
            best_later_ret_pct: r.best_later_ret_pct,
            n_terminal: r.n_terminal,
            realized_vs_terminal_pp: r.realized_vs_terminal_pp,
            band_pct: r.band_pct,
        }
    }
}

/// One entry clause's standing in the AND: what it filters alone and how much of
/// that another clause already filters.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EntryRedundancyDto {
    pub clause: String,
    pub n_vetoed: u64,
    pub solo_ret_pct: f64,
    pub solo_win_pct: Option<f64>,
    pub solo_n_closed: u64,
    pub max_overlap_pct: Option<f64>,
    pub overlap_with: Option<String>,
    /// Nearly all of this clause's vetoes are also another clause's — dead weight
    /// even when drop-one ablation shows a small delta.
    pub redundant: bool,
}

impl From<EntryRedundancy> for EntryRedundancyDto {
    fn from(r: EntryRedundancy) -> Self {
        Self {
            redundant: r.redundant(),
            clause: r.clause,
            n_vetoed: r.n_vetoed,
            solo_ret_pct: r.solo_ret_pct,
            solo_win_pct: r.solo_win_pct,
            solo_n_closed: r.solo_n_closed,
            max_overlap_pct: r.max_overlap_pct,
            overlap_with: r.overlap_with,
        }
    }
}

/// One clause's drop-one contribution under both pricings, in its side's currency.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClauseFillDto {
    pub clause: String,
    pub is_entry: bool,
    pub delta_authority: Option<f64>,
    pub delta_optimistic: Option<f64>,
    /// The contribution flips sign or keeps under a quarter of itself between the
    /// two pricings — it is selecting fill luck, not signal.
    pub fill_dependent: bool,
}

impl From<ClauseFill> for ClauseFillDto {
    fn from(f: ClauseFill) -> Self {
        Self {
            fill_dependent: f.fill_dependent(),
            clause: f.clause,
            is_entry: f.is_entry,
            delta_authority: f.delta_authority,
            delta_optimistic: f.delta_optimistic,
        }
    }
}

/// How much of the money that was available the finalist took, and how many entries
/// never had any (D3).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct CaptureDto {
    pub capture_pct: Option<f64>,
    pub n_with_upside: u64,
    pub n_no_upside: u64,
    /// `n_no_upside` as a share of the entries — the **entry**'s own score, readable
    /// with no exit rule at all. Against the ungated control's share it says whether
    /// the gate filters losers or merely trades less.
    pub no_upside_pct: Option<f64>,
    pub oracle_pnl_sol: f64,
    pub realized_pnl_sol: f64,
}

impl From<Capture> for CaptureDto {
    fn from(c: Capture) -> Self {
        Self {
            capture_pct: c.capture_pct,
            n_with_upside: c.n_with_upside,
            n_no_upside: c.n_no_upside,
            no_upside_pct: crate::family_search::no_upside_pct(&c),
            oracle_pnl_sol: c.oracle_pnl_sol,
            realized_pnl_sol: c.realized_pnl_sol,
        }
    }
}

/// How fresh the data the run scanned is, against the range it asked for.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct FreshnessDto {
    pub last_trade_at: Option<chrono::DateTime<chrono::Utc>>,
    /// The upper bound the request named. `None` for an open-ended range, which the
    /// gate never measures — the lake's tail is the range.
    pub requested_until: Option<chrono::DateTime<chrono::Utc>>,
    pub shortfall_secs: i64,
    pub slack_secs: i64,
    pub stale: bool,
}

impl From<Freshness> for FreshnessDto {
    fn from(f: Freshness) -> Self {
        Self {
            stale: f.stale(),
            last_trade_at: f.last_trade_at,
            requested_until: f.requested_until,
            shortfall_secs: f.shortfall_secs,
            slack_secs: f.slack_secs,
        }
    }
}

/// The family, as a run resolved it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FamilyDto {
    /// The `fingerprints` column the members differ on. `None` ⇒ a family of one.
    pub varied_axis: Option<String>,
    /// Fit-broad does not apply: there is nothing to fit across and nothing to hold
    /// out, so the run degraded to single-cohort and says so.
    pub single_cohort: bool,
    pub members: Vec<SiblingRow>,
}

/// What the candidate library actually looked like — so a one-family search cannot
/// read as a broad one.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LibraryDto {
    pub n_candidates: u64,
    /// Candidates the per-family quota turned away. Reported, never silent.
    pub dropped_by_quota: u64,
    /// `end-event family → slots taken`.
    pub by_family: Vec<(String, u64)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub fingerprint_id: Uuid,
    pub fingerprint_name: String,
    pub family: FamilyDto,
    pub freshness: FreshnessDto,
    pub library: LibraryDto,
    /// Spearman(fit rank, held-out rank) — the **procedure's** self-test on this
    /// family, never a result (D2).
    pub rho: Option<f64>,
    /// `false` ⇒ the board states fit-broad does not hold here instead of ranking
    /// anyway. Also `false` when ρ could not be computed at all.
    pub fit_broad_holds: bool,
    /// The finalist: highest-ranked candidate clearing **both** the win-rate and the
    /// return bar on the held-out cohort (D11). `None` when nothing cleared.
    pub draft: Option<CandidateRow>,
    /// What the bars decided, and why the ranking's head is or is not the draft.
    pub selection: Option<SelectionDto>,
    /// What the fingerprint pays with **no gate** — a property of the cohort (D6).
    pub ungated_control: Option<CandidateRow>,
    /// The oracle half of the draft's grade (D3).
    pub capture: CaptureDto,
    /// The same, for the ungated control: the entry side's comparison. A gate that
    /// admits the same share of no-upside tokens as buying everything is not filtering.
    pub ungated_capture: Option<CaptureDto>,
    /// Standing exit terms this run carried, as the operator wrote them (D10).
    pub standing_terms: Vec<String>,
    /// Every idea the enrich stage offered the skeleton, accepted or refused (D12).
    pub enrich: Vec<EnrichRow>,
    /// Whether the cohort clears its own execution cost (D8). Measured on the ungated
    /// control, before the generator runs — `refused` means no search happened.
    pub cost_clearance: Option<CostClearanceDto>,
    /// The draft, priced twice on one taken set (D8 corollary).
    pub spread: Option<SpreadDto>,
    /// What each entry clause does to the entry instant.
    pub entry_timing: Vec<EntryTimingRow>,
    /// Display only, off by default. An incumbent is an artifact, not a baseline.
    pub incumbent: Option<CandidateRow>,
    /// Which authored exit term did the work, by money as well as by count (D4).
    pub attribution: Vec<AlarmRowDto>,
    /// Closes that were not authored-metric exits, so the counts visibly cover the
    /// whole closed population.
    pub attribution_other_n: u64,
    pub attribution_other_pnl_sol: f64,
    /// The finalist re-scored on the target with each term dropped.
    pub narrow_recheck: Vec<TermRow>,
    /// Each clause's threshold replayed at neighbouring cuts — plateau or spike
    /// (Slice 7). Diagnostic only: a verdict here grades trust, never selection.
    #[serde(default)]
    pub threshold_ladders: Vec<ThresholdLadderDto>,
    /// Each alarm's closes against the best exit still available after them and
    /// against holding to the last print (Slice 7).
    #[serde(default)]
    pub alarm_regret: Vec<AlarmRegretDto>,
    /// Solo scores and veto-set overlap per entry clause (Slice 7).
    #[serde(default)]
    pub entry_redundancy: Vec<EntryRedundancyDto>,
    /// Each clause's drop-one contribution under both pricings (Slice 7).
    #[serde(default)]
    pub fill_sensitivity: Vec<ClauseFillDto>,
    /// Entry clauses measured against the varied fingerprint axis.
    pub entry_gates: Vec<EntryGateRow>,
    pub archive: Vec<CandidateRow>,
    /// The portrait: plain-language sentences, in creator terms. The prose is the
    /// product; the draft is its executable form.
    pub portrait: Vec<String>,
    pub diagnostics: Vec<String>,
}

/// Roll an [`Attribution`] onto the report's two attribution fields.
pub fn attribution_rows(a: &Attribution) -> (Vec<AlarmRowDto>, u64, f64) {
    let rows = a
        .by_slot
        .iter()
        .map(|r| AlarmRowDto {
            slot: r.slot,
            label: r.label.clone(),
            n: r.n,
            n_wins: r.n_wins,
            win_rate_pct: r.win_rate_pct(),
            standing: r.standing,
            pnl_sol: r.pnl_sol,
            entry_sol: r.entry_sol,
            pnl_pct: r.pnl_pct(),
            authored_level: r.authored_level,
            realized_level_pct: r.realized_level_pct,
            level_is_return: r.level_is_return,
            level_overshoot_pp: r.level_overshoot_pp(),
        })
        .collect();
    (rows, a.n_other, a.other_pnl_sol)
}

/// The portrait, in creator terms. Each sentence answers one question and names its
/// own uncertainty; a metric name appears only after the sentence that explains it.
pub fn portrait(r: &Report) -> Vec<String> {
    let mut out = Vec::new();

    // Can this cohort pay for its own execution? Answered before anything else,
    // because a "no" means nothing below it was searched.
    if let Some(c) = &r.cost_clearance {
        if let Some(m) = c.median_move_pct {
            out.push(if c.refused {
                format!(
                    "The best exit available on the typical entry pays {m:+.2}%, against \
                     a {:.2}% round trip at this buy size — this launch shape cannot pay \
                     for its own execution, so no search was run. The loss is a ratio, \
                     and no threshold changes a ratio.",
                    c.band_pct
                )
            } else if c.thin {
                format!(
                    "Execution costs {:.2}% of every round trip here and the typical \
                     entry's best available exit pays {m:+.2}% — under one round trip of \
                     headroom, and a rule only ever takes a fraction of the best exit.",
                    c.band_pct
                )
            } else {
                format!(
                    "Execution costs {:.2}% per round trip and the typical entry's best \
                     available exit pays {m:+.2}% — {:.1}x the cost, so there is room \
                     for a rule to work here.",
                    c.band_pct,
                    c.headroom.unwrap_or(0.0)
                )
            });
        }
    }

    // What was searched.
    let n = r.family.members.len();
    out.push(match (&r.family.varied_axis, r.family.single_cohort || n <= 1) {
        (_, true) => format!(
            "“{}” has no sibling fingerprint, so this run graded one cohort on its own — \
             nothing was fitted across a family and nothing was held out.",
            r.fingerprint_name
        ),
        (Some(axis), false) => format!(
            "“{}” belongs to a family of {n} launch shapes that differ only in `{axis}`; \
             the ordering comes from the other {} pooled together, the number from this \
             one alone.",
            r.fingerprint_name,
            n.saturating_sub(1)
        ),
        (None, false) => format!("“{}” was graded across {n} cohorts.", r.fingerprint_name),
    });

    // Can the procedure be trusted on this family?
    out.push(match (r.rho, r.fit_broad_holds) {
        (Some(rho), true) => format!(
            "Rank transferred from the pooled family to the held-out cohort at rho {rho:+.2}, \
             so the ordering below is worth reading."
        ),
        (Some(rho), false) => format!(
            "Rank did NOT transfer from the pooled family to the held-out cohort (rho \
             {rho:+.2}): fitting broad does not apply to this family, so treat the ordering \
             below as unestablished."
        ),
        (None, _) => "There is no second cohort to check the ordering against, so the \
             ordering below is unvalidated."
            .to_string(),
    });

    // What is the rule, and what does it pay here?
    match (&r.draft, &r.ungated_control) {
        (Some(d), Some(u)) => out.push(format!(
            "The draft waits on {} entry condition{} and bails on {} independent alarm{}. It \
             enters {:.0}% of this cohort's tokens and pays {:+.1}% on them, against {:+.1}% \
             for entering everything ungated.",
            d.n_entry_quantities,
            plural(d.n_entry_quantities),
            d.n_alarms,
            plural(d.n_alarms),
            d.target_enter_pct * 100.0,
            d.target_ret_pct,
            u.target_ret_pct
        )),
        (Some(d), None) => out.push(format!(
            "The draft waits on {} entry condition{} and bails on {} independent alarm{}, \
             entering {:.0}% of this cohort's tokens for {:+.1}%.",
            d.n_entry_quantities,
            plural(d.n_entry_quantities),
            d.n_alarms,
            plural(d.n_alarms),
            d.target_enter_pct * 100.0,
            d.target_ret_pct
        )),
        (None, _) => out.push(match &r.selection {
            Some(s) if s.none_cleared && !s.top_rejected.is_empty() => format!(
                "No candidate cleared both bars, so there is no draft. The best-ranked one was \
                 refused because it {} (the entry had to beat a {:.0}% win rate — what buying \
                 everything achieves here).",
                s.top_rejected.join(" and "),
                s.win_bar_pct
            ),
            _ => "No candidate survived the gates, so there is no draft — only the \
                  diagnostics below."
                .to_string(),
        }),
    }

    // Safety and profit, named as the two different jobs they are.
    if let Some(d) = &r.draft {
        if let (Some(win), Some(sel)) = (d.target_win_pct, &r.selection) {
            let against = match sel.control_win_pct {
                Some(c) => format!("{c:.0}% for buying everything"),
                None => "no ungated comparison".to_string(),
            };
            out.push(format!(
                "It closes {} positions and wins {win:.0}% of them, against {against}. The entry \
                 side is what buys that: it decides which tokens are safe to hold, and the exit \
                 side decides how much they pay.",
                d.target_n_closed
            ));
        }
        // Did the entry actually filter, or just trade less?
        if let (Some(drafted), Some(ungated)) =
            (r.capture.no_upside_pct, r.ungated_capture.as_ref().and_then(|c| c.no_upside_pct))
        {
            out.push(if drafted + 1.0 < ungated {
                format!(
                    "Of the tokens it buys, {drafted:.0}% never had a profitable exit at all — \
                     down from {ungated:.0}% for buying everything, so the entry conditions are \
                     rejecting real losers rather than merely trading less."
                )
            } else {
                format!(
                    "Of the tokens it buys, {drafted:.0}% never had a profitable exit at all, \
                     against {ungated:.0}% for buying everything — the entry conditions are \
                     trading less without picking better."
                )
            });
        }
    }

    // What the enrich stage added, and what it refused.
    let accepted: Vec<&EnrichRow> = r.enrich.iter().filter(|e| e.accepted).collect();
    if !accepted.is_empty() {
        out.push(format!(
            "{} further condition{} earned {} place after the fit: {}. Each one had to pay in \
             its own currency — an entry idea by raising the win rate, an exit alarm by raising \
             the return.",
            accepted.len(),
            plural(accepted.len()),
            if accepted.len() == 1 { "its" } else { "their" },
            accepted.iter().map(|e| e.label.as_str()).collect::<Vec<_>>().join(", ")
        ));
    } else if !r.enrich.is_empty() {
        out.push(format!(
            "All {} extra conditions offered to the draft were refused — nothing else this \
             cohort earns makes it safer or richer, so the rule is as dense as the evidence \
             supports.",
            r.enrich.len()
        ));
    }

    // Terms carried but not discovered.
    if !r.standing_terms.is_empty() {
        out.push(format!(
            "{} standing exit term{} rode into every rule scored here ({}) — mechanics you \
             asked for, never searched and never credited with the edge.",
            r.standing_terms.len(),
            plural(r.standing_terms.len()),
            r.standing_terms.join(", ")
        ));
    }

    // The two halves of the grade, reported apart (D3).
    let c = &r.capture;
    out.push(match c.capture_pct {
        Some(pct) => format!(
            "Of the tokens it entered, {} had a winning exit available after the fill and the \
             draft took {pct:.0}% of it; {} never had one at all — the second number grades \
             the entry, not the exit.",
            c.n_with_upside, c.n_no_upside
        ),
        None => format!(
            "None of the {} entries had a profitable exit available after the fill, so there \
             is nothing for the exit to have captured — that is an entry result, not an exit \
             one.",
            c.n_no_upside
        ),
    });

    // How much of the number is the fill rather than the signal.
    if let Some(s) = &r.spread {
        out.push(if s.fill_luck {
            format!(
                "Repricing the same {} closes at the friendliest honest fill moves the \
                 result from {:+.1}% to {:+.1}% — a {:.1}pp swing that is larger than \
                 the edge itself, so this draft is priced on fill luck rather than on \
                 signal.",
                s.n_common, s.authority_ret_pct, s.optimistic_ret_pct, s.spread_pp
            )
        } else {
            format!(
                "Repricing the same {} closes at the friendliest honest fill reads \
                 {:+.1}% against {:+.1}% — a {:.1}pp execution swing the edge survives.",
                s.n_common, s.optimistic_ret_pct, s.authority_ret_pct, s.spread_pp
            )
        });
        if !s.clean {
            out.push(format!(
                "The two pricings did not close on the same positions ({} only at the \
                 run's fill, {} only at the optimistic one), so read the swing as \
                 indicative rather than as one taken set measured twice.",
                s.n_authority_only, s.n_optimistic_only
            ));
        }
    }

    // A stop that does not stop.
    if let Some(worst) = r
        .attribution
        .iter()
        .filter(|a| a.level_is_return)
        .filter_map(|a| a.level_overshoot_pp.map(|d| (a, d)))
        .filter(|(_, d)| *d < -1.0)
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    {
        out.push(format!(
            "`{}` closed at {:+.1}% on average — {:.1} points past the level it asked \
             for. Prints are sparse enough here that price gaps straight through a \
             stop, so that level is a wish rather than a floor.",
            worst.0.label.as_deref().unwrap_or("the stop"),
            worst.0.realized_level_pct.unwrap_or(0.0),
            -worst.1
        ));
    }

    // A win-rate clearance the sample cannot actually support.
    if let Some(sel) = &r.selection {
        if sel.win_within_noise {
            if let (Some(low), Some(d)) = (sel.draft_win_low_pct, &r.draft) {
                out.push(format!(
                    "The draft clears the {:.0}% win-rate bar as a point estimate, but over \
                     its {} closes the rate's lower bound is {low:.0}% — the safety edge is \
                     inside the noise of this sample, so treat \"safer than buying \
                     everything\" as unproven rather than established.",
                    sel.win_bar_pct, d.target_n_closed
                ));
            }
        }
    }

    // A threshold that only works at exactly one value.
    let fragile: Vec<&ThresholdLadderDto> =
        r.threshold_ladders.iter().filter(|l| l.verdict == "fragile").collect();
    if !fragile.is_empty() {
        out.push(format!(
            "{} threshold{} sit{} on a spike rather than a plateau: {}. One step in either \
             direction gives back more than half the available range, so read {} as tuned \
             to this cohort's noise rather than as a level the launch shape defines.",
            fragile.len(),
            plural(fragile.len()),
            if fragile.len() == 1 { "s" } else { "" },
            fragile.iter().map(|l| format!("`{}`", l.clause)).collect::<Vec<_>>().join(", "),
            if fragile.len() == 1 { "that cut" } else { "those cuts" }
        ));
    }

    // An alarm that cuts winners instead of catching dumps.
    for a in r.alarm_regret.iter().filter(|a| !a.standing && a.verdict == "premature") {
        out.push(format!(
            "`{}` fires early here: over the {} closes with a print after them, the best \
             exit still available pays {:+.1}pp more than the alarm took, and holding to \
             the end would ALSO have beaten it — it is cutting winners on this cohort, not \
             catching dumps.",
            a.label.as_deref().unwrap_or("an unnamed alarm"),
            a.n_priced,
            a.forfeit_pp
        ));
    }

    // A clause whose contribution is the fill model, not the launch shape.
    let fill_dep: Vec<&ClauseFillDto> =
        r.fill_sensitivity.iter().filter(|f| f.fill_dependent).collect();
    if !fill_dep.is_empty() {
        out.push(format!(
            "The measured contribution of {} does not survive repricing at the friendliest \
             honest fill — it flips or collapses between the two pricings, so what it is \
             \"worth\" is fill luck rather than signal.",
            fill_dep.iter().map(|f| format!("`{}`", f.clause)).collect::<Vec<_>>().join(", ")
        ));
    }

    // A clause another clause already covers for.
    for red in r.entry_redundancy.iter().filter(|x| x.redundant) {
        if let (Some(o), Some(with)) = (red.max_overlap_pct, &red.overlap_with) {
            out.push(format!(
                "`{}` filters almost nothing of its own: {o:.0}% of the {} tokens it turns \
                 away are also turned away by `{with}`. Drop-one ablation cannot see this — \
                 the sibling covers for it — which is exactly why the overlap is measured.",
                red.clause, red.n_vetoed
            ));
        }
    }

    // An entry gate the move itself creates.
    for t in r.entry_timing.iter().filter(|t| t.lagging) {
        if let Some(note) = &t.note {
            out.push(note.clone());
        }
    }

    // Which alarm did the work. Standing terms are excluded: a mechanic the operator
    // asked for is not a finding about this launch shape.
    if let Some(top) = r
        .attribution
        .iter()
        .filter(|a| !a.standing)
        .max_by(|a, b| a.pnl_sol.partial_cmp(&b.pnl_sol).unwrap_or(std::cmp::Ordering::Equal))
    {
        let win = match top.win_rate_pct {
            Some(w) => format!(", winning {w:.0}% of them"),
            None => String::new(),
        };
        out.push(format!(
            "Most of the money came out on `{}`, which closed {} positions{win} for {:+.3} SOL \
             ({:+.1}% of the capital they committed).",
            top.label.as_deref().unwrap_or("an unnamed term"),
            top.n,
            top.pnl_sol,
            top.pnl_pct
        ));
    }

    // What was refused.
    let refused: Vec<&EntryGateRow> = r.entry_gates.iter().filter(|g| g.refused).collect();
    if !refused.is_empty() {
        out.push(format!(
            "{} entry clause(s) were refused as a second reading of the fingerprint axis \
             rather than a filter within it: {}.",
            refused.len(),
            refused.iter().map(|g| g.clause.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }

    // What the library actually covered.
    if r.library.dropped_by_quota > 0 {
        out.push(format!(
            "The candidate library spans {} end-event families; {} further candidates were \
             turned away by the per-family quota so no single kind of alarm could fill the \
             search.",
            r.library.by_family.len(),
            r.library.dropped_by_quota
        ));
    }

    if r.freshness.stale {
        out.push(format!(
            "The lake data ends {:.1}h before the range this run asked for.",
            r.freshness.shortfall_secs as f64 / 3600.0
        ));
    }
    out
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::family_search::attribution::rollup;
    use crate::family_search::fixtures::metric_exit;
    use hunter_engine::metrics::evaluator::Operator;
    use hunter_engine::metrics::MetricId;

    fn base() -> Report {
        Report {
            fingerprint_id: Uuid::nil(),
            fingerprint_name: "3ix:BuyExactSolIn · spend=5".into(),
            family: FamilyDto {
                varied_axis: Some("spendable_lamports_in".into()),
                single_cohort: false,
                members: Vec::new(),
            },
            freshness: FreshnessDto {
                last_trade_at: None,
                requested_until: Some(chrono::Utc::now()),
                shortfall_secs: 0,
                slack_secs: 3600,
                stale: false,
            },
            library: LibraryDto::default(),
            rho: None,
            fit_broad_holds: false,
            draft: None,
            selection: None,
            ungated_control: None,
            capture: CaptureDto::default(),
            ungated_capture: None,
            standing_terms: Vec::new(),
            enrich: Vec::new(),
            cost_clearance: None,
            spread: None,
            entry_timing: Vec::new(),
            incumbent: None,
            attribution: Vec::new(),
            attribution_other_n: 0,
            attribution_other_pnl_sol: 0.0,
            narrow_recheck: Vec::new(),
            threshold_ladders: Vec::new(),
            alarm_regret: Vec::new(),
            entry_redundancy: Vec::new(),
            fill_sensitivity: Vec::new(),
            entry_gates: Vec::new(),
            archive: Vec::new(),
            portrait: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn candidate_row(entry_ideas: usize, alarms: usize) -> CandidateRow {
        CandidateRow {
            key: "k".into(),
            params: serde_json::json!({}),
            families: Vec::new(),
            flags: Vec::new(),
            fit_ret_pct: -1.2,
            target_ret_pct: 31.0,
            target_pnl_sol: 0.31,
            target_n_tokens: 100,
            target_enter_pct: 0.42,
            target_win_pct: Some(62.0),
            target_n_closed: 100,
            n_entry_quantities: entry_ideas,
            n_alarms: alarms,
        }
    }

    #[test]
    fn the_portrait_reports_the_two_halves_of_the_grade_apart() {
        let mut r = base();
        r.capture = CaptureDto {
            capture_pct: Some(31.0),
            n_with_upside: 180,
            n_no_upside: 84,
            no_upside_pct: Some(100.0 * 84.0 / 264.0),
            oracle_pnl_sol: 1.0,
            realized_pnl_sol: 0.31,
        };
        let prose = portrait(&r);
        let line = prose.iter().find(|s| s.contains("winning exit available")).expect("capture line");
        // Both numbers, and the sentence that says which grades which.
        assert!(line.contains("180") && line.contains("84") && line.contains("31%"));
        assert!(line.contains("grades the entry"));
    }

    #[test]
    fn a_collapsed_rho_says_so_instead_of_presenting_an_ordering() {
        let mut r = base();
        r.rho = Some(0.12);
        r.fit_broad_holds = false;
        let prose = portrait(&r);
        assert!(prose.iter().any(|s| s.contains("did NOT transfer") && s.contains("+0.12")));

        r.rho = Some(0.83);
        r.fit_broad_holds = true;
        assert!(portrait(&r).iter().any(|s| s.contains("rho +0.83") && s.contains("worth reading")));
    }

    #[test]
    fn a_family_of_one_is_stated_not_papered_over() {
        let mut r = base();
        r.family.single_cohort = true;
        r.family.varied_axis = None;
        let prose = portrait(&r);
        assert!(prose[0].contains("no sibling fingerprint"));
        assert!(prose.iter().any(|s| s.contains("no second cohort")));
    }

    /// The two sides do different jobs, and the prose has to say which is which or a
    /// reader grades the whole rule on money and deletes every entry condition.
    #[test]
    fn the_portrait_names_the_shape_and_grades_the_two_sides_apart() {
        let mut r = base();
        r.draft = Some(candidate_row(3, 4));
        r.selection = Some(SelectionDto {
            win_bar_pct: 45.0,
            control_win_pct: Some(45.0),
            floor_win_pct: 0.0,
            min_closed: 8,
            n_rejected: 2,
            top_rejected: Vec::new(),
            none_cleared: false,
            draft_win_low_pct: Some(52.0),
            win_within_noise: false,
        });
        r.capture = CaptureDto { no_upside_pct: Some(22.0), ..Default::default() };
        r.ungated_capture = Some(CaptureDto { no_upside_pct: Some(41.0), ..Default::default() });
        let prose = portrait(&r);

        // The shape, in the operator's own terms.
        let shape = prose.iter().find(|s| s.contains("entry condition")).expect("shape line");
        assert!(shape.contains("3 entry conditions") && shape.contains("4 independent alarms"));

        // Safety, named as the entry side's job.
        let safety = prose.iter().find(|s| s.contains("wins 62%")).expect("safety line");
        assert!(safety.contains("45% for buying everything"));
        assert!(safety.contains("exit side decides how much they pay"));

        // And whether the entry filtered or merely traded less.
        let filt = prose.iter().find(|s| s.contains("never had a profitable exit")).unwrap();
        assert!(filt.contains("rejecting real losers"), "{filt}");
    }

    #[test]
    fn a_gate_that_only_trades_less_is_said_so_plainly() {
        let mut r = base();
        r.draft = Some(candidate_row(2, 3));
        r.capture = CaptureDto { no_upside_pct: Some(40.0), ..Default::default() };
        r.ungated_capture = Some(CaptureDto { no_upside_pct: Some(41.0), ..Default::default() });
        let prose = portrait(&r);
        let filt = prose.iter().find(|s| s.contains("never had a profitable exit")).unwrap();
        assert!(filt.contains("trading less without picking better"), "{filt}");
    }

    /// No draft is a result, and it has to name the bar that refused it.
    #[test]
    fn a_refused_top_candidate_says_which_bar_it_failed() {
        let mut r = base();
        r.selection = Some(SelectionDto {
            win_bar_pct: 45.0,
            control_win_pct: Some(45.0),
            floor_win_pct: 0.0,
            min_closed: 8,
            n_rejected: 12,
            top_rejected: vec!["win rate below the ungated control".into()],
            none_cleared: true,
            draft_win_low_pct: None,
            win_within_noise: false,
        });
        let prose = portrait(&r);
        let line = prose.iter().find(|s| s.contains("no draft")).expect("refusal line");
        assert!(line.contains("win rate below the ungated control"));
        assert!(line.contains("45% win rate"), "{line}");
    }

    /// Density is never silent: what was added, or that nothing was.
    #[test]
    fn the_enrich_stage_reports_both_outcomes() {
        let row = |label: &str, accepted: bool| EnrichRow {
            label: label.into(),
            is_entry: true,
            ret_before_pct: 21.0,
            ret_after_pct: 20.0,
            ret_delta_pct: -1.0,
            win_before_pct: Some(48.0),
            win_after_pct: Some(62.0),
            win_delta_pp: Some(14.0),
            n_closed_after: 150,
            accepted,
            refused: (!accepted).then(|| "did not make the entry safer".to_string()),
        };
        let mut r = base();
        r.enrich = vec![row("time >= 20", true)];
        assert!(portrait(&r)
            .iter()
            .any(|s| s.contains("earned its place after the fit") && s.contains("time >= 20")));

        r.enrich = vec![row("time >= 20", false), row("stall >= 45", false)];
        assert!(portrait(&r)
            .iter()
            .any(|s| s.contains("as dense as the evidence supports")));
    }

    /// A mechanic the operator asked for is never presented as a discovery.
    #[test]
    fn a_standing_term_is_named_as_carried_not_as_found() {
        let mut r = base();
        r.standing_terms = vec!["liquidity >= 85".into()];
        r.attribution = vec![
            AlarmRowDto {
                slot: 0,
                label: Some("stall >= 30".into()),
                n: 40,
                n_wins: 20,
                win_rate_pct: Some(50.0),
                standing: false,
                pnl_sol: 0.2,
                entry_sol: 0.4,
                pnl_pct: 50.0,
                authored_level: Some(30.0),
                realized_level_pct: None,
                level_is_return: false,
                level_overshoot_pp: None,
            },
            AlarmRowDto {
                slot: 1,
                label: Some("liquidity >= 85".into()),
                n: 10,
                n_wins: 10,
                win_rate_pct: Some(100.0),
                standing: true,
                // Richest row in the table — and still not a finding.
                pnl_sol: 2.0,
                entry_sol: 0.1,
                pnl_pct: 2000.0,
                authored_level: Some(85.0),
                realized_level_pct: None,
                level_is_return: false,
                level_overshoot_pp: None,
            },
        ];
        let prose = portrait(&r);
        assert!(prose.iter().any(|s| s.contains("standing exit term") && s.contains("liquidity >= 85")));
        // The "most of the money" line names the searched alarm, not the mechanic.
        let money = prose.iter().find(|s| s.contains("Most of the money")).expect("money line");
        assert!(money.contains("stall >= 30"), "{money}");
        assert!(money.contains("winning 50%"));
    }

    /// The Slice 7 diagnostics each earn a sentence — and each names the thing it
    /// distrusts, so the portrait stays an argument rather than a status dump.
    #[test]
    fn the_portrait_reports_reliability_findings_in_plain_terms() {
        let mut r = base();
        r.draft = Some(candidate_row(2, 3));
        r.selection = Some(SelectionDto {
            win_bar_pct: 45.0,
            control_win_pct: Some(45.0),
            floor_win_pct: 0.0,
            min_closed: 8,
            n_rejected: 0,
            top_rejected: Vec::new(),
            none_cleared: false,
            draft_win_low_pct: Some(41.0),
            win_within_noise: true,
        });
        r.threshold_ladders = vec![ThresholdLadderDto {
            clause: "nonvol_buy(2s) >= 1.6".into(),
            is_entry: false,
            verdict: "fragile".into(),
            points: Vec::new(),
        }];
        r.alarm_regret = vec![AlarmRegretDto {
            slot: 0,
            label: Some("stall >= 30".into()),
            standing: false,
            n: 40,
            n_priced: 36,
            realized_ret_pct: 8.0,
            best_later_ret_pct: 28.0,
            forfeit_pp: 20.0,
            n_terminal: 40,
            realized_vs_terminal_pp: -5.0,
            band_pct: 4.0,
            verdict: "premature".into(),
        }];
        r.fill_sensitivity = vec![ClauseFillDto {
            clause: "gross_flow(10s) < 15".into(),
            is_entry: false,
            delta_authority: Some(5.0),
            delta_optimistic: Some(0.4),
            fill_dependent: true,
        }];
        r.entry_redundancy = vec![EntryRedundancyDto {
            clause: "liquidity > 30".into(),
            n_vetoed: 40,
            solo_ret_pct: 10.0,
            solo_win_pct: Some(55.0),
            solo_n_closed: 90,
            max_overlap_pct: Some(95.0),
            overlap_with: Some("time >= 20".into()),
            redundant: true,
        }];
        let prose = portrait(&r);

        let noise = prose.iter().find(|s| s.contains("lower bound is 41%")).expect("noise line");
        assert!(noise.contains("unproven"), "{noise}");
        let frag = prose.iter().find(|s| s.contains("spike rather than a plateau")).unwrap();
        assert!(frag.contains("nonvol_buy(2s) >= 1.6"));
        let early = prose.iter().find(|s| s.contains("fires early")).expect("regret line");
        assert!(early.contains("stall >= 30") && early.contains("+20.0pp"), "{early}");
        let fill = prose.iter().find(|s| s.contains("fill luck rather than signal")).unwrap();
        assert!(fill.contains("gross_flow(10s) < 15"));
        let red = prose.iter().find(|s| s.contains("filters almost nothing")).unwrap();
        assert!(red.contains("liquidity > 30") && red.contains("time >= 20"), "{red}");
    }

    /// A premature STANDING alarm is the operator's own mechanic — never accused.
    #[test]
    fn a_standing_alarms_regret_stays_out_of_the_portrait() {
        let mut r = base();
        r.alarm_regret = vec![AlarmRegretDto {
            slot: 1,
            label: Some("liquidity >= 85".into()),
            standing: true,
            n: 10,
            n_priced: 10,
            realized_ret_pct: 5.0,
            best_later_ret_pct: 40.0,
            forfeit_pp: 35.0,
            n_terminal: 10,
            realized_vs_terminal_pp: -2.0,
            band_pct: 4.0,
            verdict: "premature".into(),
        }];
        assert!(!portrait(&r).iter().any(|s| s.contains("fires early")));
    }

    #[test]
    fn attribution_rows_carry_money_not_only_counts() {
        let outs = vec![
            metric_exit(0, MetricId::Retrace, Operator::Gte, 36.0, None, -0.2),
            metric_exit(1, MetricId::Stall, Operator::Gte, 30.0, None, 0.5),
            crate::family_search::fixtures::tp_exit(0.1),
        ];
        let (rows, other_n, other_pnl) = attribution_rows(&rollup(&outs, 0.01));
        assert_eq!(rows.len(), 2);
        assert_eq!(other_n, 1);
        assert!((other_pnl - 0.1).abs() < 1e-6);
        assert!(rows[0].pnl_pct < 0.0 && rows[1].pnl_pct > 0.0);
        assert_eq!(rows[1].label.as_deref(), Some("stall >= 30"));

        // The portrait names the term that made the money, not the one that fired most.
        let mut r = base();
        r.attribution = rows;
        assert!(portrait(&r).iter().any(|s| s.contains("`stall >= 30`")));
    }
}
