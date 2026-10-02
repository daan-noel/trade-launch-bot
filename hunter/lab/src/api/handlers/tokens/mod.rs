//! Local token handlers: re-exports the core token handlers and adds the
//! local-only metric-series read.

pub use trading_core::api::handlers::tokens::*;

mod metric_series;

pub use metric_series::*;
