//! Live LaserStream decode-parity + profile soak — **opt-in, `--ignored`**.
//!
//! Drives the real gRPC client ([`super::client::run`]) against live Helius
//! traffic and, for every received tx, decodes it BOTH ways — the protobuf-native
//! hot path ([`HeliusDecoder::decode_protobuf`]) and the trusted `Value` path
//! ([`super::adapter::update_tx_to_value`] → [`HeliusDecoder::decode_result`]) —
//! comparing the money-field fingerprints and timing each path. It asserts zero
//! divergence over a tx target. This is the off-line, re-runnable successor to
//! the temporary in-hot-path `parity.rs` soak (which decoded both ways on the
//! live pipeline): same guarantee, but no `DbWriter` / `StrategyRunner` / trader,
//! so it cannot place a trade, and zero cost on the production hot path.
//!
//! Run it (needs `HELIUS_LASERSTREAM_URL` + `HELIUS_API_KEY` in `.env`):
//! ```text
//! cargo test --bin backend -- --ignored --nocapture live_parity_soak
//! ```
//! Tunables (env): `PARITY_TX_TARGET` (default 3000 relevant txs), `PARITY_MAX_SECS`
//! (default 180s wall-clock cap). The subscription tracks only the pump.fun curve
//! program (empty pool set), so it covers curve buy/sell/create/migrate — the
//! paths where the two decoders' implementations genuinely diverge. AMM-live
//! parity is structurally guaranteed instead: both `decode_amm_live` and
//! `decode_amm_live_pb` build trades from the SAME shared leaves
//! (`decode_pump_swap_trades_from_logs` + `build_amm_trade`).

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use chrono::Utc;
    use dashmap::DashMap;
    use tokio::sync::{mpsc, watch, Notify};

    use super::super::adapter::update_tx_to_value;
    use super::super::client;
    use super::super::decoder::{DecodeOutput, HeliusDecoder};
    use crate::config::constants::PUMP_FUN_PROGRAM_ID;
    use crate::models::events::InternalEvent;

    /// Money-field fingerprints in decode (pre-sort) order — mint, wallet, side,
    /// amounts, leg, venue, instruction_type — excluding timestamps/ids that
    /// legitimately differ between the two ingest clocks. Identical in spirit to
    /// the synthetic parity tests' `fps` and the old `parity.rs::fingerprints`.
    fn fps(out: &DecodeOutput) -> Vec<String> {
        match out {
            DecodeOutput::Ignored => Vec::new(),
            DecodeOutput::Transaction { events, .. } => events.iter().map(event_fp).collect(),
        }
    }

    fn event_fp(ev: &InternalEvent) -> String {
        match ev {
            InternalEvent::TradeExecuted(e) => {
                let t = &e.trade;
                format!(
                    "trade|{}|{}|{:?}|{:.9}|{:.3}|{}|{}|{}",
                    t.mint_address,
                    t.wallet_address,
                    t.trade_type,
                    t.sol_amount,
                    t.token_amount,
                    t.leg_index,
                    t.venue,
                    t.instruction_type,
                )
            }
            InternalEvent::TokenCreated(e) => {
                let tk = &e.token;
                format!(
                    "create|{}|{}|{}|{}|{:?}",
                    tk.mint_address, tk.creator_wallet, tk.name, tk.symbol, tk.token_program_id,
                )
            }
            InternalEvent::TokenMigrated(e) => format!("migrate|{}", e.mint_address),
            InternalEvent::CreatorActivityDetected(e) => format!(
                "creator|{:?}|{}|{}",
                e.activity_kind, e.creator_wallet, e.mint_address,
            ),
            InternalEvent::LiquidityAdded(e) | InternalEvent::LiquidityRemoved(e) => {
                format!("liq|{}", e.mint_address)
            }
        }
    }

    #[derive(Default)]
    struct Coverage {
        trades: usize,
        creates: usize,
        migrates: usize,
        creators: usize,
        amm: usize,
    }

    impl Coverage {
        fn tally(&mut self, fps: &[String]) {
            for fp in fps {
                if let Some(rest) = fp.strip_prefix("trade|") {
                    self.trades += 1;
                    // venue is the 7th `|`-field; "amm" marks a PumpSwap leg.
                    if rest.split('|').nth(5) == Some("amm") {
                        self.amm += 1;
                    }
                } else if fp.starts_with("create|") {
                    self.creates += 1;
                } else if fp.starts_with("migrate|") {
                    self.migrates += 1;
                } else if fp.starts_with("creator|") {
                    self.creators += 1;
                }
            }
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "live soak: needs HELIUS_LASERSTREAM_URL + HELIUS_API_KEY; opens a real subscription"]
    async fn live_parity_soak() {
        dotenvy::dotenv().ok();
        let url = std::env::var("HELIUS_LASERSTREAM_URL")
            .expect("HELIUS_LASERSTREAM_URL required for the live parity soak");
        let key = std::env::var("HELIUS_API_KEY")
            .expect("HELIUS_API_KEY required for the live parity soak");
        let target: usize = std::env::var("PARITY_TX_TARGET")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3000);
        let max_secs: u64 = std::env::var("PARITY_MAX_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(180);

        // Ingest-only wiring: a channel into our decode loop, a pinned-live watch,
        // an empty pool set. No DbWriter / StrategyRunner / trader is constructed.
        let (update_tx, mut update_rx) = mpsc::channel(1024);
        let (_live_tx, live_rx) = watch::channel(true);
        let pool_index = Arc::new(DashMap::new());
        let pools_changed = Arc::new(Notify::new());

        let producer = tokio::spawn(client::run(
            url,
            key,
            PUMP_FUN_PROGRAM_ID.to_string(),
            update_tx,
            live_rx,
            pool_index.clone(),
            pools_changed,
            Duration::from_secs(2),
        ));

        let decoder =
            HeliusDecoder::new(PUMP_FUN_PROGRAM_ID.to_string()).with_pool_index(pool_index.clone());

        let mut received = 0usize;
        let mut decoded_nonempty = 0usize;
        let mut mismatches = 0usize;
        let mut pb_total = Duration::ZERO;
        let mut value_total = Duration::ZERO;
        let mut cov = Coverage::default();

        let deadline = tokio::time::Instant::now() + Duration::from_secs(max_secs);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                eprintln!("[soak] hit {max_secs}s wall-clock cap");
                break;
            }
            let update = match tokio::time::timeout(remaining, update_rx.recv()).await {
                Err(_) => {
                    eprintln!("[soak] hit {max_secs}s wall-clock cap");
                    break;
                }
                Ok(None) => {
                    eprintln!("[soak] producer channel closed");
                    break;
                }
                Ok(Some(u)) => u,
            };
            received += 1;

            // Protobuf hot path (timed).
            let now = Utc::now();
            let t0 = Instant::now();
            let pb_out = decoder.decode_protobuf(&update, now);
            pb_total += t0.elapsed();

            // Value path: build the blob + decode (timed) — the pre-Tier-B cost.
            let t1 = Instant::now();
            let value_out = match update_tx_to_value(&update, PUMP_FUN_PROGRAM_ID) {
                Some(v) => decoder.decode_result(v),
                None => DecodeOutput::Ignored,
            };
            value_total += t1.elapsed();

            let pb_fp = fps(&pb_out);
            let value_fp = fps(&value_out);

            if pb_fp != value_fp {
                mismatches += 1;
                let sig = update
                    .transaction
                    .as_ref()
                    .map(|i| bs58::encode(&i.signature).into_string())
                    .unwrap_or_default();
                eprintln!(
                    "[soak] PARITY MISMATCH sig={sig}\n  protobuf={pb_fp:?}\n  value   ={value_fp:?}"
                );
            }

            if !pb_fp.is_empty() {
                decoded_nonempty += 1;
                cov.tally(&pb_fp);
            }

            if decoded_nonempty >= target {
                eprintln!("[soak] reached tx target ({target})");
                break;
            }
        }

        drop(update_rx);
        producer.abort();

        let avg_us = |total: Duration, n: usize| {
            if n == 0 {
                0.0
            } else {
                total.as_secs_f64() * 1e6 / n as f64
            }
        };
        eprintln!(
            "\n[soak] ===== live decode parity + profile =====\n\
             received={received} decoded(non-empty)={decoded_nonempty} mismatches={mismatches}\n\
             coverage: trades={} (amm={}) creates={} migrates={} creators={}\n\
             avg decode_protobuf = {:.2} us/tx | avg Value-path (build+decode) = {:.2} us/tx\n\
             ==========================================",
            cov.trades,
            cov.amm,
            cov.creates,
            cov.migrates,
            cov.creators,
            avg_us(pb_total, received),
            avg_us(value_total, received),
        );

        assert!(
            received > 0,
            "no live txs received — check HELIUS_LASERSTREAM_URL / HELIUS_API_KEY / connectivity"
        );
        assert!(
            decoded_nonempty > 0,
            "received {received} txs but decoded zero events — pre-filter or decode is broken"
        );
        assert_eq!(
            mismatches, 0,
            "protobuf decode diverged from the Value decode on {mismatches} live tx(s)"
        );
    }
}
