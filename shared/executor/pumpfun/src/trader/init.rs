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
use tracing::info;

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

        // Fetch fee_recipient (offset 41) + the stable quote mint from chain in
        // one read. Layout (after the 8-byte discriminator): the
        // `whitelisted_quote_mints[0]` pubkey sits at byte 1013 — see the `Global`
        // struct in the pump IDL. Parsed best-effort: a shorter/older account or a
        // default (all-zero) slot just yields no stable mint (claim path skipped).
        const OFF_FEE_RECIPIENT: usize = 41;
        const OFF_STABLE_QUOTE_MINT: usize = 1013;
        let (fee_recipient, stable_quote_mint) = {
            let account = self
                .rpc
                .get_account(&global_pda)
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
            (fee_recipient, stable_quote_mint)
        };

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

    // -----------------------------------------------------------------------
    // Launch Address Lookup Table
    // -----------------------------------------------------------------------

    /// Fetch + deserialize the on-chain launch ALT into the `(key, addresses)`
    /// form v0 message compilation needs. The addresses are the immutable pump
    /// accounts pre-loaded by `create-alt` (see `crate::alt`); we copy them out of
    /// the borrowed `Cow` so the resolved table outlives the RPC response.
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
        Ok(AddressLookupTableAccount {
            key: *alt_address,
            addresses: table.addresses.to_vec(),
        })
    }
}
