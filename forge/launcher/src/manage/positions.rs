//! The holdings read model: seed a mint's positions from its launch/bundle fills,
//! then reconcile each against chain (balance + canonical token account) and the
//! ingested feed (realized proceeds). Cold, operator-triggered path.

use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context, Result};
use platform_core::models::{
    BundleStatus, LaunchStatus, PositionStatus, PositionView, TokenPosition, WalletFill,
    WalletRole, WalletStatus,
};
use platform_core::storage::repositories::{
    BundleRepo, LaunchRepo, ManagedWalletRepo, TokenMarketStateRepo, TokenPositionRepo, TradeRepo,
};
use pump_trader::protocol;
use solana_sdk::pubkey::Pubkey;
use sqlx::PgPool;
use tracing::warn;

use crate::bundle::legs_from_json;
use crate::config::LauncherSettings;

/// Load the per-wallet holdings for a launched mint. Seeds any missing cost-basis
/// rows from the launch's dev buy + bundle legs (idempotent), then — when RPC is
/// configured — reconciles each row's on-chain balance + feed realized proceeds
/// best-effort (a reconcile failure is logged, never fatal: seeded rows still
/// return).
pub async fn load_positions(
    pool: &PgPool,
    settings: Option<&LauncherSettings>,
    mint_address: &str,
) -> Result<Vec<TokenPosition>> {
    seed_positions(pool, mint_address).await?;

    if let Some(settings) = settings {
        if let Err(e) = reconcile_positions(pool, settings, mint_address).await {
            warn!(%mint_address, ?e, "position reconcile failed — returning seeded rows");
        }
    }

    TokenPositionRepo::by_mint(pool, mint_address).await
}

/// Read for the operator holdings view: seed (idempotent) + a **feed-derived**
/// reconcile — balance, cost basis, and realized proceeds all replayed from the
/// ingested `trades`, **zero RPC**. The GET `/positions` handler serves this
/// directly (DB-only, no network), so opening a token page / an SSE refetch never
/// touches an RPC. On-chain truth (external transfers the feed can't see, or a
/// missed feed leg) is corrected only on the explicit "Refresh" action, which
/// runs the RPC path [`reconcile_positions`]. The sell/preview paths still
/// reconcile against chain inline (they must size a real-SOL sell off on-chain
/// balances) via [`load_positions`].
pub async fn read_positions(pool: &PgPool, mint_address: &str) -> Result<Vec<TokenPosition>> {
    seed_positions(pool, mint_address).await?;
    reconcile_positions_feed(pool, mint_address).await?;
    TokenPositionRepo::by_mint(pool, mint_address).await
}

/// Price holdings rows for the API: each row's value + PnL at the mint's current
/// spot ([`TokenPosition::with_pnl`], the one PnL implementation), one market-state
/// read for the whole set.
pub async fn position_views(
    pool: &PgPool,
    mint_address: &str,
    rows: Vec<TokenPosition>,
) -> Result<Vec<PositionView>> {
    let price = TokenMarketStateRepo::get(pool, mint_address)
        .await?
        .and_then(|s| s.current_price_quote);
    Ok(rows.into_iter().map(|p| p.with_pnl(price)).collect())
}

