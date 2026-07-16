// ============================================================
// Venue initialization — runs once before any buy/sell.
//
// The engine-side one-time work (connection warmup, rent reads, compute-budget
// ix building, Jito tip-floor prime + refresher, nonce parse + hash prefetch,
// blockhash prime + refresher) lives in `executor_core::Engine::initialize`.
// This file only performs the pump.fun VENUE steps on top: fetching the on-chain
// `Global` account and pre-building the buy-template seed pools.
// ============================================================

use super::{GlobalAccount, PumpFunTrader};
use crate::error::{bail, Context, Result};
use crate::protocol;
use crate::types::TokenProgram;
use solana_sdk::address_lookup_table::{state::AddressLookupTable, AddressLookupTableAccount};
use solana_sdk::pubkey::Pubkey;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tracing::info;

/// Process-wide cache of the pump `Global` account's wallet-INDEPENDENT read
/// fields (`fee_recipient`, `stable_quote_mint`) — the only on-chain read in
/// [`PumpFunTrader::fetch_global_account`]. They change only on rare pump
/// governance action, so re-reading them on every trader init is waste (forge
/// rebuilds a trader per manage leg). The wallet-SPECIFIC PDAs
/// (`user_volume_accumulator`) are always derived locally, never cached.
static GLOBAL_READ_CACHE: OnceLock<Mutex<Option<GlobalRead>>> = OnceLock::new();

#[derive(Clone, Copy)]
struct GlobalRead {
    fee_recipient: Pubkey,
    stable_quote_mint: Option<Pubkey>,
    fetched_at: Instant,
}

/// TTL for a cached `Global` read. Short enough that a fee-recipient rotation is
/// picked up quickly, long enough that a multi-leg manage action (trader-per-leg,
/// whole run in seconds) reads it once instead of once per leg.
const GLOBAL_READ_TTL: Duration = Duration::from_secs(60);

impl PumpFunTrader {
    // -----------------------------------------------------------------------
    // Initialization  (call once before any buy/sell)
    // -----------------------------------------------------------------------

