//! Which coins of a Flow Discovery group carry each highlighted lens item: the
//! chips and dots on the page's token list.
//!
//! A structure item is an `ix_shape` row read by the tags' own matcher
//! ([`IxShapeRows`]), and a wallet item is the wallet the venue credited, the same
//! two tests the chart's wash runs (`lensItemMatches`). So a coin is listed as
//! carrying an item exactly when its chart washes a trade for it.

use std::collections::BTreeMap;

use hunter_engine::metrics::fee::IxShapeRows;
use hunter_engine::metrics::Side;
use serde::Deserialize;
use serde_json::Value;

use crate::sweep::corpus::CorpusToken;

/// One highlighted item, keyed by the page's `lensItemId`.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LensItem {
    Wallet { id: String, wallet: String },
    /// `row` is one `match.ix_shape` entry.
    Structure { id: String, row: Value },
}

enum Matcher {
    Wallet(String),
    Shape(IxShapeRows),
}

/// Per member coin with a hit: item id -> trades it matches. A coin with no hit is
/// absent; an item a coin never trades is absent from that coin's map.
pub type LensHits = BTreeMap<String, BTreeMap<String, u64>>;

/// Count each item's trades on the group's coins `idxs` of `tokens`.
pub fn hits(tokens: &[CorpusToken], idxs: &[usize], items: &[LensItem]) -> Result<LensHits, String> {
    let mut matchers: Vec<(&str, Matcher)> = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        matchers.push(match item {
            LensItem::Wallet { id, wallet } => (id, Matcher::Wallet(wallet.trim().to_string())),
            LensItem::Structure { id, row } => {
                let at = format!("items[{i}].row");
                let rows = std::slice::from_ref(row);
                IxShapeRows::validate(rows, &at)?;
                let shapes = IxShapeRows::parse(rows).ok_or_else(|| format!("{at} is malformed"))?;
                (id, Matcher::Shape(shapes))
            }
        });
    }
    let mut out = LensHits::new();
    let mut counts = vec![0u64; matchers.len()];
    for &ti in idxs {
        counts.fill(0);
        for t in tokens[ti].trades.iter() {
            let keys = &t.flow.labels;
            let side = if t.is_buy { Side::Buy } else { Side::Sell };
            for (n, (_, m)) in counts.iter_mut().zip(&matchers) {
                let hit = match m {
                    Matcher::Wallet(w) => t.wallet.as_deref() == Some(w.as_str()),
                    Matcher::Shape(s) => {
                        s.matches(keys.ix_hash, t.flow.fee) || s.matches_core(keys.core_hash, side, keys.core_marks, t.flow.fee)
                    }
                };
                *n += u64::from(hit);
            }
        }
        let coin: BTreeMap<String, u64> = matchers
            .iter()
            .zip(&counts)
            .filter(|(_, &n)| n > 0)
            .map(|((id, _), &n)| ((*id).to_string(), n))
            .collect();
        if !coin.is_empty() {
            out.insert(tokens[ti].mint.clone(), coin);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::{TimeZone, Utc};
    use hunter_engine::metrics::fee::FeeKeys;
    use hunter_engine::metrics::trade_keys::LabelKeys;
    use serde_json::json;

    use super::*;
    use crate::sweep::grouping::TokenFingerprint;
    use crate::sweep::projection::{CorpusTrade, FlowKeys};

    const PRIVATE: &[&str] = &["Compute Budget: SetComputeUnitLimit", "Private: ix#01", "Pump.Fun: Buy"];
    const PUBLIC: &[&str] = &["Pump.Fun: Buy"];

    fn tr(wallet: &str, labels: &[&str], is_buy: bool) -> CorpusTrade {
        CorpusTrade {
            block_time: Utc.timestamp_opt(1_760_000_000, 0).unwrap(),
            amount_sol: 1.0,
            token_amount: 1000.0,
            price_per_token: 1.0,
            reserve_sol: None,
            reserve_token: None,
            real_reserve_sol: None,
            real_token_reserves: None,
            slot: 1,
            tx_index: 0,
            leg_index: 0,
            is_buy,
            on_curve: true,
            venue_fee_bps: None,
            tx_signature: None,
            flow: FlowKeys { labels: LabelKeys::of(labels), wallet_hash: 0, fee: FeeKeys::new(Some(200_000), Some(1_000), None) },
            ix_labels: Some(serde_json::to_string(labels).unwrap().into()),
            wallet: Some(wallet.into()),
        }
    }

    fn tok(mint: &str, trades: Vec<CorpusTrade>) -> CorpusToken {
        CorpusToken {
            mint: mint.into(),
            symbol: mint.into(),
            created_at: Utc.timestamp_opt(1_760_000_000, 0).unwrap(),
            trades: Arc::new(trades),
            fp: TokenFingerprint::default(),
            identity: None,
        }
    }

    #[test]
    fn a_coin_lists_only_the_items_its_trades_match() {
        let tokens = vec![
            tok("a", vec![tr("dev", PRIVATE, true), tr("x", PUBLIC, true)]),
            tok("b", vec![tr("y", PUBLIC, true)]),
        ];
        let items = vec![
            LensItem::Structure { id: "s:exact".into(), row: json!(PRIVATE) },
            LensItem::Structure {
                id: "s:core".into(),
                row: json!({"labels": ["Private: ix#01", "Pump.Fun: Buy"], "level": "core", "side": "buy"}),
            },
            LensItem::Structure {
                id: "s:sell".into(),
                row: json!({"labels": ["Pump.Fun: Buy"], "level": "core", "side": "sell"}),
            },
            LensItem::Wallet { id: "w:y".into(), wallet: "y".into() },
        ];
        let h = hits(&tokens, &[0, 1], &items).unwrap();
        assert_eq!(h["a"], BTreeMap::from([("s:exact".into(), 1), ("s:core".into(), 1)]));
        assert_eq!(h["b"], BTreeMap::from([("w:y".into(), 1)]));
    }

    #[test]
    fn a_malformed_row_is_refused() {
        let items = vec![LensItem::Structure { id: "s".into(), row: json!(42) }];
        assert!(hits(&[], &[], &items).is_err());
    }
}
