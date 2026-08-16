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
use crate::family_search::gates::{AxisDuplication, Freshness};
use crate::family_search::oracle::Capture;
use crate::family_search::score::TermContribution;

/// One family member and what the cohort itself pays, before any rule.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SiblingRow {
    pub fp_id: Uuid,
    pub name: String,
    /// The varied axis's value (SOL for a lamports axis). `None` on a family of one.
    pub axis_value: Option<f64>,
    /// The held-out cohort the reported level comes from.
    pub is_target: bool,
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
    pub pnl_sol: f64,
    pub entry_sol: f64,
    /// Money over capital. A count-only table ranks a term that fires 200× for
    /// −0.4◎ level with one that fires 20× for +1.1◎.
    pub pnl_pct: f64,
}

/// One term's narrow contribution — the finalist re-scored with it dropped.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TermRow {
    pub label: String,
    pub ret_full_pct: f64,
    pub ret_without_pct: f64,
    pub delta_pct: f64,
    /// Dropping it changed nothing at all on this cohort.
    pub inert: bool,
}

impl From<TermContribution> for TermRow {
    fn from(t: TermContribution) -> Self {
        Self {
            delta_pct: t.delta_pct(),
            inert: t.inert(),
            label: t.label,
            ret_full_pct: t.ret_full_pct,
            ret_without_pct: t.ret_without_pct,
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
    pub oracle_pnl_sol: f64,
    pub realized_pnl_sol: f64,
}

impl From<Capture> for CaptureDto {
    fn from(c: Capture) -> Self {
        Self {
            capture_pct: c.capture_pct,
            n_with_upside: c.n_with_upside,
            n_no_upside: c.n_no_upside,
            oracle_pnl_sol: c.oracle_pnl_sol,
            realized_pnl_sol: c.realized_pnl_sol,
        }
    }
}

/// How fresh the data the run scanned is, against the range it asked for.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct FreshnessDto {
    pub last_trade_at: Option<chrono::DateTime<chrono::Utc>>,
    pub requested_until: chrono::DateTime<chrono::Utc>,
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
    /// The finalist: best pooled fit, level from the target cohort.
    pub draft: Option<CandidateRow>,
    /// What the fingerprint pays with **no gate** — a property of the cohort (D6).
    pub ungated_control: Option<CandidateRow>,
    /// The oracle half of the draft's grade (D3).
    pub capture: CaptureDto,
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
            pnl_sol: r.pnl_sol,
            entry_sol: r.entry_sol,
            pnl_pct: r.pnl_pct(),
        })
        .collect();
    (rows, a.n_other, a.other_pnl_sol)
}

/// The portrait, in creator terms. Each sentence answers one question and names its
/// own uncertainty; a metric name appears only after the sentence that explains it.
pub fn portrait(r: &Report) -> Vec<String> {
    let mut out = Vec::new();

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
            "The draft enters {:.0}% of this cohort's tokens and pays {:+.1}% on it, against \
             {:+.1}% for entering everything ungated.",
            d.target_enter_pct * 100.0,
            d.target_ret_pct,
            u.target_ret_pct
        )),
        (Some(d), None) => out.push(format!(
            "The draft enters {:.0}% of this cohort's tokens and pays {:+.1}% on it.",
            d.target_enter_pct * 100.0,
            d.target_ret_pct
        )),
        (None, _) => out.push(
            "No candidate survived the gates, so there is no draft — only the diagnostics \
             below."
                .to_string(),
        ),
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

    // Which alarm did the work.
    if let Some(top) = r.attribution.iter().max_by(|a, b| {
        a.pnl_sol.partial_cmp(&b.pnl_sol).unwrap_or(std::cmp::Ordering::Equal)
    }) {
        out.push(format!(
            "Most of the money came out on `{}`, which closed {} positions for {:+.3} SOL \
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
                requested_until: chrono::Utc::now(),
                shortfall_secs: 0,
                slack_secs: 3600,
                stale: false,
            },
            library: LibraryDto::default(),
            rho: None,
            fit_broad_holds: false,
            draft: None,
            ungated_control: None,
            capture: CaptureDto::default(),
            incumbent: None,
            attribution: Vec::new(),
            attribution_other_n: 0,
            attribution_other_pnl_sol: 0.0,
            narrow_recheck: Vec::new(),
            entry_gates: Vec::new(),
            archive: Vec::new(),
            portrait: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn the_portrait_reports_the_two_halves_of_the_grade_apart() {
        let mut r = base();
        r.capture = CaptureDto {
            capture_pct: Some(31.0),
            n_with_upside: 180,
            n_no_upside: 84,
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
