//! the **executor bridge**: turn a gated [`orchestrator::Plan`]'s ops
//! into real on-chain txs through an initialized `PumpFunTrader`.
//!
//! This is the "providers don't emit instructions (Phase C)" gap closed: each op
//! maps to the SAME proven pump ix builder the pre-cutover code used, so the
//! on-chain bytes are unchanged — only the *selection* of variant/amount/CU/tip now
//! comes from the catalog-validated, audited, disguised [`GatedPlan`] instead of a
//! free-text template string:
//!
//!   - a **bundler buy** leg → [`build_leg_tx`] → `trader.build_bundle_leg_tx`
//!     (unsent signed tx, submitted as a Jito bundle by the caller);
//!   - a **create** (+ fused dev-buy) → the launcher's existing `create_token*`
//!     path (see `service.rs`); create/dev-buy are one atomic tx on-chain, so the
//!     bridge doesn't split them;
//!   - a **`TransferSol`** op → [`execute_transfer`], the ONE plain-transfer
//!     primitive that replaces the three raw `system_instruction::transfer`
//!     bypasses (consolidate / dust-sweep / funding). No Jito — a SOL move has no
//!     landing urgency — but now every SOL move is a typed, auditable op.

use anyhow::{bail, Context, Result};
use orchestrator::{Amount, Disguise, Operation};
use pump_trader::types::TokenProgram;
use pump_trader::{BundleLegParams, IxLayout, PumpFunTrader};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::hash::Hash;
use solana_sdk::message::Message;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Signature, Signer};
use solana_sdk::system_instruction;
use solana_sdk::transaction::{Transaction, VersionedTransaction};

use crate::plan_pipeline::{bundle_buy_variant, DEFAULT_BUNDLE_SLIPPAGE_BPS};

/// The lamports a `TransferSol` op moves, resolved at send time.
#[derive(Debug, Clone, Copy)]
pub enum TransferMode {
    /// Move exactly this many lamports (funding — the treasury pays the fee).
    Exact(u64),
    /// Sweep the source to ~0: send `balance − fee`, skipping if the balance is at
    /// or below `min_lamports` or the fee (consolidate / dust-sweep). The op's
    /// authored `Amount::Sol` is the *estimate* the audit saw; the exact remainder
    /// is a send-time detail (the plan can't know the fee ahead of time).
    SweepAll { min_lamports: u64 },
}

/// Map a bundler-buy op + its drawn disguise → the per-leg params the Jito leg
/// builder consumes. Slippage comes from the op (late-bound min_out policy); CU
/// limit / price / tip come from the persona disguise (replacing the old per-field
/// recipe jitter). `min_tip_lamports` is this leg's share of the live-floor bundle
/// tip target ([`crate::bundle_execute`]): the disguise draw is only ever raised to
/// it, never lowered, so the persona jitter is preserved when it already clears the
/// auction and the tip is bid up to the live floor when it doesn't.
pub fn leg_params(op: &Operation, disguise: &Disguise, min_tip_lamports: u64) -> BundleLegParams {
    // Authored layout (hand-picked step order) rides on the op; absent ⇒ the
    // canonical buy shape. The gate already validated any authored layout.
    let layout = op
        .layout
        .clone()
        .map(|steps| IxLayout { steps })
        .unwrap_or_else(IxLayout::canonical_buy);
    BundleLegParams {
        slippage_bps: op.slippage_bps.unwrap_or(DEFAULT_BUNDLE_SLIPPAGE_BPS),
        cu_limit: disguise.cu_limit,
        cu_price: disguise.cu_price_micro_lamports,
        tip_lamports: disguise.tip_lamports.unwrap_or(0).max(min_tip_lamports),
        layout,
    }
}

/// The SOL (lamports) a buy op spends — a bundler/dev/volume buy is always an
/// `ExactQuote` (SOL-in) leg after the catalog cutover (the overflow-prone
/// tokens-out `ExactBase` encoding is never chosen for our own buys).
pub fn buy_lamports(op: &Operation) -> Result<u64> {
    match op.amount {
        Amount::ExactQuote(q) => Ok(q),
        other => bail!("buy op {} has non-SOL-in amount {:?}", op.id, other),
    }
}