/// Seed cost-basis rows for the dev wallet + every bundle leg of a mint's launch.
/// Idempotent, so it's safe to call on every read. No-op when the mint isn't one
/// of our launches.
async fn seed_positions(pool: &PgPool, mint_address: &str) -> Result<()> {
    let Some(launch) = LaunchRepo::find_by_mint(pool, mint_address).await? else {
        return Ok(());
    };

    let bundle = match launch.bundle_id {
        Some(bundle_id) => BundleRepo::get(pool, bundle_id).await?,
        None => None,
    };

    // The dev-buy is fused into the create tx (`tx0` of the atomic bundle, or the
    // standalone create tx on a bundler-less launch), so it shares the launch's fate:
    // it bought iff the launch landed. Seed the dev position exactly like a co-buy
    // leg — `open` on a landed outcome, `dropped` on a terminal drop/failure, and
    // nothing while still in flight (a later read seeds the correct row once the
    // confirm watcher resolves it). Seeding it unconditionally `open` was the bug: a
    // failed launch showed a phantom "dev buy: open" with a nonzero cost basis and a
    // zero balance.
    if let Some(dev_wallet_id) = launch.dev_wallet_id {
        match launch_bought(&launch.status, bundle.as_ref().map(|b| b.status.as_str())) {
            Some(true) => {
                TokenPositionRepo::seed(
                    pool,
                    mint_address,
                    dev_wallet_id,
                    WalletRole::Dev.as_str(),
                    launch.dev_buy_quote.unwrap_or(0),
                )
                .await?;
            }
            Some(false) => {
                TokenPositionRepo::seed_dropped(
                    pool,
                    mint_address,
                    dev_wallet_id,
                    WalletRole::Dev.as_str(),
                )
                .await?;
            }
            None => {} // still in flight — don't seed yet
        }
    }

    if let Some(bundle) = &bundle {
        // A Jito bundle is atomic: `landed`/`partial` ⇒ every present leg bought; any
        // other TERMINAL outcome (dropped/failed) ⇒ no leg bought, so those wallets
        // hold nothing and spent nothing — seed them `dropped` at zero cost (not a
        // phantom closed buy). While the bundle is still non-terminal
        // (planned/submitting/submitted) we can't yet tell, so skip seeding and let a
        // later read (once the confirm watcher resolves it) seed the correct row —
        // `seed`/`seed_dropped` are DO-NOTHING idempotent, so the first terminal read
        // wins and sticks.
        match bundle.status.parse::<BundleStatus>() {
            Ok(BundleStatus::Landed) | Ok(BundleStatus::Partial) => {
                for leg in legs_from_json(&bundle.legs)? {
                    TokenPositionRepo::seed(
                        pool,
                        mint_address,
                        leg.managed_wallet_id,
                        WalletRole::Bundler.as_str(),
                        leg.quote_amount,
                    )
                    .await?;
                }
            }
            Ok(BundleStatus::Dropped) | Ok(BundleStatus::Failed) => {
                for leg in legs_from_json(&bundle.legs)? {
                    TokenPositionRepo::seed_dropped(
                        pool,
                        mint_address,
                        leg.managed_wallet_id,
                        WalletRole::Bundler.as_str(),
                    )
                    .await?;
                }
            }
            // Non-terminal (or an unrecognized status) — don't seed yet.
            _ => {}
        }
    }

    Ok(())
}

/// Did the launch's create (and its fused dev-buy) actually land? The dev-buy rides
/// the create tx, which — for an atomic launch — is `tx0` of the bundle, so the
/// bundle's terminal outcome is authoritative; a bundler-less launch has no bundle,
/// so the launch's own status is. `Some(true)` = landed (seed the dev cost basis
/// `open`), `Some(false)` = terminally dropped/failed (seed `dropped`), `None` = still
/// in flight (don't seed yet). Takes the raw status strings so it's a pure decision
/// the unit tests can pin without a full `Launch`/`Bundle` fixture.
fn launch_bought(launch_status: &str, bundle_status: Option<&str>) -> Option<bool> {
    if let Some(bundle_status) = bundle_status {
        return match bundle_status.parse::<BundleStatus>() {
            Ok(BundleStatus::Landed) | Ok(BundleStatus::Partial) => Some(true),
            Ok(BundleStatus::Dropped) | Ok(BundleStatus::Failed) => Some(false),
            _ => None,
        };
    }
    match launch_status.parse::<LaunchStatus>() {
        Ok(LaunchStatus::Created) => Some(true),
        Ok(LaunchStatus::Failed) => Some(false),
        _ => None,
    }
}

/// The reconcile probe set: `owner_address -> (managed_wallet_id, role)`. Every
/// non-retired managed wallet (holdings discovery) UNION every existing
/// non-`dropped` position owner (so a sold-out row on a since-retired wallet still
/// reconciles to `closed`). A `dropped` leg never bought — never probe or seed it.
/// Shared by the feed read path and the RPC refresh path so the two can't drift.
async fn build_probe_set(
    pool: &PgPool,
    existing: &[TokenPosition],
) -> Result<HashMap<String, (uuid::Uuid, String)>> {
    let mut probe: HashMap<String, (uuid::Uuid, String)> = HashMap::new();
    for w in ManagedWalletRepo::list_all(pool, None).await? {
        if w.status == WalletStatus::Retired.as_str() {
            continue;
        }
        probe.insert(w.address, (w.id, w.role));
    }
    for p in existing {
        if p.status == PositionStatus::Dropped.as_str() {
            continue;
        }
        probe
            .entry(p.wallet_address.clone())
            .or_insert((p.managed_wallet_id, p.role.clone()));
    }
    Ok(probe)
}

