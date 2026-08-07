//! Volume-flow structure discovery — score distinct trade `ix_labels` sequences
//! inside sweep-style fingerprint groups so a user can toggle volume patterns.
//!
//! See `hunter/docs/plans/strategies/metrics-reference.md` "Discovery scoring".

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};

use hunter_engine::metrics::flow_split::ix_hash;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::sweep::corpus::{Corpus, CorpusToken};
use crate::sweep::grouped_engine::partition;
use crate::sweep::grouping::{GroupField, GroupKey, SolPrecision, SOL_BUCKET_WIDTH};

/// Cap ranked structures returned per group (wire + UI).
pub const MAX_STRUCTURES_PER_GROUP: usize = 64;
/// Cap ranked per-token roster returned per group (wire + UI token picker).
pub const MAX_TOKENS_PER_GROUP: usize = 500;
/// `group_lift` below this ⇒ ambiguity warning (plan §7.1).
pub const LIFT_AMBIGUOUS: f64 = 1.25;
/// Per-token gross floor for "meaningful volume" in cross-token recurrence.
pub const MIN_STRUCTURE_SOL: f64 = 0.05;
/// Default: drop groups smaller than this.
pub const DEFAULT_MIN_TOKENS: usize = 3;

#[derive(Clone, Debug)]
pub struct DiscoveryConfig {
    pub group_by: Vec<GroupField>,
    /// Bucket width for the SOL group axes, or `None` to group on exact amounts
    /// (`SolPrecision::Exact`). `None` -- not 0 -- is how "not bucketed" is spelled.
    pub bucket_width_sol: Option<f64>,
    pub min_tokens: usize,
    pub min_structure_sol: f64,
    pub lift_ambiguous: f64,
    pub max_structures_per_group: usize,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            group_by: Vec::new(),
            bucket_width_sol: Some(SOL_BUCKET_WIDTH),
            min_tokens: DEFAULT_MIN_TOKENS,
            min_structure_sol: MIN_STRUCTURE_SOL,
            lift_ambiguous: LIFT_AMBIGUOUS,
            max_structures_per_group: MAX_STRUCTURES_PER_GROUP,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StructureScore {
    pub ix_labels: Vec<String>,
    pub volume_share: f64,
    pub wash_symmetry: f64,
    pub cross_token_recurrence: f64,
    pub group_lift: f64,
    pub slot_burst: f64,
    pub wallet_reuse: f64,
    pub wallet_overlap: f64,
    pub n_trades: u64,
    pub gross_sol: f64,
    /// Buy-side gross SOL for this structure (`is_buy` trades only).
    pub buy_sol: f64,
    /// Sell-side gross SOL for this structure.
    pub sell_sol: f64,
    /// Gross SOL of this structure that landed in its token's **creation slot**
    /// (see [`crate::sweep::projection::creation_slot`]) — the launch-bundle
    /// share. `first_slot_gross_sol / gross_sol` is the purity the UI ranks on
    /// (`Launch%`); the *launch* bulk-select instead keys on mere presence
    /// ([`Self::first_slot_trades`] > 0), since the launch bundle is the set of
    /// shapes appearing in that slot. Purity stays visible because a shape that
    /// also trades later is ambient, and tagging it as volume sweeps organic flow
    /// (and, via wallet contagion, those wallets' other trades) into the volume
    /// bucket.
    ///
    /// `Option`, not a bare `f64`: a result cached before this field existed must
    /// read back as "unknown" (rendered `—`), never as an authoritative 0%.
    #[serde(default)]
    pub first_slot_gross_sol: Option<f64>,
    /// Trade count behind [`Self::first_slot_gross_sol`]. Same `Option` contract.
    #[serde(default)]
    pub first_slot_trades: Option<u64>,
    /// Per-wallet gross SOL on this structure — lets the UI preview live's
    /// wallet-contagion classifier (a wallet tagged by one checked structure
    /// sweeps its trades on OTHER structures into "volume" too, even if those
    /// don't match any pattern themselves — e.g. a buy-only and a sell-only
    /// ix_labels shape fired by the same bot wallet).
    pub wallets: Vec<WalletGross>,
}

/// One wallet's gross SOL contribution to a `StructureScore`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WalletGross {
    /// Stringified `wallet_hash` (u64 as decimal text — avoids JS f64 precision loss).
    pub wallet_hash: String,
    pub gross_sol: f64,
}

