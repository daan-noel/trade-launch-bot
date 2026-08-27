//! Shared no-DB fixtures for the family-search tests. Every constructed test in
//! [family-search.md] §8 names these, at the same bar as
//! [rule-search-method.md] §5: no lake, no Postgres, no network.
//!
//! [family-search.md]: ../../../docs/roadmap/family-search.md
//! [rule-search-method.md]: ../../../docs/plans/strategies/rule-search-method.md

use std::sync::Arc;

use chrono::{DateTime, Duration, TimeZone, Utc};

use hunter_engine::metrics::evaluator::Operator;
use hunter_engine::metrics::MetricId;
use trading_core::strategies::kernel::{CostModel, ExitCode};
use trading_core::strategies::paper_fill::FillModel;

use crate::sweep::corpus::CorpusToken;
use crate::sweep::generic::Pricing;
use crate::sweep::projection::{CorpusTrade, FlowKeys};
use crate::sweep::strategy::TokenOutcome;

/// The charter's own conditions: 0.01 SOL, `WorstCase` fill, `pumpfun_impact` — the
/// only cost model whose cost responds to buy size, so an oracle priced through it is
/// charged what a real round trip is charged.
pub fn pricing() -> Pricing {
    Pricing {
        buy_amount_sol: 0.01,
        fill_model: FillModel::WorstCase,
        cost: CostModel::pumpfun_with_impact(),
    }
}

pub fn created_at() -> DateTime<Utc> {
    Utc.timestamp_opt(1_700_000_000, 0).unwrap()
}

/// One priced print. The reserve **pair** carries the spot (that is what
/// `chart_spot_price` reads); `real_reserve_sol` is held constant so the impact
/// charge is the same on every row and a test's arithmetic stays legible.
pub fn priced_trade(secs: i64, price: f64) -> CorpusTrade {
    CorpusTrade {
        block_time: created_at() + Duration::seconds(secs),
        amount_sol: 1.0,
        token_amount: 1.0,
        // Deliberately NOT the spot: the execution price is a different number, and
        // a fixture that made them equal would hide a reader using the wrong one.
        price_per_token: price * 0.5,
        reserve_sol: Some(price * 1_000.0),
        reserve_token: Some(1_000.0),
        real_reserve_sol: Some(40.0),
        real_token_reserves: None,
        slot: secs as u64,
        leg_index: 0,
        is_buy: true,
        tx_signature: None,
        flow: FlowKeys::default(),
        ix_labels: None,
        wallet: None,
    }
}

/// A token whose `chart_spot_price` walks `prices`, one print per second.
/// Oracle-free by default — call `.with_oracle()` for the loads that ask for it.
pub fn token_from_prices(prices: &[f64]) -> CorpusToken {
    let trades: Vec<CorpusTrade> =
        prices.iter().enumerate().map(|(i, p)| priced_trade(i as i64, *p)).collect();
    CorpusToken {
        mint: "mint".into(),
        symbol: "SYM".into(),
        created_at: created_at(),
        trades: Arc::new(trades),
        fp: Default::default(),
        identity: None,
        peak_after: None,
    }
}

/// A fired outcome with the entry stamped — the shape the oracle reads.
pub fn outcome_at(entry_at: DateTime<Utc>, entry_price: f64, pnl_sol: f64) -> TokenOutcome {
    TokenOutcome {
        fired: true,
        holding_secs: 10,
        pnl_percent: 0.0,
        pnl_sol: pnl_sol as f32,
        exit: ExitCode::TimeStop,
        exit_metric: None,
        exit_operator: None,
        exit_metric_value: None,
        exit_metric_window: None,
        exit_metric_slot: None,
        entry_time: Some(entry_at),
        entry_price: Some(entry_price),
        entry_slot: None,
        exit_time: Some(entry_at + Duration::seconds(10)),
        exit_price: Some(entry_price),
        exit_slot: None,
    }
}

/// A close on an authored exit term, stamped exactly as the engine stamps it.
pub fn metric_exit(
    slot: u8,
    metric: MetricId,
    operator: Operator,
    value: f64,
    window: Option<hunter_engine::metrics::WindowSpec>,
    pnl_sol: f64,
) -> TokenOutcome {
    TokenOutcome {
        exit: ExitCode::Metrics,
        exit_metric: Some(metric),
        exit_operator: Some(operator),
        exit_metric_value: Some(value),
        exit_metric_window: window,
        exit_metric_slot: Some(slot),
        ..outcome_at(created_at(), 1.0, pnl_sol)
    }
}

/// Stamp the round trip's two fill prices on an outcome. The realized level column
/// reads `exit ÷ entry − 1`, so a fixture that left them equal could never show a
/// close landing past its own threshold.
pub fn filled_at(o: TokenOutcome, entry_price: f64, exit_price: f64) -> TokenOutcome {
    TokenOutcome { entry_price: Some(entry_price), exit_price: Some(exit_price), ..o }
}

/// A close that is not an authored term — the "other" bucket.
pub fn tp_exit(pnl_sol: f64) -> TokenOutcome {
    TokenOutcome { exit: ExitCode::TakeProfit, ..outcome_at(created_at(), 1.0, pnl_sol) }
}

/// A `fingerprints` row shaped like the charter's reference family:
/// `3ix:BuyExactSolIn · bkt=exact`. A caller varies exactly one axis off this base to
/// build siblings — every other axis stays put, which is what makes them siblings.
pub fn fp_row(name: &str) -> trading_core::models::Fingerprint {
    trading_core::models::Fingerprint {
        wildcard: false,
        id: uuid::Uuid::new_v4(),
        name: name.into(),
        cu_limit: Some(200_000),
        cu_price: Some(50),
        init_buy_lamports: Some(trading_core::config::constants::sol_to_lamports(0.5)),
        max_cost_lamports: None,
        spendable_lamports_in: None,
        first_slot_buy_lamports: None,
        first_slot_sell_lamports: None,
        // `bkt=exact` — a NULL width is an exact compare, not an unset one.
        bucket_size_amount: None,
        ix_labels: Some(vec![
            "Pump.Fun: CreateIdempotent".into(),
            "Pump.Fun: Create".into(),
            "Pump.Fun: BuyExactSolIn".into(),
        ]),
        metric_config: serde_json::json!({}),
        created_at: created_at(),
        updated_at: created_at(),
    }
}