/// Feed-only reconcile — the default holdings read path. Replays each probed
/// wallet's ingested fills to derive its CURRENT balance (`held_base`), lot-reset
/// cost basis, and realized proceeds, then writes them in ONE `UNNEST` batch.
/// **No RPC**: the balance comes from `buys − sells` in `trades`, not the chain, so
/// serving a token page never issues a network call. A wallet whose fills the feed
/// has ingested (present in the replay) is written; a freshly-seeded row with no
/// fills yet (the ingest-lag window right after a launch/buy) is skipped, keeping
/// its seed values until the fill lands. `token_account` is left untouched (the
/// feed can't know the canonical ATA — only the RPC path fills it, and only the
/// sell path needs it). The two things the feed can't see — an external
/// wallet→wallet transfer, or a dropped feed leg — are corrected by the operator
/// "Refresh" action via [`reconcile_positions`].
pub async fn reconcile_positions_feed(pool: &PgPool, mint_address: &str) -> Result<()> {
    let existing = TokenPositionRepo::by_mint(pool, mint_address).await?;
    let existing_owners: std::collections::HashSet<String> =
        existing.iter().map(|p| p.wallet_address.clone()).collect();

    let probe = build_probe_set(pool, &existing).await?;
    if probe.is_empty() {
        return Ok(());
    }
    let probe_addrs: Vec<String> = probe.keys().cloned().collect();

    // Each wallet's CURRENT lot (held tokens, SOL paid in, SOL returned), replayed
    // from its fills.
    let lots =
        lots_by_address(&TradeRepo::fills_for_mint_wallets(pool, mint_address, &probe_addrs).await?);

    // Discovery: a wallet the feed shows holding the mint with no position row yet
    // (an out-of-band manage-buy) gets one seeded at cost 0 so it's visible +
    // sellable. Feed-derived, so discovery costs no RPC either.
    for (addr, lot) in &lots {
        if lot.held_base > 0 && !existing_owners.contains(addr) {
            if let Some((wid, role)) = probe.get(addr) {
                if let Err(e) = TokenPositionRepo::seed(pool, mint_address, *wid, role, 0).await {
                    warn!(%addr, ?e, "seed feed-discovered holder failed — skipping");
                }
            }
        }
    }

    // Re-read so freshly-seeded discovery rows carry an id, then write every row the
    // feed has fills for in ONE batch. A row with no fills yet is skipped (kept at
    // its seed values through the ingest-lag window), not zeroed.
    let positions = TokenPositionRepo::by_mint(pool, mint_address).await?;
    let mut ids = Vec::new();
    let mut balances = Vec::new();
    let mut token_accounts: Vec<Option<String>> = Vec::new();
    let mut realized: Vec<Option<i64>> = Vec::new();
    let mut cost_quote: Vec<Option<i64>> = Vec::new();
    for pos in &positions {
        if pos.status == PositionStatus::Dropped.as_str() {
            continue;
        }
        let Some(lot) = lots.get(&pos.wallet_address) else {
            continue;
        };
        ids.push(pos.id);
        balances.push(lot.held_base);
        token_accounts.push(None); // feed can't know the ATA; leave it (COALESCE)
        realized.push(Some(lot.received_quote));
        cost_quote.push(Some(lot.paid_quote));
    }

    TokenPositionRepo::reconcile_batch(pool, &ids, &balances, &token_accounts, &realized, &cost_quote)
        .await
}

