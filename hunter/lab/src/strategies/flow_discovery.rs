//! Volume-flow structure discovery — score distinct trade `ix_labels` sequences
//! inside sweep-style fingerprint groups so a user can toggle volume patterns.
//!
//! See `hunter/docs/roadmap/volume-flow-split-plan.md` §7.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};

use hunter_engine::metrics::flow_split::ix_hash;
use serde::Serialize;
use serde_json::Value;

use crate::sweep::corpus::{Corpus, CorpusToken};
use crate::sweep::grouped_engine::partition;
use crate::sweep::grouping::{GroupField, GroupKey, SOL_BUCKET_WIDTH};

/// Cap ranked structures returned per group (wire + UI).
pub const MAX_STRUCTURES_PER_GROUP: usize = 64;
/// `group_lift` below this ⇒ ambiguity warning (plan §7.1).
pub const LIFT_AMBIGUOUS: f64 = 1.25;
/// Per-token gross floor for "meaningful volume" in cross-token recurrence.
pub const MIN_STRUCTURE_SOL: f64 = 0.05;
/// Default: drop groups smaller than this.
pub const DEFAULT_MIN_TOKENS: usize = 3;

#[derive(Clone, Debug)]
pub struct DiscoveryConfig {
    pub group_by: Vec<GroupField>,
    pub bucket_width_sol: f64,
    pub min_tokens: usize,
    pub min_structure_sol: f64,
    pub lift_ambiguous: f64,
    pub max_structures_per_group: usize,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            group_by: Vec::new(),
            bucket_width_sol: SOL_BUCKET_WIDTH,
            min_tokens: DEFAULT_MIN_TOKENS,
            min_structure_sol: MIN_STRUCTURE_SOL,
            lift_ambiguous: LIFT_AMBIGUOUS,
            max_structures_per_group: MAX_STRUCTURES_PER_GROUP,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
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
}

#[derive(Clone, Debug, Serialize)]
pub struct DiscoveryGroup {
    pub group_key: Value,
    pub n_tokens: usize,
    pub n_trades_scored: u64,
    pub ambiguity: bool,
    pub structures: Vec<StructureScore>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DiscoveryResult {
    pub groups: Vec<DiscoveryGroup>,
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
    let width = if cfg.bucket_width_sol.is_finite() && cfg.bucket_width_sol > 0.0 {
        cfg.bucket_width_sol
    } else {
        SOL_BUCKET_WIDTH
    };

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

    let parts = partition(corpus, &cfg.group_by, width);
    let mut groups: Vec<DiscoveryGroup> = Vec::new();

    for (key, idxs) in parts {
        if idxs.len() < cfg.min_tokens {
            continue;
        }
        if cancelled(cancel) {
            return Err(Cancelled);
        }
        let g = score_group(
            corpus,
            &key,
            &idxs,
            cfg,
            window_gross_total,
            &window_gross_by_struct,
            &labels_by_hash,
        );
        groups.push(g);
    }

    groups.sort_by_key(|g| std::cmp::Reverse(g.n_tokens));
    Ok(DiscoveryResult { groups })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

fn cancelled(flag: Option<&AtomicBool>) -> bool {
    flag.map(|f| f.load(Ordering::Acquire)).unwrap_or(false)
}

fn score_group(
    corpus: &Corpus,
    key: &GroupKey,
    idxs: &[usize],
    cfg: &DiscoveryConfig,
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
        slots: Vec<u64>,
    }

    let mut by_struct: HashMap<u64, Acc> = HashMap::new();
    let mut group_gross_total = 0.0_f64;
    let mut n_trades_scored = 0_u64;

    for &ti in idxs {
        let tok: &CorpusToken = &corpus.tokens[ti];
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
            acc.slots.push(t.slot);
            *acc.gross_by_token.entry(ti).or_insert(0.0) += g;
            let signed = if t.is_buy { g } else { -g };
            *acc.net_by_token.entry(ti).or_insert(0.0) += signed;
            if let Some(w) = t.wallet.as_deref() {
                let wh = hunter_engine::metrics::flow_split::wallet_hash(w);
                acc.wallets.insert(wh);
                acc.wallets_by_token.entry(ti).or_default().insert(wh);
            }
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

    let ambiguity = structures
        .first()
        .map(|s| s.group_lift < cfg.lift_ambiguous)
        .unwrap_or(false);

    DiscoveryGroup {
        group_key: key.to_json(),
        n_tokens,
        n_trades_scored,
        ambiguity,
        structures,
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

    #[test]
    fn null_ix_labels_excluded_from_scoring() {
        let tokens = vec![tok(
            "n1",
            200_000,
            vec![CorpusTrade {
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
