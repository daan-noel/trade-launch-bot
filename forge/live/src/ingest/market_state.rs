//! Pure coalescer: one ingest flush's trades (+ migration events) → one
//! [`MarketStateDelta`] per mint, so the writer folds a whole batch into
//! `token_market_state` with ONE upsert instead of a round trip per trade.
//!
//! No DB, no network — unit-tested below. The DB merge rules (price watermark,
//! rising ATH, additive totals, sticky migration) live in
//! [`TokenMarketStateRepo::apply_deltas`](platform_core::storage::repositories::TokenMarketStateRepo::apply_deltas).

use std::collections::{HashMap, HashSet};

use platform_core::models::{spot_price_quote, MarketStateDelta, NewTrade};
use platform_core::venue::MarketKind;

/// Coalesce `rows` (the flush's mapped trades, any order) and `migrated` (mints a
/// `TokenMigrated` event named) into one delta per mint.
///
/// - price = spot of the mint's LATEST row in canonical order `(slot, tx_index,
///   leg_index)`, from its post-trade reserves;
/// - ATH = highest spot across the mint's rows, stamped with that row's `block_time`;
/// - volume / count cover only rows in `landed` (the PK-deduped inserts), so a
///   replayed trade adds nothing;
/// - `is_migrated` when any row traded on the AMM or the mint is in `migrated`.
pub fn coalesce(
    rows: &[NewTrade],
    landed: &HashSet<(Vec<u8>, i16)>,
    migrated: &[String],
) -> Vec<MarketStateDelta> {
    let amm = MarketKind::Amm.as_str();
    // mint → (delta, canonical order key of the row currently priced).
    let mut by_mint: HashMap<&str, (MarketStateDelta, (i64, i32, i16))> = HashMap::new();
    for r in rows {
        let order = (r.slot, r.tx_index, r.leg_index);
        let (d, latest) = by_mint.entry(r.mint_address.as_str()).or_insert_with(|| {
            (
                MarketStateDelta { mint_address: r.mint_address.clone(), ..Default::default() },
                (i64::MIN, i32::MIN, i16::MIN),
            )
        });
        let spot = spot_price_quote(r.reserve_quote, r.reserve_base);
        if order >= *latest {
            *latest = order;
            d.current_price_quote = spot;
            d.last_trade_at = Some(r.block_time);
        }
        if let Some(p) = spot {
            if d.ath_price_quote.is_none_or(|a| p > a) {
                d.ath_price_quote = Some(p);
                d.ath_at = Some(r.block_time);
            }
        }
        if landed.contains(&(r.tx_signature.clone(), r.leg_index)) {
            d.volume_quote = d.volume_quote.saturating_add(r.amount_quote.max(0));
            d.trade_count += 1;
        }
        if r.market_kind == amm {
            d.is_migrated = true;
        }
    }

    let mut out: Vec<MarketStateDelta> = by_mint.into_values().map(|(d, _)| d).collect();
    for mint in migrated {
        match out.iter_mut().find(|d| &d.mint_address == mint) {
            Some(d) => d.is_migrated = true,
            None => out.push(MarketStateDelta {
                mint_address: mint.clone(),
                is_migrated: true,
                ..Default::default()
            }),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn row(mint: &str, slot: i64, leg: i16, rq: i64, rb: i64, kind: &str) -> NewTrade {
        NewTrade {
            mint_address: mint.to_string(),
            wallet_ref: 1,
            launchpad_id: 1,
            market_kind: kind.to_string(),
            quote_asset_id: 1,
            trade_type: "buy".to_string(),
            amount_quote: 100,
            amount_base: 10,
            reserve_quote: Some(rq),
            reserve_base: Some(rb),
            slot,
            tx_index: 0,
            leg_index: leg,
            block_time: Utc.timestamp_opt(1_700_000_000 + slot, 0).unwrap(),
            tx_signature: vec![slot as u8, leg as u8],
        }
    }

    fn keys(rows: &[NewTrade]) -> HashSet<(Vec<u8>, i16)> {
        rows.iter().map(|r| (r.tx_signature.clone(), r.leg_index)).collect()
    }

    /// The price is the spot of the canonically-latest row even when the batch
    /// arrives out of order; the ATH is the highest spot with its own timestamp.
    #[test]
    fn price_follows_latest_row_and_ath_the_peak() {
        let rows = vec![
            row("M", 12, 0, 3_000, 100, "bonding_curve"), // spot 30, latest
            row("M", 10, 0, 1_000, 100, "bonding_curve"), // spot 10
            row("M", 11, 0, 5_000, 100, "bonding_curve"), // spot 50, peak
        ];
        let d = coalesce(&rows, &keys(&rows), &[]);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].current_price_quote, Some(30.0));
        assert_eq!(d[0].last_trade_at, Some(rows[0].block_time));
        assert_eq!(d[0].ath_price_quote, Some(50.0));
        assert_eq!(d[0].ath_at, Some(rows[2].block_time));
        assert_eq!(d[0].trade_count, 3);
        assert_eq!(d[0].volume_quote, 300);
        assert!(!d[0].is_migrated);
    }

    /// A replayed row (absent from `landed`) still prices but adds no volume/count.
    #[test]
    fn replayed_rows_add_no_volume() {
        let rows = vec![row("M", 10, 0, 1_000, 100, "bonding_curve")];
        let d = coalesce(&rows, &HashSet::new(), &[]);
        assert_eq!(d[0].current_price_quote, Some(10.0));
        assert_eq!((d[0].trade_count, d[0].volume_quote), (0, 0));
    }

    /// An AMM trade or a migration event marks the mint migrated; a migration for
    /// a mint with no trades in the batch is its own delta.
    #[test]
    fn amm_trade_and_migration_event_mark_migrated() {
        let rows = vec![row("A", 10, 0, 1_000, 100, "amm"), row("B", 10, 1, 1_000, 100, "bonding_curve")];
        let d = coalesce(&rows, &keys(&rows), &["C".to_string(), "B".to_string()]);
        let get = |m: &str| d.iter().find(|x| x.mint_address == m).unwrap();
        assert!(get("A").is_migrated);
        assert!(get("B").is_migrated);
        assert!(get("C").is_migrated);
        assert_eq!(get("C").current_price_quote, None);
        assert_eq!(get("C").last_trade_at, None);
    }
}