/// Reconcile a mint's positions against **chain** + feed, **discovering** any
/// managed wallet that holds the mint but was never seeded a row. This is the ONLY
/// RPC path for holdings — reached from the operator "Refresh" action and the
/// sell/preview sizing paths, never from a plain page read (that's the feed-only
/// [`reconcile_positions_feed`]). Public so the sell executor can refresh
/// positions immediately after a sell.
///
/// The probe set is every non-retired managed wallet (discovery) UNION every
/// existing non-`dropped` position owner (so a sold-out row on a since-retired
/// wallet still reconciles to `closed`). Balances are read in ONE batched sweep:
/// each owner's canonical ATA is derived deterministically (the mint's token
/// program comes from the launch `variant` — Legacy SPL for `create_v1`,
/// Token-2022 otherwise) and read via `getMultipleAccounts` (100 accounts/call).
/// So a whole-pool probe is `ceil(N/100)` RPC calls, not `N` per-wallet
/// `getTokenAccountsByOwner` calls — full discovery stays cheap. A wallet found
/// holding the mint with no position row gets one seeded (cost basis 0 — the row
/// exists only to make the holding visible + sellable) before the batch balance
/// write, so "sell all" is correct for a manual manage-buy or any out-of-band
/// holding, not just the launch dev/bundle wallets.
///
/// Shape: every wallet's lot (paid + returned SOL) comes from the feed in one
/// mint-scoped query (not a per-position N+1); balances come from `ceil(N/100)`
/// batched account reads; and every reconciled row is written in ONE `UNNEST`
/// batch update. Cold, operator-triggered path — never run it on an ingest hot
/// path.
pub async fn reconcile_positions(
    pool: &PgPool,
    settings: &LauncherSettings,
    mint_address: &str,
) -> Result<()> {
    let existing = TokenPositionRepo::by_mint(pool, mint_address).await?;
    let existing_owners: std::collections::HashSet<String> =
        existing.iter().map(|p| p.wallet_address.clone()).collect();

    let probe = build_probe_set(pool, &existing).await?;
    if probe.is_empty() {
        return Ok(());
    }

    // Each probed wallet's CURRENT lot from the feed (mint + wallet scoped): the SOL
    // paid in and returned, on the PnL basis of `TokenPosition`. Only wallets the
    // feed has seen appear; the rest keep their stored figures (reconcile_batch
    // COALESCEs a `None`) so a fresh seed isn't zeroed during the ingest-lag window.
    // Lags one ingest cycle behind a just-fired trade; the next read picks it up.
    let probe_addrs: Vec<String> = probe.keys().cloned().collect();
    let lots =
        lots_by_address(&TradeRepo::fills_for_mint_wallets(pool, mint_address, &probe_addrs).await?);

    // The mint's token program picks the ATA derivation (Legacy for `create_v1`,
    // Token-2022 otherwise) — resolved from the launch we're reconciling.
    let token_program = match LaunchRepo::find_by_mint(pool, mint_address).await? {
        Some(l) if l.variant.contains("create_v1") => protocol::TOKEN,
        _ => protocol::TOKEN_2022,
    };
    let mint_pk =
        Pubkey::from_str(mint_address).with_context(|| format!("parse mint {mint_address}"))?;

    // Derive each probed owner's canonical ATA, then read them all in one batched
    // sweep. A malformed owner address is logged + skipped (never aborts the pass).
    let mut probed: Vec<(String, uuid::Uuid, String, String)> = Vec::new(); // (owner, wid, role, ata)
    for (owner, (wid, role)) in &probe {
        let owner_pk = match Pubkey::from_str(owner) {
            Ok(p) => p,
            Err(e) => {
                warn!(%owner, ?e, "bad managed-wallet address — skipping");
                continue;
            }
        };
        let ata = derive_ata(&owner_pk, &token_program, &mint_pk).to_string();
        probed.push((owner.clone(), *wid, role.clone(), ata));
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .context("build reqwest client for position reconcile")?;
    let atas: Vec<String> = probed.iter().map(|(_, _, _, ata)| ata.clone()).collect();
    let balances = fetch_ata_balances_batched(&client, &settings.rpc_url, &atas).await?;

    // Seed a row for any DISCOVERED holder (holds the mint, no existing row) so the
    // batch update below has an id to write. `token_account` is the derived ATA.
    let mut holdings: HashMap<String, (i64, Option<String>)> = HashMap::new();
    for ((owner, wid, role, ata), balance_base) in probed.into_iter().zip(balances) {
        if balance_base > 0 && !existing_owners.contains(&owner) {
            if let Err(e) = TokenPositionRepo::seed(pool, mint_address, wid, &role, 0).await {
                warn!(%owner, ?e, "seed discovered holder position failed — skipping");
                continue;
            }
        }
        holdings.insert(owner, (balance_base, Some(ata)));
    }

    // Re-read so freshly-seeded discovery rows carry an id, then write every
    // non-dropped row we have a fresh balance for in ONE `UNNEST` batch update.
    let positions = TokenPositionRepo::by_mint(pool, mint_address).await?;
    let mut ids = Vec::new();
    let mut balances = Vec::new();
    let mut token_accounts: Vec<Option<String>> = Vec::new();
    let mut realized: Vec<Option<i64>> = Vec::new();
    let mut cost_quote: Vec<Option<i64>> = Vec::new();
    for pos in &positions {
        if pos.status == PositionStatus::Dropped.as_str() {
            continue;
        }
        let Some((balance_base, token_account)) = holdings.get(&pos.wallet_address) else {
            continue;
        };
        ids.push(pos.id);
        balances.push(*balance_base);
        token_accounts.push(token_account.clone());
        // `None` when the feed has no fills for this wallet yet — keep the stored lot.
        let lot = lots.get(&pos.wallet_address);
        realized.push(lot.map(|l| l.received_quote));
        cost_quote.push(lot.map(|l| l.paid_quote));
    }

    TokenPositionRepo::reconcile_batch(pool, &ids, &balances, &token_accounts, &realized, &cost_quote)
        .await
}

/// One wallet's CURRENT lot of a mint (all exact base units): tokens held, SOL
/// paid into the lot, SOL its sells returned — the PnL basis documented on
/// [`TokenPosition`]. A lot opens on the first buy after the balance reached zero;
/// a closed lot keeps its figures until then, so a sold-out row still shows what
/// it made.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Lot {
    held_base: i64,
    paid_quote: i64,
    received_quote: i64,
}

