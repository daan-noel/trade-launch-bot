use std::collections::HashSet;
use std::sync::Arc;

use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, watch, Notify};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info, trace};

use crate::config::Settings;
use crate::ingest::subscription;

/// Max pool accounts per `accountInclude` array. Helius caps how many accounts a
/// single subscription may list, so large pool sets are chunked across several
/// subscription messages on the same connection.
const POOLS_PER_SUB: usize = 90;

/// The write half of the Helius WebSocket, after `split()`.
type WsSink = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    Message,
>;

/// Spawn-ready entry point. Loops forever: connect → subscribe → read → reconnect.
/// Raw JSON text frames are forwarded to `raw_tx` for decoding.
///
/// `pool_index` (pool → mint, populated by the pipeline for migrated tokens)
/// drives the dynamic PumpSwap pool subscriptions; `pools_changed` is pinged
/// whenever a new token migrates so we can subscribe to its pool without
/// dropping the connection.
pub async fn run(
    settings: Arc<Settings>,
    raw_tx: mpsc::Sender<String>,
    mut live_rx: watch::Receiver<bool>,
    pool_index: Arc<DashMap<String, String>>,
    pools_changed: Arc<Notify>,
) {
    loop {
        while !*live_rx.borrow() {
            info!("Helius WS: live mode disabled, paused");
            if live_rx.changed().await.is_err() {
                return;
            }
        }

        match run_once(&settings, &raw_tx, &mut live_rx, &pool_index, &pools_changed).await {
            Ok(()) => info!("Helius WS: connection closed gracefully"),
            Err(e) => error!("Helius WS: connection error — {e}"),
        }

        info!(
            "Helius WS: reconnecting in {:?}",
            settings.reconnect_interval
        );
        tokio::time::sleep(settings.reconnect_interval).await;
    }
}

/// Single connection attempt: connect, subscribe, read until error or close.
async fn run_once(
    settings: &Settings,
    raw_tx: &mpsc::Sender<String>,
    live_rx: &mut watch::Receiver<bool>,
    pool_index: &DashMap<String, String>,
    pools_changed: &Notify,
) -> anyhow::Result<()> {
    info!("Helius WS: connecting to {}", settings.helius_ws_url);

    let (ws_stream, _response) = connect_async(settings.helius_ws_url.as_str())
        .await
        .map_err(|e| anyhow::anyhow!("WS connect failed: {e}"))?;

    info!("Helius WS: connected");

    let (mut write, mut read) = ws_stream.split();

    // 1) Static subscription on the bonding-curve program: token discovery,
    //    curve trades, and migrations. id = 1.
    write
        .send(Message::Text(subscription::build_subscribe_message(
            settings,
            1,
            &[settings.pump_program_id.as_str()],
        )))
        .await
        .map_err(|e| anyhow::anyhow!("WS send curve subscription failed: {e}"))?;

    // 2) Dynamic subscriptions on the specific PumpSwap pools of migrated tokens.
    //    `subscribed` tracks what this connection already covers so a later
    //    `pools_changed` ping only subscribes to the delta — never double-
    //    subscribing (which would duplicate messages).
    let mut next_id: u64 = 2;
    let mut subscribed: HashSet<String> = HashSet::new();
    let pools: Vec<String> = pool_index.iter().map(|e| e.key().clone()).collect();
    next_id = send_pool_subscriptions(&mut write, settings, &pools, next_id).await?;
    subscribed.extend(pools);

    info!(
        "Helius WS: subscriptions sent (curve + {} pool(s))",
        subscribed.len()
    );

    // Send pings starting one interval after connection (not immediately)
    let ping_start = tokio::time::Instant::now() + settings.ping_interval;
    let mut ping_interval = tokio::time::interval_at(ping_start, settings.ping_interval);

    loop {
        tokio::select! {
            _ = live_rx.changed() => {
                if !*live_rx.borrow() {
                    info!("Helius WS: live mode disabled, closing websocket");
                    return Ok(());
                }
            }

            _ = pools_changed.notified() => {
                // A token migrated — subscribe to any pools this connection
                // doesn't yet cover. Diffing against `subscribed` keeps it
                // idempotent across coalesced notifications and stale wakeups.
                let new_pools: Vec<String> = pool_index
                    .iter()
                    .filter_map(|e| {
                        let pool = e.key();
                        (!subscribed.contains(pool)).then(|| pool.clone())
                    })
                    .collect();
                if !new_pools.is_empty() {
                    next_id =
                        send_pool_subscriptions(&mut write, settings, &new_pools, next_id).await?;
                    info!(
                        "Helius WS: subscribed to {} newly-migrated pool(s)",
                        new_pools.len()
                    );
                    subscribed.extend(new_pools);
                }
            }

            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        // If the consumer is gone, stop the WS loop cleanly
                        if raw_tx.send(text).await.is_err() {
                            info!("Helius WS: raw_tx receiver dropped — shutting down");
                            return Ok(());
                        }
                    }
                    Some(Ok(Message::Ping(data))) => {
                        // Respond to server-initiated pings
                        let _ = write.send(Message::Pong(data)).await;
                    }
                    Some(Ok(Message::Close(frame))) => {
                        info!("Helius WS: server closed connection — {:?}", frame);
                        return Ok(());
                    }
                    Some(Err(e)) => {
                        return Err(anyhow::anyhow!("WS read error: {e}"));
                    }
                    None => return Ok(()),
                    // Binary / Pong — ignore
                    _ => {}
                }
            }

            _ = ping_interval.tick() => {
                trace!("Helius WS: sending keepalive ping");
                write
                    .send(Message::Ping(vec![]))
                    .await
                    .map_err(|e| anyhow::anyhow!("WS ping failed: {e}"))?;
            }
        }
    }
}

/// Send one `transactionSubscribe` per chunk of up to [`POOLS_PER_SUB`] pool
/// accounts. Returns the next free request id.
async fn send_pool_subscriptions(
    write: &mut WsSink,
    settings: &Settings,
    pools: &[String],
    mut next_id: u64,
) -> anyhow::Result<u64> {
    for chunk in pools.chunks(POOLS_PER_SUB) {
        let refs: Vec<&str> = chunk.iter().map(|s| s.as_str()).collect();
        let msg = subscription::build_subscribe_message(settings, next_id, &refs);
        write
            .send(Message::Text(msg))
            .await
            .map_err(|e| anyhow::anyhow!("WS send pool subscription failed: {e}"))?;
        next_id += 1;
    }
    Ok(next_id)
}