/// One token's aggregate contribution to a `DiscoveryGroup` — a cheap roster
/// (no trade payload) so the UI can rank/pick a token to preview in detail
/// without fetching every member token's full trade history up front.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenGross {
    pub mint_address: String,
    pub gross_sol: f64,
    pub n_trades: u64,
    /// This token's creation slot ([`crate::sweep::projection::creation_slot`]),
    /// or `None` when no trade in the corpus carries one. Shown so the UI can
    /// name the slot the launch set was read from.
    #[serde(default)]
    pub first_slot: Option<u64>,
    /// **Every** distinct ix shape that traded in THIS token's creation slot,
    /// ranked by first-slot gross desc.
    ///
    /// Deliberately not derived from [`DiscoveryGroup::structures`], which is the
    /// wrong instrument for "what was in this token's launch bundle" in three
    /// ways: it aggregates first-slot presence over *every* member token, it is
    /// ranked by lift/volume and truncated to `max_structures_per_group`, and its
    /// readers apply a group-wide dust floor. A launch shape that is rare and
    /// small — exactly what a bundler's tail looks like — loses on all three. This
    /// list is per token, uncapped, and unfloored: presence in the creation slot
    /// is an identity claim about the launch, and size does not get a vote.
    ///
    /// Bounded in practice by how many distinct shapes fit in one slot.
    ///
    /// `Option`, not a bare `Vec`: a result cached before this field existed must
    /// read back as *unknown* (the UI says "re-run discovery"), never as an
    /// authoritative "this token had no launch bundle". `Some([])` is that real
    /// zero. Same unknown-vs-zero contract as
    /// [`StructureScore::first_slot_trades`].
    #[serde(default)]
    pub first_slot_ix_labels: Option<Vec<Vec<String>>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiscoveryGroup {
    pub group_key: Value,
    pub n_tokens: usize,
    pub n_trades_scored: u64,
    pub ambiguity: bool,
    /// Whether this group's `group_lift` carries any information.
    ///
    /// Lift is `share(S|group) / share(S|window)`, and the window denominator is
    /// the **whole scored corpus**. When the group IS that corpus — a
    /// fingerprint-scoped run (one `ALL` group over the matched tokens), or any
    /// run with no group-by — the ratio is the group's own share over itself, so
    /// every structure scores exactly `1.0`. That is *no baseline*, not "ambient
    /// everywhere", and a reader that fails a `lift >= 1.25` gate on it rejects
    /// every row of the run (which is exactly what silenced the UI's auto-select
    /// on scoped runs). Readers must **skip** the lift gate when this is false,
    /// never fail it; `ambiguity` is likewise suppressed rather than always-on.
    ///
    /// `#[serde(default)]` = `true` for a result cached before the field existed:
    /// those runs were read as having a real lift, and for the multi-group ones
    /// that was correct. A cached scoped run stays gated until it is re-run.
    #[serde(default = "default_lift_defined")]
    pub lift_defined: bool,
    pub structures: Vec<StructureScore>,
    /// Ranked (desc gross_sol) member-token roster, capped at
    /// `MAX_TOKENS_PER_GROUP` — drives the per-token preview picker.
    pub tokens: Vec<TokenGross>,
}

/// A scored discovery run: its groups **plus the corpus identity that produced
/// them**.
///
/// The identity fields are not scoring output — [`score_corpus`] leaves them at
/// their defaults and the handler stamps them before caching. They exist because
/// the page rehydrates a disk-cached result on mount, at which point its form
/// state is whatever the user last left it, not what the run used. Reading the
/// precision or the label filter off the form to rebuild a fingerprint identity
/// silently attributes a card to the wrong fingerprint (or binds one that drops
/// an axis). The grouped sweep gets this right by reading its persisted run row;
/// this is the same contract for a run that lives only in the cache.
///
/// `#[serde(default)]` on each so a result cached before these fields existed
/// still deserializes — it reads back as "0.1 bucket, no filter, unscoped",
/// which is what those older runs actually were.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiscoveryResult {
    pub groups: Vec<DiscoveryGroup>,
    /// Bucket width (SOL) the continuous SOL group axes were binned at, or `None`
    /// when the run keyed them on their **exact** amount ([`SolPrecision::Exact`]).
    /// `None` is the mode, never a `0` width — see [`SolPrecision`].
    #[serde(default = "default_result_width")]
    pub bucket_width_sol: Option<f64>,
    /// The exact-set instruction-label corpus filter the run applied, or `None`.
    /// Part of what selected these groups, so it is part of the fingerprint
    /// identity a group binds to — the group key never carries it (the form
    /// disables the filter box when `ix_labels` is a group-by).
    #[serde(default)]
    pub ix_labels_filter: Option<Vec<String>>,
    /// Saved fingerprint the corpus was scoped to (engine match), or `None` for an
    /// unscoped run. Authoritative attribution for every group in the run.
    #[serde(default)]
    pub fingerprint_id: Option<uuid::Uuid>,
}

/// A pre-identity cached result predates exact mode, so it was bucketed at the
/// default width — not exact. Spelled out because `Option::default()` is `None`,
/// which [`SolPrecision::from_width`] reads as `Exact`: the wrong answer here.
fn default_result_width() -> Option<f64> {
    Some(SOL_BUCKET_WIDTH)
}

/// See [`DiscoveryGroup::lift_defined`] — a pre-field cached result is read as
/// having an informative lift, which is what those runs were treated as.
fn default_lift_defined() -> bool {
    true
}