impl Lot {
    fn buy(&mut self, base: i64, quote: i64) {
        if self.held_base == 0 && (self.paid_quote != 0 || self.received_quote != 0) {
            *self = Lot::default(); // the prior lot closed at zero; this buy opens anew
        }
        self.held_base = self.held_base.saturating_add(base);
        self.paid_quote = self.paid_quote.saturating_add(quote);
    }

    /// A sell past the held balance (a feed gap or an external transfer-in) floors
    /// the balance at 0, never negative.
    fn sell(&mut self, base: i64, quote: i64) {
        self.held_base = self.held_base.saturating_sub(base).max(0);
        self.received_quote = self.received_quote.saturating_add(quote);
    }

    /// Book one transaction's legs for this wallet. With the tx's wallet flow
    /// (`payer_net_lamports`) the tx is ONE fill: its net token change, priced at
    /// what the wallet actually moved (fees, tip and venue fee included, counted
    /// once however many legs the tx has). Without it each leg books its
    /// `amount_quote`.
    fn apply_tx(&mut self, legs: &[&WalletFill]) {
        let Some(net) = legs.iter().find_map(|f| f.payer_net_lamports) else {
            for f in legs {
                if f.trade_type == "buy" {
                    self.buy(f.amount_base, f.amount_quote);
                } else {
                    self.sell(f.amount_base, f.amount_quote);
                }
            }
            return;
        };
        let side_base = |side: &str| -> i64 {
            legs.iter().filter(|f| f.trade_type == side).map(|f| f.amount_base).sum()
        };
        let (bought, sold) = (side_base("buy"), side_base("sell"));
        if bought >= sold && bought > 0 {
            self.buy(bought - sold, -net);
        } else {
            self.sell(sold - bought, net);
        }
    }
}

