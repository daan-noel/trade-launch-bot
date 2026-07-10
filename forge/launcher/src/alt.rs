//! CLI: provision the persistent launch Address Lookup Table.
//!
//! `create-alt <authority_key_ref>` creates one on-chain ALT and populates it
//! with the immutable pump.fun accounts a launch tx references (SSOT:
//! `pump_trader::launch_alt_addresses`). Print the table address and set it as
//! `PUMP_LAUNCH_ALT` in `.env` — the launcher then compiles create + dev-buy as a
//! v0 tx against it, so a `create_v2` + dev-buy fits the 1232 B tx limit it
//! otherwise overflows.
//!
//! This spends real SOL: ~0.0009 SOL rent for the table account plus signature
//! fees for the create + extend txs. `authority_key_ref` is BOTH the payer and the
//! ALT authority (an existing keystore blob — e.g. the treasury wallet); keep the
//! authority safe, it's what could later close/modify the table.

use anyhow::{bail, Context, Result};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::address_lookup_table::instruction::{create_lookup_table, extend_lookup_table};
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::message::Message;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signer;
use solana_sdk::transaction::Transaction;

use crate::config::LauncherSettings;
use crate::keystore::{self, EnvKek};

/// `extend_lookup_table` addresses per tx. Each address is 32 B; a legacy tx caps
/// at 1232 B, so ~30 fit alongside the ix overhead. 20 is a comfortable margin —
/// the full immutable set (~28 addresses) takes two extends.
const EXTEND_CHUNK: usize = 20;

/// `create-alt <authority_key_ref>` — create + populate the launch ALT and print
/// its address. Idempotency is NOT attempted: each run derives a fresh table from
/// the current slot, so re-running mints a NEW table (set `PUMP_LAUNCH_ALT` to the
/// latest). The old one can be left dormant or closed via a Solana CLI.
pub async fn run_create_alt(settings: &LauncherSettings, args: &[String]) -> Result<()> {
    let key_ref = args
        .first()
        .context("usage: create-alt <authority_key_ref>")?;
    if args.len() > 1 {
        bail!("usage: create-alt <authority_key_ref>");
    }

    let kek = EnvKek::from_passphrase(&settings.kek_passphrase);
    let signer = keystore::resolve_signer(&settings.keystore_dir, key_ref, &kek)?;
    let authority = signer.pubkey();
    let signer_ref = signer.as_ref();

    let rpc =
        RpcClient::new_with_commitment(settings.rpc_url.clone(), CommitmentConfig::confirmed());

    let addresses = pump_trader::launch_alt_addresses();
    println!(
        "Provisioning launch ALT: authority/payer = {authority}, {} addresses",
        addresses.len()
    );

    // 1. Create the table. The derivation seeds on a recent (finalized) slot; the
    //    program rejects a slot it can't find in the slot-hashes sysvar, so use a
    //    finalized one rather than the bleeding edge.
    let recent_slot = rpc
        .get_slot_with_commitment(CommitmentConfig::finalized())
        .await
        .context("fetch recent slot for ALT derivation")?;
    let (create_ix, alt_address) = create_lookup_table(authority, authority, recent_slot);
    send_signed(&rpc, &[create_ix], &authority, signer_ref, "create ALT").await?;
    println!("✅ ALT account created: {alt_address}");

    // 2. Extend with the immutable address set, chunked to fit the tx size limit.
    for (i, chunk) in addresses.chunks(EXTEND_CHUNK).enumerate() {
        let ix = extend_lookup_table(alt_address, authority, Some(authority), chunk.to_vec());
        send_signed(
            &rpc,
            &[ix],
            &authority,
            signer_ref,
            &format!("extend ALT (chunk {})", i + 1),
        )
        .await?;
        println!("  extended +{} addresses", chunk.len());
    }

    println!("\n✅ Launch ALT ready with {} addresses.", addresses.len());
    println!("   Set in forge/.env:\n     PUMP_LAUNCH_ALT={alt_address}");
    println!("   Then restart forge-live. The table is usable one slot after this — no wait needed for a manual relaunch.");
    Ok(())
}

/// Build, sign, and confirm a single-payer legacy tx from `ixs`. Fetches a fresh
/// blockhash per call so a slow confirm on an earlier step can't expire a later
/// one. `label` names the step in any error surfaced.
async fn send_signed(
    rpc: &RpcClient,
    ixs: &[solana_sdk::instruction::Instruction],
    payer: &Pubkey,
    signer: &(dyn Signer + Send + Sync),
    label: &str,
) -> Result<()> {
    let blockhash = rpc
        .get_latest_blockhash()
        .await
        .with_context(|| format!("{label}: fetch blockhash"))?;
    let msg = Message::new_with_blockhash(ixs, Some(payer), &blockhash);
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[signer as &dyn Signer], blockhash)
        .with_context(|| format!("{label}: sign"))?;
    let sig = rpc
        .send_and_confirm_transaction(&tx)
        .await
        .with_context(|| format!("{label}: send+confirm"))?;
    println!("  {label}: {sig}");
    Ok(())
}