    pub async fn initialize(&mut self) -> Result<()> {
        info!("🔧 Initializing PumpFunTrader...");

        // 1. Engine-side init: warmup, rent-exemption reads, compute-budget ixs,
        // Jito tip-floor prime + refresher, nonce parse + hash prefetch, and the
        // recent-blockhash prime + refresher.
        self.engine.initialize().await?;

        // 2. Global account (pump.fun `Global` PDA — fee recipient, volume
        // accumulators, fee config, stable cashback mint).
        self.global_account = Some(self.fetch_global_account().await?);
        info!("✅ Global account initialized");

        // 2b. Launch ALT (optional): resolve the configured table to its address
        // set once, so the create path can compile a v0 tx against it. A bad /
        // missing table address fails init loudly rather than silently falling
        // back to an oversized legacy tx at launch time.
        if let Some(alt_address) = self.config.launch_alt_address {
            self.launch_alt = Some(self.fetch_launch_alt(&alt_address).await?);
            info!(
                "✅ Launch ALT loaded: {} ({} addresses)",
                alt_address,
                self.launch_alt.as_ref().map(|a| a.addresses.len()).unwrap_or(0)
            );
        }

        // 3. Pre-build buy seed pools for both token programs (uses the engine's
        // rent values, read in step 1).
        info!(
            "🌱 Pre-building buy seed pools (target={})",
            self.config.limits.buy_seed_pool_size
        );
        self.fill_buy_pool(TokenProgram::Legacy).await?;
        self.fill_buy_pool(TokenProgram::Token2022).await?;
        info!("✅ Buy seed pools ready");

        info!(
            "🚀 PumpFunTrader ready — wallet: {}",
            self.config.signer.pubkey()
        );
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Global account
    // -----------------------------------------------------------------------

    async fn fetch_global_account(&self) -> Result<GlobalAccount> {
        let (global_pda, _) = Pubkey::find_program_address(&[b"global"], &protocol::PUMP_FUN);

        // Wallet-independent read (fee_recipient + stable_quote_mint), served from
        // a short-TTL process cache so repeated trader inits don't each pay a
        // `getAccountInfo`. The wallet-specific PDAs below are always derived local.
        let (fee_recipient, stable_quote_mint) = self.cached_global_read(&global_pda).await?;

        let (global_volume_accumulator, _) =
            Pubkey::find_program_address(&[b"global_volume_accumulator"], &protocol::PUMP_FUN);

        let wallet = self.config.signer.pubkey();
        let (user_volume_accumulator, _) = Pubkey::find_program_address(
            &[b"user_volume_accumulator", wallet.as_ref()],
            &protocol::PUMP_FUN,
        );

        let (fee_config, _) = Pubkey::find_program_address(
            &[b"fee_config", protocol::PUMP_FUN.as_ref()],
            &protocol::FEE_PROGRAM,
        );

        Ok(GlobalAccount {
            global_pda,
            fee_recipient,
            global_volume_accumulator,
            user_volume_accumulator,
            fee_config,
            stable_quote_mint,
        })
    }

    /// Read the pump `Global` account's wallet-independent fields
    /// (`fee_recipient` + `stable_quote_mint`) through [`GLOBAL_READ_CACHE`],
    /// hitting the RPC only on a cold/stale entry. Layout (after the 8-byte
    /// discriminator): `fee_recipient` at offset 41; `whitelisted_quote_mints[0]`
    /// at byte 1013 — see the `Global` struct in the pump IDL. `stable_quote_mint`
    /// is best-effort: a shorter/older account or a default (all-zero) slot yields
    /// `None` (the claim path is skipped).
    async fn cached_global_read(
        &self,
        global_pda: &Pubkey,
    ) -> Result<(Pubkey, Option<Pubkey>)> {
        let cell = GLOBAL_READ_CACHE.get_or_init(|| Mutex::new(None));

        // Fast path: a fresh cached read (fee_recipient is process-global, so any
        // trader's cached value is valid for every wallet).
        if let Ok(guard) = cell.lock() {
            if let Some(g) = *guard {
                if g.fetched_at.elapsed() <= GLOBAL_READ_TTL {
                    return Ok((g.fee_recipient, g.stable_quote_mint));
                }
            }
        }

        // Cold/stale: read on-chain, then refresh the cache.
        const OFF_FEE_RECIPIENT: usize = 41;
        const OFF_STABLE_QUOTE_MINT: usize = 1013;
        let account = self
            .rpc
            .get_account(global_pda)
            .await
            .context("Failed to fetch pump global account")?;
        if account.data.len() < OFF_FEE_RECIPIENT + 32 {
            bail!("Global account data too short");
        }
        let fee_recipient =
            Pubkey::try_from(&account.data[OFF_FEE_RECIPIENT..OFF_FEE_RECIPIENT + 32])
                .context("Failed to parse fee_recipient")?;
        let stable_quote_mint = account
            .data
            .get(OFF_STABLE_QUOTE_MINT..OFF_STABLE_QUOTE_MINT + 32)
            .and_then(|s| Pubkey::try_from(s).ok())
            .filter(|pk| *pk != Pubkey::default());

        if let Ok(mut guard) = cell.lock() {
            *guard = Some(GlobalRead {
                fee_recipient,
                stable_quote_mint,
                fetched_at: Instant::now(),
            });
        }
        Ok((fee_recipient, stable_quote_mint))
    }

    // -----------------------------------------------------------------------
    // Launch Address Lookup Table
    // -----------------------------------------------------------------------

    /// Fetch + deserialize the on-chain launch ALT into the `(key, addresses)`
    /// form v0 message compilation needs. The addresses are the immutable pump
    /// accounts pre-loaded by `create-alt` (see `crate::alt`); we copy them out of
    /// the borrowed `Cow` so the resolved table outlives the RPC response.
    ///
    /// Jito tip accounts are stripped from the resolved set even if an older ALT
    /// was provisioned with them: a tip referenced through an ALT is invisible to
    /// Jito's auction-eligibility write-lock check, so the bundle is rejected. With
    /// the tip account absent from the resolved addresses, v0 compilation leaves it
    /// an inline static key. This keeps a pre-existing on-chain table (tips at the
    /// tail — that's how `crate::alt` always appended them) working without a
    /// re-provision; the survivors keep their on-chain indices because we only ever
    /// drop a trailing suffix (guarded below).
    async fn fetch_launch_alt(&self, alt_address: &Pubkey) -> Result<AddressLookupTableAccount> {
        let account = self
            .rpc
            .get_account(alt_address)
            .await
            .with_context(|| format!("fetch launch ALT {alt_address}"))?;
        let table = AddressLookupTable::deserialize(&account.data)
            .map_err(|e| crate::error::TradeError::Other(format!("deserialize launch ALT: {e}")))?;
        if table.addresses.is_empty() {
            bail!("launch ALT {alt_address} is empty — run `create-alt` to populate it");
        }

        // v0 lookup indices are positions into THIS address vec and must match the
        // on-chain table, so we may only drop a trailing suffix. Guard that every
        // tip account sits after every retained (non-tip) address before filtering.
        let last_kept = table
            .addresses
            .iter()
            .rposition(|a| !protocol::JITO_TIP_ACCOUNTS.contains(a));
        if let Some(last_kept) = last_kept {
            if table.addresses[..last_kept]
                .iter()
                .any(|a| protocol::JITO_TIP_ACCOUNTS.contains(a))
            {
                bail!(
                    "launch ALT {alt_address} interleaves Jito tip accounts among launch \
                     accounts — indices would desync if filtered; re-provision with `create-alt`"
                );
            }
        }
        let addresses: Vec<Pubkey> = table
            .addresses
            .iter()
            .filter(|a| !protocol::JITO_TIP_ACCOUNTS.contains(a))
            .copied()
            .collect();

        Ok(AddressLookupTableAccount {
            key: *alt_address,
            addresses,
        })
    }
}