/// Replay every wallet's fills into its CURRENT [`Lot`]. `fills` is in canonical
/// order with one tx's legs adjacent (the query's `ORDER BY slot, tx_index,
/// tx_signature, leg_index`); wallets may interleave, so each tx is split by
/// wallet. Only wallets present in `fills` appear (callers keep the rest's stored
/// figures).
fn lots_by_address(fills: &[WalletFill]) -> HashMap<String, Lot> {
    let mut lots: HashMap<String, Lot> = HashMap::new();
    for tx in fills.chunk_by(|a, b| a.tx_signature == b.tx_signature) {
        let mut wallets: Vec<&str> = Vec::new();
        for f in tx {
            if !wallets.contains(&f.address.as_str()) {
                wallets.push(&f.address);
            }
        }
        for wallet in wallets {
            let legs: Vec<&WalletFill> = tx.iter().filter(|f| f.address == wallet).collect();
            lots.entry(wallet.to_string()).or_default().apply_tx(&legs);
        }
    }
    lots
}

/// The canonical associated token account: `PDA([owner, token_program, mint],
/// ATA_PROGRAM)`. Deterministic — so a whole pool's balances read in one batched
/// `getMultipleAccounts` sweep instead of a per-owner `getTokenAccountsByOwner`.
fn derive_ata(owner: &Pubkey, token_program: &Pubkey, mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[owner.as_ref(), token_program.as_ref(), mint.as_ref()],
        &protocol::ASSOCIATED_TOKEN_PROGRAM,
    )
    .0
}