/// Build one signed Jito bundle leg tx from a gated bundler-buy op. `signer` is the
/// bundler wallet (resolved by the caller from the op's managed wallet id).
/// `blockhash` must be shared across every leg in the same bundle submission. The
/// leg is a **v0** tx (compressed via the launch ALT when configured) so a v2
/// bundle-buy leg's ~27 accounts fit the per-tx limit Jito enforces on each leg.
#[allow(clippy::too_many_arguments)]
pub async fn build_leg_tx(
    trader: &PumpFunTrader,
    signer: &(dyn Signer + Send + Sync),
    blockhash: Hash,
    mint: &Pubkey,
    creator: &Pubkey,
    token_program: TokenProgram,
    cashback_enabled: bool,
    op: &Operation,
    disguise: &Disguise,
    min_tip_lamports: u64,
    reserves_override: Option<(u128, u128)>,
) -> Result<VersionedTransaction> {
    let variant = bundle_buy_variant(&op.variant)?;
    let lamports = buy_lamports(op)?;
    let params = leg_params(op, disguise, min_tip_lamports);
    trader
        .build_bundle_leg_tx(
            signer,
            blockhash,
            mint,
            creator,
            token_program,
            lamports,
            cashback_enabled,
            variant,
            &params,
            reserves_override,
        )
        .await
        .map_err(|e| anyhow::anyhow!("build bundle leg (op {}): {e}", op.id))
}

/// Execute a `TransferSol` op as a plain SOL transfer — the SSOT that replaces the
/// consolidate / dust-sweep / funding raw-transfer sites. `signer` is the source
/// wallet (it pays the fee and is the `from`); `to` is the op target. Returns the
/// signature **and the exact lamports moved** (`None` only when a `SweepAll` had
/// nothing worth sweeping) — the lamports let a caller report/reconcile the send
/// without re-deriving the probe-fee remainder itself.
///
/// `confirm` picks the send mode: `true` waits for confirmation (background funding
/// pass, consolidate, dust-sweep); `false` fire-and-forget (manual funding pass —
/// the balance poller promotes the wallet when the SOL lands).
pub async fn execute_transfer(
    rpc: &RpcClient,
    signer: &(dyn Signer + Send + Sync),
    from: Pubkey,
    to: Pubkey,
    mode: TransferMode,
    confirm: bool,
) -> Result<Option<(Signature, u64)>> {
    let blockhash = rpc.get_latest_blockhash().await.context("fetch blockhash")?;
    execute_transfer_with_blockhash(rpc, signer, from, to, mode, confirm, blockhash).await
}

/// [`execute_transfer`] over a **caller-supplied** blockhash — lets a batch pass
/// (e.g. the funding loop) fetch one recent blockhash and reuse it across many
/// transfers instead of a `getLatestBlockhash` per send. The blockhash must be
/// recent enough to land; the caller refreshes it periodically.
pub async fn execute_transfer_with_blockhash(
    rpc: &RpcClient,
    signer: &(dyn Signer + Send + Sync),
    from: Pubkey,
    to: Pubkey,
    mode: TransferMode,
    confirm: bool,
    blockhash: Hash,
) -> Result<Option<(Signature, u64)>> {
    let lamports = match mode {
        TransferMode::Exact(n) => n,
        TransferMode::SweepAll { min_lamports } => {
            let balance = rpc.get_balance(&from).await.context("fetch source balance")?;
            if balance <= min_lamports {
                return Ok(None);
            }
            // A transfer's lamports is a fixed-width u64 in the ix data, so a probe
            // message (any amount) has the exact serialized size — and fee — as the
            // final one. Send `balance − fee` so the source lands at exactly 0
            // (a sub-rent remainder would be rejected by the runtime).
            let probe_ix = system_instruction::transfer(&from, &to, balance);
            let probe_msg = Message::new_with_blockhash(&[probe_ix], Some(&from), &blockhash);
            let fee = rpc
                .get_fee_for_message(&probe_msg)
                .await
                .context("fetch sweep transfer fee")?;
            if balance <= fee {
                return Ok(None);
            }
            balance - fee
        }
    };

    let ix = system_instruction::transfer(&from, &to, lamports);
    let msg = Message::new_with_blockhash(&[ix], Some(&from), &blockhash);
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[signer as &dyn Signer], blockhash).context("sign transfer")?;

    let sig = if confirm {
        rpc.send_and_confirm_transaction(&tx)
            .await
            .context("send transfer")?
    } else {
        rpc.send_transaction(&tx).await.context("submit transfer")?
    };
    Ok(Some((sig, lamports)))
}
