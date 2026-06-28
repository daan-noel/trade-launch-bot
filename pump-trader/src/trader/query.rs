// ============================================================
// Read-only queries — NOT on the trade hot path.
//
//  - get_all_token_accounts:   list all non-zero wallet holdings (raw JSON-RPC).
//  - get_token_balance:        on-chain SPL balance for a wallet + mint.
//  - get_creator_from_mint_pda: read the bonding-curve PDA to find the creator,
//                               and cache all sell-side PDAs for the mint.
// ============================================================

use super::{PumpFunTrader, TokenPDAs};
use crate::error::{bail, Result, TradeError};
use crate::protocol::{TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID};
use solana_sdk::pubkey::Pubkey;
use spl_associated_token_account::get_associated_token_address_with_program_id;
use std::collections::HashMap;
use std::str::FromStr;

/// Routing + PDA-seed facts parsed from a mint's bonding-curve and mint
/// accounts in a single `getMultipleAccounts` round-trip. Shared by
/// [`PumpFunTrader::resolve_buy_routing`] and
/// [`PumpFunTrader::get_creator_from_mint_pda`] so the read and the offset
/// parsing live in exactly one place.
#[derive(Clone, Copy)]
pub(super) struct CurveRouting {
    creator: Pubkey,
    token_program: Pubkey,
    /// Bonding-curve `complete` flag — true once migrated to the AMM.
    is_migrated: bool,
    /// create_v2 cashback flag (bonding-curve account offset 82).
    cashback_enabled: bool,
}

impl PumpFunTrader {
    /// Fetch the on-chain lamport balance of the trader's wallet. Intended for the
    /// background SOL-balance refresh task; never called inline on the buy hot path.
    pub async fn get_sol_balance(&self) -> Result<u64> {
        let wallet = self.config.signer.pubkey();
        Ok(self.rpc.get_balance(&wallet).await?)
    }