/// Read many token accounts' balances in ONE `getMultipleAccounts` call per 100
/// pubkeys (the RPC cap), preserving the input order. A never-created / non-token
/// account comes back `null` (or without a parsed `tokenAmount`) ⇒ 0. Balances are
/// exact base units.
async fn fetch_ata_balances_batched(
    client: &reqwest::Client,
    rpc_url: &str,
    atas: &[String],
) -> Result<Vec<i64>> {
    const BATCH: usize = 100; // getMultipleAccounts caps at 100 pubkeys per call
    let mut out = Vec::with_capacity(atas.len());
    for chunk in atas.chunks(BATCH) {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getMultipleAccounts",
            "params": [chunk, { "encoding": "jsonParsed", "commitment": "confirmed" }],
        });
        let v: serde_json::Value = client
            .post(rpc_url)
            .json(&body)
            .send()
            .await
            .context("getMultipleAccounts HTTP")?
            .error_for_status()
            .context("getMultipleAccounts HTTP status")?
            .json()
            .await
            .context("parse getMultipleAccounts body")?;
        if let Some(err) = v.get("error") {
            anyhow::bail!("getMultipleAccounts RPC error: {err}");
        }
        let accounts = v
            .get("result")
            .and_then(|r| r.get("value"))
            .and_then(|value| value.as_array())
            .context("getMultipleAccounts response missing result.value")?;
        for acc in accounts {
            // `null` account (never created) or a non-token account (no parsed
            // tokenAmount) ⇒ 0. `amount` is a decimal string in base units.
            let amount = acc
                .pointer("/data/parsed/info/tokenAmount/amount")
                .and_then(|a| a.as_str())
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0);
            out.push(amount);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{launch_bought, lots_by_address, Lot};
    use platform_core::models::WalletFill;

    /// Regression (dev-buy phantom `open`): the dev-buy is fused into the create
    /// (`tx0`), so a terminally dropped/failed atomic bundle means the dev never
    /// bought — `launch_bought` MUST report `Some(false)` so the dev position is
    /// seeded `dropped`, not a phantom `open` at a nonzero cost with a zero balance.
    #[test]
    fn launch_bought_reflects_bundle_outcome() {
        // Atomic launch: the bundle's terminal outcome is authoritative.
        assert_eq!(launch_bought("pending", Some("landed")), Some(true));
        assert_eq!(launch_bought("pending", Some("partial")), Some(true));
        assert_eq!(launch_bought("pending", Some("dropped")), Some(false));
        assert_eq!(launch_bought("pending", Some("failed")), Some(false));
        // Still in flight — can't tell yet, so don't seed either way.
        assert_eq!(launch_bought("pending", Some("submitted")), None);
        assert_eq!(launch_bought("pending", Some("planned")), None);
    }

    /// A bundler-less launch has no bundle, so its own status decides.
    #[test]
    fn launch_bought_bundlerless_uses_launch_status() {
        assert_eq!(launch_bought("created", None), Some(true));
        assert_eq!(launch_bought("failed", None), Some(false));
        assert_eq!(launch_bought("pending", None), None);
    }

    fn fill(addr: &str, side: &str, quote: i64, base: i64) -> WalletFill {
        // One tx per fill unless a test groups legs by signature.
        static SEQ: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        WalletFill {
            address: addr.to_string(),
            trade_type: side.to_string(),
            amount_quote: quote,
            amount_base: base,
            tx_signature: vec![n],
            payer_net_lamports: None,
        }
    }

    fn lot(held_base: i64, paid_quote: i64, received_quote: i64) -> Lot {
        Lot { held_base, paid_quote, received_quote }
    }

    /// Paid and received cover the same lot; a partial sell leaves `paid` whole.
    #[test]
    fn partial_sell_books_proceeds_against_the_lot() {
        let fills = vec![
            fill("A", "buy", 20, 1_000), // hold 1000, paid 20
            fill("A", "sell", 6, 400),   // hold 600, received 6
        ];
        assert_eq!(lots_by_address(&fills)["A"], lot(600, 20, 6));
    }

    /// A sold-out lot keeps its figures (its PnL stays visible); the next buy opens
    /// a fresh lot, so a re-buy shows only its own cost, never the prior lot's
    /// proceeds against it.
    #[test]
    fn closed_lot_keeps_figures_until_a_rebuy_opens_a_new_one() {
        let closed = vec![fill("A", "buy", 20, 1_000), fill("A", "sell", 25, 1_000)];
        assert_eq!(lots_by_address(&closed)["A"], lot(0, 20, 25));
        let mut rebuy = closed.clone();
        rebuy.push(fill("A", "buy", 10, 500));
        assert_eq!(lots_by_address(&rebuy)["A"], lot(500, 10, 0));
    }

    /// Over-selling (sell base > held) floors the balance at 0, never negative.
    #[test]
    fn oversell_floors_balance_at_zero() {
        let fills = vec![fill("A", "buy", 20, 1_000), fill("A", "sell", 30, 1_500)];
        assert_eq!(lots_by_address(&fills)["A"], lot(0, 20, 30));
    }

    /// Wallets are keyed independently even when their fills interleave.
    #[test]
    fn interleaved_wallets_are_independent() {
        let fills = vec![
            fill("A", "buy", 20, 1_000),
            fill("B", "buy", 5, 200),
            fill("A", "buy", 10, 500),
            fill("B", "sell", 5, 200),
        ];
        let lots = lots_by_address(&fills);
        assert_eq!(lots["A"], lot(1_500, 30, 0));
        assert_eq!(lots["B"], lot(0, 5, 5));
    }

    /// With the wallet flow on the row, a tx books what the wallet moved (fee, tip
    /// and venue fee included), once per signature however many legs it has.
    #[test]
    fn wallet_flow_books_once_per_signature() {
        let leg = |side: &str, base: i64, quote: i64, net: i64| WalletFill {
            address: "A".to_string(),
            trade_type: side.to_string(),
            amount_quote: quote,
            amount_base: base,
            tx_signature: vec![200],
            payer_net_lamports: Some(net),
        };
        // Two buy legs in one tx: 1_000 curve-side each, the wallet paid 2_060.
        let buy = vec![leg("buy", 500, 1_000, -2_060), leg("buy", 500, 1_000, -2_060)];
        assert_eq!(lots_by_address(&buy)["A"], lot(1_000, 2_060, 0));
        // A sell the wallet netted 1_900 from (proceeds after fees).
        let mut sell = buy.clone();
        sell.push(WalletFill { tx_signature: vec![201], ..leg("sell", 400, 2_000, 1_900) });
        assert_eq!(lots_by_address(&sell)["A"], lot(600, 2_060, 1_900));
    }

    /// Rows without the wallet flow (older rows, a backfill without balances) book
    /// their `amount_quote` leg by leg.
    #[test]
    fn rows_without_wallet_flow_fall_back_to_amount_quote() {
        let fills = vec![fill("A", "buy", 1_000, 500), fill("A", "sell", 300, 100)];
        assert_eq!(lots_by_address(&fills)["A"], lot(400, 1_000, 300));
    }
}