/// Parse lake `ix_labels` JSON array string → ordered labels. `None` when missing,
/// empty, or malformed (excluded from scoring — plan §7.0.5).
pub fn parse_trade_ix_labels(raw: Option<&str>) -> Option<Vec<String>> {
    let s = raw?;
    let v: Vec<String> = serde_json::from_str(s).ok()?;
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

/// Score every surviving group in `corpus`. Polls `cancel` between tokens.
pub fn score_corpus(
    corpus: &Corpus,
    cfg: &DiscoveryConfig,
    cancel: Option<&AtomicBool>,
) -> Result<DiscoveryResult, Cancelled> {
    // One reader for the stored width — `None` (or a junk value) means exact.
    let precision = SolPrecision::from_width(cfg.bucket_width_sol);

    // Window-wide denominators for lift (after filters, before group split).
    let mut window_gross_by_struct: HashMap<u64, f64> = HashMap::new();
    let mut window_gross_total = 0.0_f64;
    let mut labels_by_hash: HashMap<u64, Vec<String>> = HashMap::new();

    for (i, tok) in corpus.tokens.iter().enumerate() {
        if i % 64 == 0 && cancelled(cancel) {
            return Err(Cancelled);
        }
        for t in tok.trades.iter() {
            let Some(labels) = parse_trade_ix_labels(t.ix_labels.as_deref()) else {
                continue;
            };
            let h = ix_hash(&labels);
            labels_by_hash.entry(h).or_insert_with(|| labels);
            let g = t.amount_sol.abs();
            *window_gross_by_struct.entry(h).or_insert(0.0) += g;
            window_gross_total += g;
        }
    }

    let parts = partition(corpus, &cfg.group_by, precision);
    let mut groups: Vec<DiscoveryGroup> = Vec::new();

    for (key, idxs) in parts {
        if idxs.len() < cfg.min_tokens {
            continue;
        }
        if cancelled(cancel) {
            return Err(Cancelled);
        }
        // Lift needs a baseline the group is measured AGAINST. A group holding
        // every scored token is its own baseline (ratio ≡ 1.0), and so is a
        // corpus with no scored volume at all (ratio ≡ 0.0) — say so instead of
        // shipping a number that looks like a verdict.
        let lift_defined = idxs.len() < corpus.tokens.len() && window_gross_total > 0.0;
        let g = score_group(
            corpus,
            &key,
            &idxs,
            cfg,
            lift_defined,
            window_gross_total,
            &window_gross_by_struct,
            &labels_by_hash,
        );
        groups.push(g);
    }

    groups.sort_by_key(|g| std::cmp::Reverse(g.n_tokens));
    Ok(DiscoveryResult {
        groups,
        // Stamped from the very `cfg` that partitioned above, so the echoed
        // precision cannot drift from the one the group keys were rendered at.
        // The corpus-selection fields are the handler's to fill — this fn never
        // sees the filter or the scope.
        bucket_width_sol: cfg.bucket_width_sol,
        ix_labels_filter: None,
        fingerprint_id: None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

fn cancelled(flag: Option<&AtomicBool>) -> bool {
    flag.map(|f| f.load(Ordering::Acquire)).unwrap_or(false)
}

#[allow(clippy::too_many_arguments)]
fn score_group(
    corpus: &Corpus,
    key: &GroupKey,
    idxs: &[usize],
    cfg: &DiscoveryConfig,
    lift_defined: bool,
    window_gross_total: f64,
    window_gross_by_struct: &HashMap<u64, f64>,
    labels_by_hash: &HashMap<u64, Vec<String>>,
) -> DiscoveryGroup {
    #[derive(Default)]
    struct Acc {
        gross: f64,
        buy: f64,
        sell: f64,
        n_trades: u64,
        wallets: BTreeSet<u64>,
        /// wallet_hash set per token index (for Jaccard overlap)
        wallets_by_token: HashMap<usize, BTreeSet<u64>>,
        /// per-token gross
        gross_by_token: HashMap<usize, f64>,
        /// per-token net (buy − sell)
        net_by_token: HashMap<usize, f64>,
        /// per-wallet gross (drives the contagion-overlap preview)
        gross_by_wallet: HashMap<u64, f64>,
        /// gross SOL + trade count landing in the owning token's creation slot
        first_slot_gross: f64,
        first_slot_trades: u64,
        slots: Vec<u64>,
    }

    let mut by_struct: HashMap<u64, Acc> = HashMap::new();
    let mut group_gross_total = 0.0_f64;
    let mut n_trades_scored = 0_u64;
    let mut token_gross: HashMap<usize, f64> = HashMap::new();
    let mut token_trades: HashMap<usize, u64> = HashMap::new();
    // Per-token launch set: creation slot, and gross-by-structure WITHIN that slot.
    // Kept per token (not folded into `Acc`) because the roster answers "what was
    // in THIS token's bundle" — see `TokenGross::first_slot_ix_labels`.
    let mut token_first_slot: HashMap<usize, u64> = HashMap::new();
    let mut token_first_slot_gross: HashMap<usize, HashMap<u64, f64>> = HashMap::new();

    for &ti in idxs {
        let tok: &CorpusToken = &corpus.tokens[ti];
        // One creation-slot read per token (SSOT with replay's `FirstSlotSettled`),
        // hoisted out of the trade loop.
        let creation_slot = crate::sweep::projection::creation_slot(&tok.trades);
        if let Some(slot) = creation_slot {
            token_first_slot.insert(ti, slot);
        }
        for t in tok.trades.iter() {
            let Some(labels) = parse_trade_ix_labels(t.ix_labels.as_deref()) else {
                continue;
            };
            let h = ix_hash(&labels);
            let g = t.amount_sol.abs();
            let acc = by_struct.entry(h).or_default();
            acc.gross += g;
            if t.is_buy {
                acc.buy += g;
            } else {
                acc.sell += g;
            }
            acc.n_trades += 1;
            n_trades_scored += 1;
            group_gross_total += g;
            if creation_slot == Some(t.slot) {
                acc.first_slot_gross += g;
                acc.first_slot_trades += 1;
                *token_first_slot_gross
                    .entry(ti)
                    .or_default()
                    .entry(h)
                    .or_insert(0.0) += g;
            }
            acc.slots.push(t.slot);
            *acc.gross_by_token.entry(ti).or_insert(0.0) += g;
            let signed = if t.is_buy { g } else { -g };
            *acc.net_by_token.entry(ti).or_insert(0.0) += signed;
            if let Some(w) = t.wallet.as_deref() {
                let wh = hunter_engine::metrics::flow_split::wallet_hash(w);
                acc.wallets.insert(wh);
                acc.wallets_by_token.entry(ti).or_default().insert(wh);
                *acc.gross_by_wallet.entry(wh).or_insert(0.0) += g;
            }
            *token_gross.entry(ti).or_insert(0.0) += g;
            *token_trades.entry(ti).or_insert(0) += 1;
        }
    }

    let n_tokens = idxs.len();
    let mut structures: Vec<StructureScore> = Vec::with_capacity(by_struct.len());

    for (h, acc) in by_struct {
        let Some(labels) = labels_by_hash.get(&h).cloned() else {
            continue;
        };
        let volume_share = if group_gross_total > 0.0 {
            acc.gross / group_gross_total * 100.0
        } else {
            0.0
        };

        // wash_symmetry: mean |net|/gross over tokens with gross(S,t)>0
        let mut wash_sum = 0.0;
        let mut wash_n = 0_u32;
        for (&ti, &tg) in &acc.gross_by_token {
            if tg <= 0.0 {
                continue;
            }
            let net = acc.net_by_token.get(&ti).copied().unwrap_or(0.0).abs();
            wash_sum += net / tg;
            wash_n += 1;
        }
        let wash_symmetry = if wash_n > 0 {
            wash_sum / f64::from(wash_n)
        } else {
            1.0
        };

        let meaningful = acc
            .gross_by_token
            .values()
            .filter(|&&g| g >= cfg.min_structure_sol)
            .count();
        let cross_token_recurrence = if n_tokens > 0 {
            meaningful as f64 / n_tokens as f64 * 100.0
        } else {
            0.0
        };

        let share_g = if group_gross_total > 0.0 {
            acc.gross / group_gross_total
        } else {
            0.0
        };
        let share_w = if window_gross_total > 0.0 {
            window_gross_by_struct.get(&h).copied().unwrap_or(0.0) / window_gross_total
        } else {
            0.0
        };
        let group_lift = if share_w > 0.0 {
            share_g / share_w
        } else {
            0.0
        };

        let slot_burst = slot_burst_pct(&acc.slots);
        let wallet_reuse = if acc.n_trades > 0 {
            1.0 - (acc.wallets.len() as f64 / acc.n_trades as f64)
        } else {
            0.0
        };
        let wallet_overlap = mean_jaccard(&acc.wallets_by_token);

        let mut wallets: Vec<WalletGross> = acc
            .gross_by_wallet
            .into_iter()
            .map(|(wh, gross_sol)| WalletGross {
                wallet_hash: wh.to_string(),
                gross_sol,
            })
            .collect();
        wallets.sort_by(|a, b| {
            b.gross_sol
                .partial_cmp(&a.gross_sol)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        structures.push(StructureScore {
            ix_labels: labels,
            volume_share,
            wash_symmetry,
            cross_token_recurrence,
            group_lift,
            slot_burst,
            wallet_reuse,
            wallet_overlap,
            n_trades: acc.n_trades,
            gross_sol: acc.gross,
            buy_sol: acc.buy,
            sell_sol: acc.sell,
            first_slot_gross_sol: Some(acc.first_slot_gross),
            first_slot_trades: Some(acc.first_slot_trades),
            wallets,
        });
    }

    // Rank: lift desc, volume_share desc, wash_symmetry asc
    structures.sort_by(|a, b| {
        b.group_lift
            .partial_cmp(&a.group_lift)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.volume_share
                    .partial_cmp(&a.volume_share)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| {
                a.wash_symmetry
                    .partial_cmp(&b.wash_symmetry)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    structures.truncate(cfg.max_structures_per_group);

    // "Nothing here stands out" is only sayable against a real baseline — an
    // undefined lift means the run has no out-of-group comparison, not that the
    // split is noisy.
    let ambiguity = lift_defined
        && structures
            .first()
            .map(|s| s.group_lift < cfg.lift_ambiguous)
            .unwrap_or(false);

    let mut tokens: Vec<TokenGross> = token_gross
        .into_iter()
        .map(|(ti, gross_sol)| {
            // Ranked by first-slot gross desc, then by the labels themselves so a
            // tie (two dust shapes at the same size) is stable across runs rather
            // than ordered by HashMap iteration.
            let mut shapes: Vec<(f64, Vec<String>)> = token_first_slot_gross
                .get(&ti)
                .map(|by_h| {
                    by_h.iter()
                        .filter_map(|(h, &g)| labels_by_hash.get(h).map(|l| (g, l.clone())))
                        .collect()
                })
                .unwrap_or_default();
            shapes.sort_by(|a, b| {
                b.0.partial_cmp(&a.0)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.1.cmp(&b.1))
            });
            TokenGross {
                mint_address: corpus.tokens[ti].mint.clone(),
                gross_sol,
                n_trades: token_trades.get(&ti).copied().unwrap_or(0),
                first_slot: token_first_slot.get(&ti).copied(),
                first_slot_ix_labels: Some(shapes.into_iter().map(|(_, l)| l).collect()),
            }
        })
        .collect();
    tokens.sort_by(|a, b| {
        b.gross_sol
            .partial_cmp(&a.gross_sol)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    tokens.truncate(MAX_TOKENS_PER_GROUP);

    DiscoveryGroup {
        group_key: key.to_json(),
        n_tokens,
        n_trades_scored,
        ambiguity,
        lift_defined,
        structures,
        tokens,
    }
}

fn slot_burst_pct(slots: &[u64]) -> f64 {
    if slots.is_empty() {
        return 0.0;
    }
    // Count trades whose slot has ≥1 other S-trade within ±1 slot.
    let mut counts: BTreeMap<u64, u32> = BTreeMap::new();
    for &s in slots {
        *counts.entry(s).or_insert(0) += 1;
    }
    let mut burst = 0_u64;
    for &s in slots {
        let nearby = counts.get(&s.saturating_sub(1)).copied().unwrap_or(0)
            + counts.get(&s).copied().unwrap_or(0)
            + counts.get(&(s.saturating_add(1))).copied().unwrap_or(0);
        // nearby includes self; need ≥2 trades in the ±1 window
        if nearby >= 2 {
            burst += 1;
        }
    }
    burst as f64 / slots.len() as f64 * 100.0
}

fn mean_jaccard(by_token: &HashMap<usize, BTreeSet<u64>>) -> f64 {
    let sets: Vec<&BTreeSet<u64>> = by_token.values().filter(|s| !s.is_empty()).collect();
    if sets.len() < 2 {
        return 0.0;
    }
    let mut sum = 0.0;
    let mut n = 0_u32;
    for i in 0..sets.len() {
        for j in (i + 1)..sets.len() {
            let inter = sets[i].intersection(sets[j]).count();
            let union = sets[i].union(sets[j]).count();
            if union > 0 {
                sum += inter as f64 / union as f64;
                n += 1;
            }
        }
    }
    if n > 0 {
        sum / f64::from(n)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::Utc;

    use super::*;
    use crate::sweep::grouping::TokenFingerprint;
    use crate::sweep::projection::CorpusTrade;

    fn trade(
        labels: &[&str],
        wallet: &str,
        sol: f64,
        is_buy: bool,
        slot: u64,
    ) -> CorpusTrade {
        let ix = serde_json::to_string(&labels).unwrap();
        CorpusTrade {
            flow: crate::sweep::projection::FlowKeys::from_stored(Some(&ix), Some(wallet)),
            block_time: Utc::now(),
            amount_sol: sol,
            token_amount: 1.0,
            price_per_token: sol,
            reserve_sol: None,
            reserve_token: None,
            real_reserve_sol: None,
            real_token_reserves: None,
            slot,
            leg_index: 0,
            is_buy,
            tx_signature: None,
            ix_labels: Some(ix.into_boxed_str()),
            wallet: Some(wallet.into()),
        }
    }

    fn tok(mint: &str, cu: i64, trades: Vec<CorpusTrade>) -> CorpusToken {
        CorpusToken {
            mint: mint.into(),
            symbol: mint.into(),
            created_at: Utc::now(),
            trades: Arc::new(trades),
            fp: TokenFingerprint {
                token_program_id: None,
                initial_buy_sol: None,
                cu_limit: Some(cu),
                cu_price: None,
                is_cashback_enabled: false,
                max_cost_lamports: None,
                spendable_lamports_in: None,
                first_slot_buy_sol: None,
                first_slot_sell_sol: None,
                ix_labels: vec![],
            },
            identity: None,
        }
    }

    /// Wash-like structure: high gross, near-zero net, shared across tokens in a
    /// tight CU group — should rank above a vanilla `["Pump.Fun: Buy"]` that also
    /// appears in the out-of-group window.
    #[test]
    fn wash_structure_ranks_above_ambient_buy() {
        // Group A (cu=200k): wash tooling ["Pump.Fun: Create","Pump.Fun: Buy"] + organic buys
        let mut tokens = vec![
            tok(
                "a1",
                200_000,
                vec![
                    trade(&["Pump.Fun: Create", "Pump.Fun: Buy"], "w1", 1.0, true, 100),
                    trade(&["Pump.Fun: Create", "Pump.Fun: Buy"], "w1", 1.0, false, 100),
                    trade(&["Pump.Fun: Buy"], "org1", 0.2, true, 110),
                ],
            ),
            tok(
                "a2",
                200_000,
                vec![
                    trade(&["Pump.Fun: Create", "Pump.Fun: Buy"], "w2", 1.0, true, 200),
                    trade(&["Pump.Fun: Create", "Pump.Fun: Buy"], "w2", 1.0, false, 200),
                    trade(&["Pump.Fun: Buy"], "org2", 0.2, true, 210),
                ],
            ),
            tok(
                "a3",
                200_000,
                vec![
                    trade(&["Pump.Fun: Create", "Pump.Fun: Buy"], "w3", 1.0, true, 300),
                    trade(&["Pump.Fun: Create", "Pump.Fun: Buy"], "w3", 1.0, false, 301),
                    trade(&["Pump.Fun: Buy"], "org3", 0.2, true, 310),
                ],
            ),
        ];
        // Out-of-group ambient: many vanilla buys so ["Pump.Fun: Buy"] has high window share
        for i in 0..10 {
            tokens.push(tok(
                &format!("b{i}"),
                300_000,
                vec![trade(
                    &["Pump.Fun: Buy"],
                    &format!("bx{i}"),
                    2.0,
                    true,
                    1000 + i,
                )],
            ));
        }

        let corpus = Corpus {
            tokens,
            hash: "test".into(),
            has_fingerprints: true,
            candidates_capped: false,
        };
        let cfg = DiscoveryConfig {
            group_by: vec![GroupField::CuLimit],
            min_tokens: 3,
            ..DiscoveryConfig::default()
        };
        let result = score_corpus(&corpus, &cfg, None).unwrap();
        let g = result
            .groups
            .iter()
            .find(|g| g.group_key.get("cu_limit").and_then(|v| v.as_str()) == Some("200000"))
            .expect("200k group");
        assert!(g.n_tokens >= 3);
        let top = &g.structures[0];
        assert_eq!(top.ix_labels, vec!["Pump.Fun: Create", "Pump.Fun: Buy"]);
        assert!(
            top.group_lift > LIFT_AMBIGUOUS,
            "wash tooling should lift above ambient: got {}",
            top.group_lift
        );
        assert!(
            top.wash_symmetry < 0.2,
            "wash net≈0 ⇒ low wash_symmetry, got {}",
            top.wash_symmetry
        );
        assert!(!g.ambiguity);
    }

    /// The per-token launch set is per TOKEN, and survives both of the losses that
    /// make the group-wide `first_slot_trades` unusable for "what was in THIS
    /// token's bundle": the rank truncation, and dust size.
    #[test]
    fn per_token_launch_set_is_uncapped_and_per_token() {
        let tokens = vec![
            tok(
                "a1",
                200_000,
                vec![
                    trade(&["Pump.Fun: Create", "Pump.Fun: Buy"], "w1", 1.0, true, 100),
                    // Dust, and in the launch slot: the bundler tail the old
                    // `SUGGEST_MIN_GROSS` floor silently dropped.
                    trade(&["Bundler: Tip"], "w1", 0.01, true, 100),
                    // Same token, LATER slot — not part of the launch bundle.
                    trade(&["Pump.Fun: Buy"], "org1", 0.5, true, 110),
                ],
            ),
            tok(
                "a2",
                200_000,
                vec![
                    trade(&["Pump.Fun: Create", "Pump.Fun: Buy"], "w2", 1.0, true, 200),
                    trade(&["Pump.Fun: Buy"], "org2", 0.5, true, 210),
                ],
            ),
            tok(
                "a3",
                200_000,
                vec![trade(
                    &["Pump.Fun: Create", "Pump.Fun: Buy"],
                    "w3",
                    1.0,
                    true,
                    300,
                )],
            ),
        ];
        let corpus = Corpus {
            tokens,
            hash: "test".into(),
            has_fingerprints: true,
            candidates_capped: false,
        };
        // One structure row survives ranking — the group-wide list is now blind to
        // the dust shape, exactly as the 64-row cap is blind on a real run.
        let cfg = DiscoveryConfig {
            group_by: vec![GroupField::CuLimit],
            min_tokens: 3,
            max_structures_per_group: 1,
            ..DiscoveryConfig::default()
        };
        let result = score_corpus(&corpus, &cfg, None).unwrap();
        let g = &result.groups[0];
        assert_eq!(g.structures.len(), 1, "rank cap applied");
        assert!(
            !g.structures
                .iter()
                .any(|s| s.ix_labels == ["Bundler: Tip".to_string()]),
            "the dust shape is NOT reachable through the ranked table"
        );

        let a1 = g
            .tokens
            .iter()
            .find(|t| t.mint_address == "a1")
            .expect("a1 in roster");
        assert_eq!(a1.first_slot, Some(100));
        assert_eq!(
            a1.first_slot_ix_labels.as_deref(),
            Some(
                [
                    vec!["Pump.Fun: Create".to_string(), "Pump.Fun: Buy".to_string()],
                    vec!["Bundler: Tip".to_string()],
                ]
                .as_slice()
            ),
            "both launch shapes, ranked by first-slot gross desc; the later-slot \
             [\"Pump.Fun: Buy\"] is not a launch shape"
        );

        let a2 = g
            .tokens
            .iter()
            .find(|t| t.mint_address == "a2")
            .expect("a2 in roster");
        assert_eq!(
            a2.first_slot_ix_labels.as_deref(),
            Some([vec!["Pump.Fun: Create".to_string(), "Pump.Fun: Buy".to_string()]].as_slice()),
            "a2 must not inherit a1's bundler shape — this is the per-token answer"
        );
    }

    /// Ambient `["Pump.Fun: Buy"]` alone inside a group that also sees it
    /// everywhere ⇒ lift≈1 and ambiguity flag.
    #[test]
    fn ambient_buy_flags_ambiguity() {
        let mut tokens = Vec::new();
        for i in 0..5 {
            tokens.push(tok(
                &format!("t{i}"),
                200_000,
                vec![trade(
                    &["Pump.Fun: Buy"],
                    &format!("w{i}"),
                    1.0,
                    true,
                    100 + i,
                )],
            ));
        }
        // Same structure outside the group
        for i in 0..5 {
            tokens.push(tok(
                &format!("o{i}"),
                300_000,
                vec![trade(
                    &["Pump.Fun: Buy"],
                    &format!("ow{i}"),
                    1.0,
                    true,
                    200 + i,
                )],
            ));
        }
        let corpus = Corpus {
            tokens,
            hash: "test".into(),
            has_fingerprints: true,
            candidates_capped: false,
        };
        let cfg = DiscoveryConfig {
            group_by: vec![GroupField::CuLimit],
            min_tokens: 3,
            ..DiscoveryConfig::default()
        };
        let result = score_corpus(&corpus, &cfg, None).unwrap();
        let g = result
            .groups
            .iter()
            .find(|g| g.group_key.get("cu_limit").and_then(|v| v.as_str()) == Some("200000"))
            .expect("group");
        assert!(g.ambiguity, "lift≈1 should warn");
        let buy = g
            .structures
            .iter()
            .find(|s| s.ix_labels == ["Pump.Fun: Buy"])
            .expect("buy structure");
        assert!(
            (buy.group_lift - 1.0).abs() < 0.15,
            "expected lift≈1, got {}",
            buy.group_lift
        );
    }

    /// Hand-label kit shape (V4.4): labeled volume structures land in top-5,
    /// or the fixture expects ambiguity.
    #[test]
    fn hand_label_kit_synthetic() {
        // Fixture: wash group expects create+buy in top-5; ambient-only group
        // expects ambiguity. Labels use the real ingest vocabulary.
        let labels: Value = serde_json::json!([
            {
                "group_key": { "cu_limit": "200000" },
                "volume_structures": [["Pump.Fun: Create", "Pump.Fun: Buy"]],
                "expected_ambiguous": false,
                "notes": "synthetic wash batch"
            },
            {
                "group_key": { "cu_limit": "300000" },
                "volume_structures": [["Pump.Fun: Buy"]],
                "expected_ambiguous": true,
                "notes": "ambient buy only"
            }
        ]);

        let mut tokens = vec![
            tok(
                "w1",
                200_000,
                vec![
                    trade(&["Pump.Fun: Create", "Pump.Fun: Buy"], "a", 1.0, true, 1),
                    trade(&["Pump.Fun: Create", "Pump.Fun: Buy"], "a", 1.0, false, 1),
                ],
            ),
            tok(
                "w2",
                200_000,
                vec![
                    trade(&["Pump.Fun: Create", "Pump.Fun: Buy"], "b", 1.0, true, 2),
                    trade(&["Pump.Fun: Create", "Pump.Fun: Buy"], "b", 1.0, false, 2),
                ],
            ),
            tok(
                "w3",
                200_000,
                vec![
                    trade(&["Pump.Fun: Create", "Pump.Fun: Buy"], "c", 1.0, true, 3),
                    trade(&["Pump.Fun: Create", "Pump.Fun: Buy"], "c", 1.0, false, 3),
                ],
            ),
        ];
        for i in 0..5 {
            tokens.push(tok(
                &format!("amb{i}"),
                300_000,
                vec![trade(
                    &["Pump.Fun: Buy"],
                    &format!("m{i}"),
                    1.0,
                    true,
                    50 + i as u64,
                )],
            ));
        }
        // Dilute ["Pump.Fun: Buy"] across many out-of-group tokens so the 300k-only
        // ambient group has lift≈1 (ambiguous), while wash at 200k still lifts.
        for i in 0..20 {
            tokens.push(tok(
                &format!("x{i}"),
                400_000,
                vec![trade(
                    &["Pump.Fun: Buy"],
                    &format!("x{i}"),
                    1.0,
                    true,
                    90 + i as u64,
                )],
            ));
        }

        let corpus = Corpus {
            tokens,
            hash: "kit".into(),
            has_fingerprints: true,
            candidates_capped: false,
        };
        let cfg = DiscoveryConfig {
            group_by: vec![GroupField::CuLimit],
            min_tokens: 3,
            ..DiscoveryConfig::default()
        };
        let result = score_corpus(&corpus, &cfg, None).unwrap();

        for fix in labels.as_array().unwrap() {
            let cu = fix["group_key"]["cu_limit"].as_str().unwrap();
            let g = result
                .groups
                .iter()
                .find(|g| g.group_key.get("cu_limit").and_then(|v| v.as_str()) == Some(cu))
                .unwrap_or_else(|| panic!("missing group {cu}"));
            let expect_amb = fix["expected_ambiguous"].as_bool().unwrap();
            if expect_amb {
                assert!(g.ambiguity, "group {cu} should be ambiguous");
            } else {
                for vs in fix["volume_structures"].as_array().unwrap() {
                    let want: Vec<String> = vs
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect();
                    let in_top = g.structures.iter().take(5).any(|s| s.ix_labels == want);
                    assert!(
                        in_top,
                        "labeled {:?} not in top-5 for group {cu}: {:?}",
                        want,
                        g.structures
                            .iter()
                            .take(5)
                            .map(|s| &s.ix_labels)
                            .collect::<Vec<_>>()
                    );
                }
            }
        }
    }

    /// Launch tooling trades only in the creation slot ⇒ 100% first-slot purity,
    /// while the same-group ambient buy that trades later reports 0% — the split
    /// the UI's first-slot auto-select keys on.
    #[test]
    fn first_slot_split_separates_launch_tooling_from_ambient() {
        let mut tokens = Vec::new();
        for i in 0..3u64 {
            let create = 100 + i * 10;
            tokens.push(tok(
                &format!("t{i}"),
                200_000,
                vec![
                    // Creation slot: bundle create+buy, then a same-slot sell.
                    trade(&["Pump.Fun: Create", "Pump.Fun: Buy"], "w", 1.0, true, create),
                    trade(&["Pump.Fun: Create", "Pump.Fun: Buy"], "w", 1.0, false, create),
                    // Later slots: ambient buys, plus one late trade of the launch
                    // shape so purity is a share, not a flag.
                    trade(&["Pump.Fun: Buy"], "org", 0.5, true, create + 5),
                    trade(&["Pump.Fun: Buy"], "org", 0.5, true, create + 6),
                ],
            ));
        }
        let corpus = Corpus {
            tokens,
            hash: "fs".into(),
            has_fingerprints: true,
            candidates_capped: false,
        };
        let cfg = DiscoveryConfig {
            group_by: vec![GroupField::CuLimit],
            min_tokens: 3,
            ..DiscoveryConfig::default()
        };
        let result = score_corpus(&corpus, &cfg, None).unwrap();
        let g = &result.groups[0];

        let launch = g
            .structures
            .iter()
            .find(|s| s.ix_labels == ["Pump.Fun: Create", "Pump.Fun: Buy"])
            .expect("launch structure");
        assert_eq!(launch.first_slot_gross_sol, Some(launch.gross_sol));
        assert_eq!(launch.first_slot_trades, Some(launch.n_trades));

        let ambient = g
            .structures
            .iter()
            .find(|s| s.ix_labels == ["Pump.Fun: Buy"])
            .expect("ambient structure");
        assert_eq!(ambient.first_slot_gross_sol, Some(0.0));
        assert_eq!(ambient.first_slot_trades, Some(0));
    }

    /// A run whose single group holds the whole corpus (fingerprint-scoped, or
    /// any run with no group-by) has no out-of-group baseline: every lift is
    /// exactly 1.0. It must say so via `lift_defined = false` and NOT raise the
    /// ambiguity flag — a `lift >= 1.25` gate applied to that 1.0 rejects every
    /// structure in the run.
    #[test]
    fn whole_corpus_group_reports_lift_undefined() {
        let mut tokens = Vec::new();
        for i in 0..4u64 {
            tokens.push(tok(
                &format!("s{i}"),
                200_000,
                vec![
                    trade(&["Pump.Fun: Create", "Pump.Fun: Buy"], "w", 1.0, true, 100 + i),
                    trade(&["Pump.Fun: Buy"], "org", 0.4, true, 110 + i),
                ],
            ));
        }
        let corpus = Corpus {
            tokens,
            hash: "scoped".into(),
            has_fingerprints: true,
            candidates_capped: false,
        };
        // No group-by ⇒ one ALL group over every token — the shape a scoped run takes.
        let cfg = DiscoveryConfig {
            group_by: vec![],
            min_tokens: 3,
            ..DiscoveryConfig::default()
        };
        let result = score_corpus(&corpus, &cfg, None).unwrap();
        assert_eq!(result.groups.len(), 1);
        let g = &result.groups[0];
        assert!(!g.lift_defined, "a group that IS the corpus has no baseline");
        assert!(!g.ambiguity, "no baseline ⇒ nothing to call ambiguous");
        for s in &g.structures {
            assert!(
                (s.group_lift - 1.0).abs() < 1e-9,
                "self-comparison is exactly 1.0, got {}",
                s.group_lift
            );
        }

        // Splitting the same corpus into two groups restores the baseline.
        let mut split = corpus;
        for (i, t) in split.tokens.iter_mut().enumerate() {
            if i >= 2 {
                t.fp.cu_limit = Some(300_000);
            }
        }
        let cfg = DiscoveryConfig {
            group_by: vec![GroupField::CuLimit],
            min_tokens: 1,
            ..DiscoveryConfig::default()
        };
        let split_result = score_corpus(&split, &cfg, None).unwrap();
        assert!(split_result.groups.iter().all(|g| g.lift_defined));
    }

    /// A pre-field cached result must read back as "unknown", never as an
    /// authoritative 0% first-slot (which the UI would happily rank).
    #[test]
    fn missing_first_slot_fields_deserialize_as_unknown() {
        let s: StructureScore = serde_json::from_value(serde_json::json!({
            "ix_labels": ["Pump.Fun: Buy"],
            "volume_share": 1.0,
            "wash_symmetry": 1.0,
            "cross_token_recurrence": 1.0,
            "group_lift": 1.0,
            "slot_burst": 0.0,
            "wallet_reuse": 0.0,
            "wallet_overlap": 0.0,
            "n_trades": 1,
            "gross_sol": 1.0,
            "buy_sol": 1.0,
            "sell_sol": 0.0,
            "wallets": [],
        }))
        .expect("legacy structure deserializes");
        assert_eq!(s.first_slot_gross_sol, None);
        assert_eq!(s.first_slot_trades, None);

        // Same contract on the roster: a cached run predating the per-token launch
        // set reads *unknown*, never an authoritative "this token had no bundle".
        let t: TokenGross = serde_json::from_value(serde_json::json!({
            "mint_address": "m1",
            "gross_sol": 1.0,
            "n_trades": 1,
        }))
        .expect("legacy roster token deserializes");
        assert_eq!(t.first_slot, None);
        assert_eq!(t.first_slot_ix_labels, None);
    }

    #[test]
    fn null_ix_labels_excluded_from_scoring() {
        let tokens = vec![tok(
            "n1",
            200_000,
            vec![CorpusTrade {
                flow: crate::sweep::projection::FlowKeys::default(),
                block_time: Utc::now(),
                amount_sol: 5.0,
                token_amount: 1.0,
                price_per_token: 5.0,
                reserve_sol: None,
                reserve_token: None,
                real_reserve_sol: None,
                real_token_reserves: None,
                slot: 1,
                leg_index: 0,
                is_buy: true,
                tx_signature: None,
                ix_labels: None,
                wallet: Some("w".into()),
            }],
        )];
        // Need min_tokens — pad with labeled trades
        let mut tokens = tokens;
        for i in 0..3 {
            tokens.push(tok(
                &format!("p{i}"),
                200_000,
                vec![trade(
                    &["Pump.Fun: Buy"],
                    &format!("p{i}"),
                    0.1,
                    true,
                    10 + i as u64,
                )],
            ));
        }
        let corpus = Corpus {
            tokens,
            hash: "n".into(),
            has_fingerprints: true,
            candidates_capped: false,
        };
        let cfg = DiscoveryConfig {
            group_by: vec![GroupField::CuLimit],
            min_tokens: 3,
            ..DiscoveryConfig::default()
        };
        let result = score_corpus(&corpus, &cfg, None).unwrap();
        let g = &result.groups[0];
        // The 5 SOL unlabeled trade must not appear as a structure or inflate gross
        assert!(g.structures.iter().all(|s| s.gross_sol < 1.0));
    }
}
