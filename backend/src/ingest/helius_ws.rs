use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info, trace};

use crate::config::Settings;
use crate::ingest::subscription;

/// Spawn-ready entry point. Loops forever: connect → subscribe → read → reconnect.
/// Raw JSON text frames are forwarded to `raw_tx` for decoding.
pub async fn run(
    settings: Arc<Settings>,
    raw_tx: mpsc::Sender<String>,
    mut live_rx: watch::Receiver<bool>,
) {
    let subscribe_msg = subscription::build_subscribe_message(&settings);

    loop {
        while !*live_rx.borrow() {
            info!("Helius WS: live mode disabled, paused");
            if live_rx.changed().await.is_err() {
                return;
            }
        }

        match run_once(&settings, &raw_tx, &subscribe_msg, &mut live_rx).await {
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
    subscribe_msg: &str,
    live_rx: &mut watch::Receiver<bool>,
) -> anyhow::Result<()> {
    info!("Helius WS: connecting to {}", settings.helius_ws_url);

    let (ws_stream, _response) = connect_async(settings.helius_ws_url.as_str())
        .await
        .map_err(|e| anyhow::anyhow!("WS connect failed: {e}"))?;

    info!("Helius WS: connected");

    let (mut write, mut read) = ws_stream.split();

    // Send the subscription request immediately after connecting
    write
        .send(Message::Text(subscribe_msg.to_string()))
        .await
        .map_err(|e| anyhow::anyhow!("WS send subscription failed: {e}"))?;

    info!("Helius WS: subscription sent");

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