    /// Return all non-zero token accounts held by the trader's wallet.
    /// Uses raw JSON-RPC via reqwest to avoid extra Solana SDK dependencies.
    pub async fn get_all_token_accounts(
        &self,
    ) -> Result<Vec<crate::types::WalletHolding>> {
        let wallet = self.wallet_pubkey();
        let rpc_url = self.rpc_url();

        let req = |prog: &str| {
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "getTokenAccountsByOwner",
                "params": [
                    wallet.clone(),
                    { "programId": prog },
                    { "encoding": "jsonParsed", "commitment": "confirmed" }
                ]
            })
        };

        // The classic-Token and Token-2022 scans are independent reads. Fire
        // both concurrently (reusing the trader's shared HTTP client for
        // connection-pool / TLS reuse) rather than sequentially, halving the
        // wallet-scan latency on the page-load / refresh path.
        let (spl, t22) = tokio::join!(
            async {
                let resp: serde_json::Value = self
                    .http
                    .post(rpc_url)
                    .json(&req(TOKEN_PROGRAM_ID))
                    .send()
                    .await?
                    .json()
                    .await?;
                Ok::<serde_json::Value, TradeError>(resp)
            },
            async {
                let resp: serde_json::Value = self
                    .http
                    .post(rpc_url)
                    .json(&req(TOKEN_2022_PROGRAM_ID))
                    .send()
                    .await?
                    .json()
                    .await?;
                Ok::<serde_json::Value, TradeError>(resp)
            },
        );

        let mut holdings: Vec<crate::types::WalletHolding> = Vec::new();
        for (prog, resp) in [(TOKEN_PROGRAM_ID, spl?), (TOKEN_2022_PROGRAM_ID, t22?)] {
            let Some(accounts) = resp["result"]["value"].as_array() else {
                continue;
            };

            for account in accounts {
                let info = &account["account"]["data"]["parsed"]["info"];
                let mint = match info["mint"].as_str() {
                    Some(m) if !m.is_empty() => m.to_string(),
                    _ => continue,
                };
                let ta = &info["tokenAmount"];
                let amount: u64 = ta["amount"].as_str().unwrap_or("0").parse().unwrap_or(0);
                if amount == 0 {
                    continue;
                }
                let ui_amount = ta["uiAmount"].as_f64().unwrap_or(0.0);
                let decimals = ta["decimals"].as_u64().unwrap_or(0) as u8;
                let token_account = account["pubkey"].as_str().unwrap_or("").to_string();

                holdings.push(crate::types::WalletHolding {
                    mint,
                    amount,
                    ui_amount,
                    decimals,
                    token_account,
                    token_program_id: prog.to_string(),
                });
            }
        }

        holdings.sort_by(|a, b| {
            b.ui_amount
                .partial_cmp(&a.ui_amount)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(holdings)
    }

    /// Return the wallet's holding for a single `mint`, or `None` if not held.
    ///
    /// A single `getTokenAccountsByOwner` call with a **mint filter** — no full
    /// wallet scan and no ATA derivation. The mint belongs to exactly one token
    /// program, so the response covers both classic Token and Token-2022 (the
    /// owning program is read back from `account.owner`). Used to confirm a
    /// balance change after a manual trade cheaply: a not-yet-created ATA simply
    /// returns `None` until the buy lands.
    pub async fn get_token_account_for_mint(
        &self,
        mint: &str,
    ) -> Result<Option<crate::types::WalletHolding>> {
        let wallet = self.wallet_pubkey();
        let rpc_url = self.rpc_url();
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getTokenAccountsByOwner",
            "params": [
                wallet,
                { "mint": mint },
                { "encoding": "jsonParsed", "commitment": "confirmed" }
            ]
        });

        let resp: serde_json::Value = self
            .http
            .post(rpc_url)
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        let Some(accounts) = resp["result"]["value"].as_array() else {
            return Ok(None);
        };

        for account in accounts {
            let info = &account["account"]["data"]["parsed"]["info"];
            let ta = &info["tokenAmount"];
            let amount: u64 = ta["amount"].as_str().unwrap_or("0").parse().unwrap_or(0);
            if amount == 0 {
                continue;
            }
            let ui_amount = ta["uiAmount"].as_f64().unwrap_or(0.0);
            let decimals = ta["decimals"].as_u64().unwrap_or(0) as u8;
            let token_account = account["pubkey"].as_str().unwrap_or("").to_string();
            // The owning token program (classic vs Token-2022) is the account
            // owner, not derivable from the mint filter alone.
            let token_program_id = account["account"]["owner"].as_str().unwrap_or("").to_string();

            return Ok(Some(crate::types::WalletHolding {
                mint: mint.to_string(),
                amount,
                ui_amount,
                decimals,
                token_account,
                token_program_id,
            }));
        }
        Ok(None)
    }

    /// Resolve the wallet's token account for `mint` with at most one on-chain
    /// lookup. Serves from the in-memory cache when the account is already known
    /// (populated by a prior buy/sell on this trader), otherwise performs a
    /// single **mint-filtered** `getTokenAccountsByOwner` and caches the result.
    /// Returns `None` when the wallet holds no account for the mint.
    ///
    /// Hot-path callers (e.g. the TPSL sell retry loop) use this so repeated
    /// attempts don't each re-query — the lookup happens once and every later
    /// attempt hits the cache (zero RPC).
    pub async fn resolve_cached_token_account(
        &self,
        mint: &str,
    ) -> Result<Option<Pubkey>> {
        if let Some(pk) = self.user_token_accounts.get(mint).map(|r| *r) {
            return Ok(Some(pk));
        }
        // Cache miss: a single mint-filtered lookup for just this mint, not a full
        // two-program wallet scan. The hot path only needs this mint's account, so
        // resolving it directly is one cheap RPC regardless of how many other
        // tokens the wallet holds. Other mints warm their own cache entry on their
        // first miss. On-chain is the source of truth, so overwrite any stale entry.
        let Some(holding) = self.get_token_account_for_mint(mint).await? else {
            return Ok(None);
        };
        let pk = Pubkey::from_str(&holding.token_account)?;
        self.user_token_accounts.insert(mint.to_string(), pk);
        Ok(Some(pk))
    }

    /// Read the bonding curve's virtual reserves — `(virtual_token, virtual_quote)`
    /// in raw units (quote = lamports) — for slippage quoting on the curve path.
    /// Layout after the 8-byte Anchor discriminator: `virtual_token_reserves` @8,
    /// `virtual_quote_reserves` @16 (both u64 LE).
    pub(crate) async fn curve_virtual_reserves(
        &self,
        bonding_curve: &Pubkey,
    ) -> Result<(u128, u128)> {
        let acct = self
            .rpc
            .get_account(bonding_curve)
            .await
            .map_err(|e| TradeError::Other(format!("read bonding curve {bonding_curve}: {e}")))?;
        let d = &acct.data;
        if d.len() < 24 {
            bail!("bonding curve account too short: {} bytes", d.len());
        }
        let vt = u64::from_le_bytes(d[8..16].try_into().unwrap()) as u128;
        let vq = u64::from_le_bytes(d[16..24].try_into().unwrap()) as u128;
        if vt == 0 || vq == 0 {
            bail!("bonding curve has zero virtual reserves");
        }
        Ok((vt, vq))
    }

    /// Curve virtual reserves with a WS-cache fast path: serve a fresh cached
    /// snapshot for `mint` (same `(virtual_token, virtual_quote=lamports)` units
    /// as [`curve_virtual_reserves`]) when one is available, otherwise read the
    /// bonding-curve account on-chain. Used for curve buy/sell slippage quoting.
    pub(crate) async fn curve_reserves(
        &self,
        mint: &str,
        bonding_curve: &Pubkey,
    ) -> Result<(u128, u128)> {
        if let Some(r) = self.reserve_cache.get_fresh(
            mint,
            std::time::Duration::from_millis(self.config.cache.reserve_max_age_ms),
            false,
        ) {
            return Ok(r);
        }
        self.curve_virtual_reserves(bonding_curve).await
    }

    /// Query the on-chain SPL token balance for a given wallet + mint.
    /// Tries both classic Token and Token-2022 ATAs; returns 0 if none found.
    pub async fn get_token_balance(
        &self,
        wallet: &str,
        mint: &str,
    ) -> Result<crate::types::TokenBalance> {
        // FIX: don't derive ATA — look up actual on-chain account. Serve from
        // the cache, or do a single wallet scan (which also warms the cache for
        // every other held mint).
        let token_account_pk = match self.resolve_cached_token_account(mint).await? {
            Some(pk) => pk,
            None => {
                // Truly not found
                return Ok(crate::types::TokenBalance {
                    mint: mint.to_string(),
                    wallet: wallet.to_string(),
                    amount: 0,
                    ui_amount: 0.0,
                    decimals: 0,
                    token_account: None,
                    token_program_id: String::new(),
                });
            }
        };

        match self.rpc.get_token_account_balance(&token_account_pk).await {
            Ok(ui_amount) => {
                let amount: u64 = ui_amount.amount.parse().unwrap_or(0);
                Ok(crate::types::TokenBalance {
                    mint: mint.to_string(),
                    wallet: wallet.to_string(),
                    amount,
                    ui_amount: ui_amount.ui_amount.unwrap_or(0.0),
                    decimals: ui_amount.decimals,
                    token_account: Some(token_account_pk.to_string()),
                    token_program_id: String::new(),
                })
            }
            Err(e) => bail!("Failed to get token balance: {e}"),
        }
    }

    /// The `bonding-curve` PDA for a mint — single source of truth for the
    /// `find_program_address(&[b"bonding-curve", ..])` previously inlined across
    /// the buy and query paths.
    pub(super) fn bonding_curve_pda(&self, mint: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(&[b"bonding-curve", mint.as_ref()], &self.pump_program).0
    }

    /// All curve PDAs needed to build a buy/sell for `mint`, given its `creator`
    /// and SPL token program. Pure (no RPC) and deterministic — shared by the
    /// snipe buy path and `get_creator_from_mint_pda`.
    pub(super) fn derive_token_pdas(
        &self,
        mint: &Pubkey,
        creator: &Pubkey,
        token_program: &Pubkey,
        cashback_enabled: bool,
    ) -> TokenPDAs {
        let bonding_curve = self.bonding_curve_pda(mint);
        let (bonding_curve_v2, _) =
            Pubkey::find_program_address(&[b"bonding-curve-v2", mint.as_ref()], &self.pump_program);
        let associated_bonding_curve =
            get_associated_token_address_with_program_id(&bonding_curve, mint, token_program);
        let (creator_vault, _) =
            Pubkey::find_program_address(&[b"creator-vault", creator.as_ref()], &self.pump_program);
        TokenPDAs {
            token_program: *token_program,
            bonding_curve,
            bonding_curve_v2,
            associated_bonding_curve,
            creator_vault,
            cashback_enabled,
        }
    }

    /// Read a mint's bonding-curve PDA and mint account in one
    /// `getMultipleAccounts` request and parse the routing facts both trade
    /// paths need. BondingCurve layout after the 8-byte Anchor discriminator:
    /// `complete: bool` @48, `creator: Pubkey` @49, cashback flag @82; the
    /// token program is the mint account's owner. Shared by
    /// [`Self::resolve_buy_routing`] and [`Self::get_creator_from_mint_pda`].
    async fn read_curve_routing(&self, mint: &Pubkey) -> Result<CurveRouting> {
        let key = mint.to_string();

        // creator / token_program / cashback are fixed at creation and migration
        // is terminal on-chain (curve → AMM, never back). So once a mint is
        // observed migrated, every routing fact is immutable and we can serve it
        // from cache with zero RPC — the common case for trading a graduated
        // token. A not-yet-migrated entry is deliberately re-read each call so we
        // catch the curve→AMM transition; a stale `is_migrated = false` would
        // misroute a now-migrated trade to the bonding curve, which the program
        // rejects with BondingCurveComplete (6005).
        if let Some(cached) = self.curve_routing_cache.get(&key).map(|r| *r) {
            if cached.is_migrated {
                return Ok(cached);
            }
            // A fresh AMM-venue reserve snapshot in the WS-fed cache is monotonic
            // proof of migration (curve → AMM is terminal, and an AMM snapshot
            // only exists post-graduation). The other routing facts (creator,
            // token program) are immutable, so promote the cached entry to
            // migrated and serve it with zero RPC — instead of re-reading the
            // bonding curve on every pre-migration trade just to catch the flip.
            let has_amm_snapshot = self
                .reserve_cache
                .get_fresh(
                    &key,
                    std::time::Duration::from_millis(self.config.cache.reserve_max_age_ms),
                    true,
                )
                .is_some();
            if has_amm_snapshot {
                let migrated = CurveRouting {
                    is_migrated: true,
                    ..cached
                };
                self.curve_routing_cache.insert(key, migrated);
                return Ok(migrated);
            }
        }

        let bonding_curve = self.bonding_curve_pda(mint);

        // Both independent accounts in one request — this gates every manual
        // buy/sell, so keep it a single round-trip.
        let accounts = self.rpc.get_multiple_accounts(&[bonding_curve, *mint]).await?;
        let [bonding_acct, mint_acct]: [Option<_>; 2] = accounts
            .try_into()
            .map_err(|_| {
                TradeError::Other("getMultipleAccounts returned an unexpected count".into())
            })?;
        let account = bonding_acct
            .ok_or_else(|| TradeError::Other("bonding curve account not found".into()))?;
        let mint_account =
            mint_acct.ok_or_else(|| TradeError::Other("mint account not found".into()))?;

        const COMPLETE_OFFSET: usize = 48;
        const CREATOR_OFFSET: usize = 49;
        const CASHBACK_OFFSET: usize = 82;
        if account.data.len() < CREATOR_OFFSET + 32 {
            bail!("Bonding curve account data too short");
        }
        let creator_bytes: [u8; 32] =
            account.data[CREATOR_OFFSET..CREATOR_OFFSET + 32].try_into()?;
        let routing = CurveRouting {
            creator: Pubkey::from(creator_bytes),
            token_program: mint_account.owner,
            is_migrated: account.data[COMPLETE_OFFSET] != 0,
            cashback_enabled: account.data.len() > CASHBACK_OFFSET
                && account.data[CASHBACK_OFFSET] != 0,
        };
        // Cache the fresh read. A migrated entry is terminal and will be served
        // directly next time; a not-yet-migrated entry is overwritten on the next
        // re-read until it flips.
        self.curve_routing_cache.insert(key, routing);
        Ok(routing)
    }

    /// Resolve everything a manual buy needs straight from chain (source of
    /// truth, so it handles freshly-migrated and Token-2022 tokens the local
    /// cache may not know about yet). Reads the bonding-curve PDA for the
    /// creator and the `complete` flag, and the mint account owner for the
    /// token program.
    ///
    /// BondingCurve layout: 8-byte discriminator, 5×u64 reserves/supply, then
    /// `complete: bool` at offset 48 and `creator: Pubkey` at offset 49 — the
    /// same account [`get_creator_from_mint_pda`] reads.
    pub async fn resolve_buy_routing(
        &self,
        mint_address: &str,
    ) -> Result<crate::types::BuyRouting> {
        let mint = Pubkey::from_str(mint_address)?;
        let routing = self.read_curve_routing(&mint).await?;
        Ok(crate::types::BuyRouting {
            creator: routing.creator.to_string(),
            token_program_id: routing.token_program.to_string(),
            is_migrated: routing.is_migrated,
            mint,
            creator_pubkey: routing.creator,
            token_program: crate::types::TokenProgram::from_pubkey(&routing.token_program),
        })
    }

    /// Batch-resolve on-chain bonding-curve facts (migration + cashback) for
    /// many mints in as few RPC round-trips as possible. Reads each mint's
    /// bonding-curve PDA and inspects the `complete` byte at offset 48 and the
    /// create_v2 cashback byte at offset 82 — the same account and offsets the
    /// trade path reads in [`Self::resolve_buy_routing`], but via
    /// `getMultipleAccounts` so a whole wallet costs one request per 100 mints
    /// instead of one per mint.
    ///
    /// Returns a map of mint -> [`CurveFacts`] holding only the mints whose
    /// bonding curve account exists and is long enough to read. Mints absent
    /// from the map could not be resolved (not a pump.fun bonding-curve token,
    /// account missing, or the RPC chunk errored); the caller decides how to
    /// treat that "unknown" — e.g. fall back to a cached value.
    pub async fn resolve_curve_facts_batch(
        &self,
        mints: &[String],
    ) -> HashMap<String, crate::types::CurveFacts> {
        const COMPLETE_OFFSET: usize = 48;
        const CASHBACK_OFFSET: usize = 82;

        // Derive the bonding-curve PDA for every mint we can parse, keeping the
        // mint string alongside it so results map back to the caller's keys.
        let derived: Vec<(String, Pubkey)> = mints
            .iter()
            .filter_map(|m| {
                let mint = Pubkey::from_str(m).ok()?;
                Some((m.clone(), self.bonding_curve_pda(&mint)))
            })
            .collect();

        let mut out = HashMap::new();
        // getMultipleAccounts is capped at 100 accounts per request.
        for chunk in derived.chunks(100) {
            let pubkeys: Vec<Pubkey> = chunk.iter().map(|(_, pda)| *pda).collect();
            let accounts = match self.rpc.get_multiple_accounts(&pubkeys).await {
                Ok(a) => a,
                Err(e) => {
                    tracing::warn!("resolve_curve_facts_batch: get_multiple_accounts failed: {e}");
                    continue;
                }
            };
            for ((mint, _), account) in chunk.iter().zip(accounts) {
                if let Some(acct) = account {
                    if acct.data.len() > COMPLETE_OFFSET {
                        out.insert(
                            mint.clone(),
                            crate::types::CurveFacts {
                                is_migrated: acct.data[COMPLETE_OFFSET] != 0,
                                cashback_enabled: acct.data.len() > CASHBACK_OFFSET
                                    && acct.data[CASHBACK_OFFSET] != 0,
                            },
                        );
                    }
                }
            }
        }
        out
    }

    /// Warm the per-mint [`TokenPDAs`] cache (creator vault, bonding curve, token
    /// program, cashback flag) by reading the bonding-curve PDA — without
    /// allocating the creator `String`. The sell hot path only needs the PDAs
    /// cached, not the creator text, so it calls this instead of
    /// [`Self::get_creator_from_mint_pda`] to skip the wasted `to_string`.
    pub(super) async fn ensure_token_pdas(&self, mint_address: &str) -> Result<()> {
        let mint = Pubkey::from_str(mint_address)?;
        let routing = self.read_curve_routing(&mint).await?;

        // Cache the full PDA set for this mint (shared derivation with the buy
        // path; pure PDA math, no RPC).
        self.token_pdas.insert(
            mint_address.to_string(),
            self.derive_token_pdas(
                &mint,
                &routing.creator,
                &routing.token_program,
                routing.cashback_enabled,
            ),
        );
        Ok(())
    }

    /// Utility to fetch the creator pubkey for a given mint by reading the bonding curve PDA account.
    /// Returns the creator as a String. Also warms the [`TokenPDAs`] cache; callers
    /// that only need the cache warmed (not the creator text) should use the
    /// String-free [`Self::ensure_token_pdas`] instead.
    pub async fn get_creator_from_mint_pda(&self, mint_address: &str) -> Result<String> {
        let mint = Pubkey::from_str(mint_address)?;
        let routing = self.read_curve_routing(&mint).await?;

        // Cache the full PDA set for this mint (shared derivation with the buy
        // path; pure PDA math, no RPC).
        self.token_pdas.insert(
            mint_address.to_string(),
            self.derive_token_pdas(
                &mint,
                &routing.creator,
                &routing.token_program,
                routing.cashback_enabled,
            ),
        );

        Ok(routing.creator.to_string())
    }

    /// Force-refresh the cached curve `creator_vault` for `mint` by re-reading the
    /// bonding curve's CURRENT `creator` from chain, returning the freshly derived
    /// vault. pump.fun can mutate `bonding_curve.creator` (via `set_creator`) AFTER
    /// the snipe buy cached this mint's [`TokenPDAs`]; the stale buy-time
    /// `creator_vault` then reverts every sell with Anchor `ConstraintSeeds` (2006).
    /// [`Self::ensure_token_pdas`] re-reads the routing (a non-migrated curve is read
    /// fresh, not served from cache) and overwrites the cached PDAs, so the next sell
    /// attempt builds with the current vault. OFF the hot path — the exit loop calls
    /// this only after a sell poll window failed AND the on-chain revert code is 2006.
    pub async fn refresh_curve_creator_vault(&self, mint_address: &str) -> Result<Pubkey> {
        self.ensure_token_pdas(mint_address).await?;
        self.token_pdas
            .get(mint_address)
            .map(|r| r.creator_vault)
            .ok_or_else(|| {
                TradeError::Other(format!("creator_vault missing after refresh for {mint_address}"))
            })
    }
}
