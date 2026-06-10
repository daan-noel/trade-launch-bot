// ============================================================
// Buy-template seed pool.
//
// A buy needs a freshly-seeded create-account instruction ready to go.
// We keep a per-token-program pool of pre-built `BuyTemplate`s so the
// hot path never blocks building one.
//
//  - pool_for / next_seed / build_template : private building blocks.
//  - fill_buy_pool          : synchronously fill to target (init).
//  - acquire_buy_template   : pop one, or build inline on a miss.
//  - replenish_pool_async   : background top-up to target after a buy.
//  - prebuild_one_template_async : eagerly build one extra after a buy.
// ============================================================

use super::{BuyTemplate, PumpFunTrader};
use crate::constants::{BUY_SEED_POOL_SIZE, TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID};
use anyhow::Result;
use solana_sdk::system_instruction;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash as StdHash, Hasher};
use std::str::FromStr;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tracing::warn;

impl PumpFunTrader {
    fn pool_for(&self, token_program_id: &str) -> &Arc<Mutex<Vec<BuyTemplate>>> {
        if token_program_id == TOKEN_PROGRAM_ID {
            &self.buy_pool_legacy
        } else {
            &self.buy_pool_2022
        }
    }

    fn next_seed(&self) -> String {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let ctr = self.seed_counter.fetch_add(1, Ordering::Relaxed);
        let mut h = DefaultHasher::new();
        ts.hash(&mut h);
        ctr.hash(&mut h);
        format!("{:x}", h.finish())
    }

    /// (space, rent) for a token account of the given program — legacy vs 2022.
    fn space_rent_for(&self, token_program_id: &str) -> (u64, u64) {
        if token_program_id == TOKEN_PROGRAM_ID {
            (self.token_account_space, self.token_account_rent)
        } else {
            (self.token_2022_account_space, self.token_2022_account_rent)
        }
    }

    fn build_template(&self, token_program_id: &str) -> Result<BuyTemplate> {
        let program_id = Pubkey::from_str(token_program_id)?;
        let seed = self.next_seed();
        let (space, rent) = self.space_rent_for(token_program_id);
        build_template_with_seed(&self.config.keypair, &program_id, &seed, space, rent)
    }

    pub(super) async fn fill_buy_pool(&self, token_program_id: &str) -> Result<()> {
        let pool = self.pool_for(token_program_id);
        let target = BUY_SEED_POOL_SIZE;
        loop {
            if pool.lock().await.len() >= target {
                break;
            }
            let t = self.build_template(token_program_id)?;
            pool.lock().await.push(t);
        }
        Ok(())
    }

    /// Pop a template from the pool; build one on-the-fly on a miss (with logging).
    pub(super) async fn acquire_buy_template(&self, token_program_id: &str) -> Result<BuyTemplate> {
        if let Some(t) = self.pool_for(token_program_id).lock().await.pop() {
            return Ok(t);
        }

        // Pool miss — build inline and track
        let count = if token_program_id == TOKEN_PROGRAM_ID {
            self.buy_pool_misses_legacy.fetch_add(1, Ordering::Relaxed) + 1
        } else {
            self.buy_pool_misses_2022.fetch_add(1, Ordering::Relaxed) + 1
        };
        if count % 25 == 0 {
            warn!("⚠️  Buy pool miss #{} for {}", count, token_program_id);
        }

        self.build_template(token_program_id)
    }

    /// Refill the pool back up to target in the background after a buy consumed a
    /// template. Builds one (a buy consumes one), reusing the same builder the
    /// sync path uses, so there's a single copy of the seed/derivation logic.
    pub(super) fn replenish_pool_async(&self, token_program_id: &str) {
        // Determine which static string to use so we can 'static the spawn
        let prog_id: &'static str = if token_program_id == TOKEN_PROGRAM_ID {
            TOKEN_PROGRAM_ID
        } else {
            TOKEN_2022_PROGRAM_ID
        };

        let pool = Arc::clone(self.pool_for(prog_id));
        let target = BUY_SEED_POOL_SIZE;
        let kp = self.config.keypair.insecure_clone();
        let (space, rent) = self.space_rent_for(prog_id);

        tokio::spawn(async move {
            if pool.lock().await.len() >= target {
                return;
            }
            let program_id = match Pubkey::from_str(prog_id) {
                Ok(p) => p,
                Err(_) => return,
            };
            // No `&self` in the spawn, so derive a unique seed from the clock
            // rather than `next_seed`'s counter (a rare collision just yields a
            // duplicate template — harmless).
            let seed = format!(
                "{:x}",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or_default()
            );
            if let Ok(template) = build_template_with_seed(&kp, &program_id, &seed, space, rent) {
                pool.lock().await.push(template);
            }
        });
    }
}

/// Build one buy template (a create-account-with-seed instruction + the resulting
/// token account address) for `program_id`. Shared by the synchronous fill path
/// ([`PumpFunTrader::build_template`]) and the background replenisher so the
/// derivation lives in exactly one place.
fn build_template_with_seed(
    keypair: &Keypair,
    program_id: &Pubkey,
    seed: &str,
    space: u64,
    rent: u64,
) -> Result<BuyTemplate> {
    let owner = keypair.pubkey();
    let user_token_account = Pubkey::create_with_seed(&owner, seed, program_id)?;
    let ix = system_instruction::create_account_with_seed(
        &owner,
        &user_token_account,
        &owner,
        seed,
        rent,
        space,
        program_id,
    );
    Ok(BuyTemplate {
        create_with_seed_ix: ix,
        user_token_account,
    })
}
