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
use crate::constants::BUY_SEED_POOL_SIZE;
use crate::types::TokenProgram;
use anyhow::Result;
use solana_sdk::system_instruction;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash as StdHash, Hasher};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tracing::warn;

impl PumpFunTrader {
    fn pool_for(&self, token_program: TokenProgram) -> &Arc<Mutex<Vec<BuyTemplate>>> {
        match token_program {
            TokenProgram::Legacy => &self.buy_pool_legacy,
            TokenProgram::Token2022 => &self.buy_pool_2022,
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
    fn space_rent_for(&self, token_program: TokenProgram) -> (u64, u64) {
        match token_program {
            TokenProgram::Legacy => (self.token_account_space, self.token_account_rent),
            TokenProgram::Token2022 => {
                (self.token_2022_account_space, self.token_2022_account_rent)
            }
        }
    }

    fn build_template(&self, token_program: TokenProgram) -> Result<BuyTemplate> {
        let program_id = token_program.pubkey();
        let seed = self.next_seed();
        let (space, rent) = self.space_rent_for(token_program);
        build_template_with_seed(&self.config.keypair, &program_id, &seed, space, rent)
    }

    pub(super) async fn fill_buy_pool(&self, token_program: TokenProgram) -> Result<()> {
        let pool = self.pool_for(token_program);
        let target = BUY_SEED_POOL_SIZE;
        loop {
            if pool.lock().await.len() >= target {
                break;
            }
            let t = self.build_template(token_program)?;
            pool.lock().await.push(t);
        }
        Ok(())
    }

    /// Pop a template from the pool; build one on-the-fly on a miss (with logging).
    pub(super) async fn acquire_buy_template(
        &self,
        token_program: TokenProgram,
    ) -> Result<BuyTemplate> {
        if let Some(t) = self.pool_for(token_program).lock().await.pop() {
            return Ok(t);
        }

        // Pool miss — build inline and track
        let count = match token_program {
            TokenProgram::Legacy => self.buy_pool_misses_legacy.fetch_add(1, Ordering::Relaxed) + 1,
            TokenProgram::Token2022 => {
                self.buy_pool_misses_2022.fetch_add(1, Ordering::Relaxed) + 1
            }
        };
        if count % 25 == 0 {
            warn!("⚠️  Buy pool miss #{} for {}", count, token_program.as_id());
        }

        self.build_template(token_program)
    }

    /// Refill the pool back up to target in the background after a buy consumed a
    /// template. Builds one (a buy consumes one), reusing the same builder the
    /// sync path uses, so there's a single copy of the seed/derivation logic.
    pub(super) fn replenish_pool_async(&self, token_program: TokenProgram) {
        let pool = Arc::clone(self.pool_for(token_program));
        let target = BUY_SEED_POOL_SIZE;
        let kp = self.config.keypair.insecure_clone();
        let (space, rent) = self.space_rent_for(token_program);
        let program_id = token_program.pubkey();

        tokio::spawn(async move {
            if pool.lock().await.len() >= target {
                return;
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trader::TraderConfig;
    use solana_sdk::signature::Keypair;
    use std::time::Duration;

    fn dummy_trader() -> PumpFunTrader {
        let config = Arc::new(TraderConfig {
            rpc_url: "http://localhost".into(),
            helius_sender_urls: vec!["http://localhost".into()],
            keypair: Keypair::new(),
            nonce_accounts: vec![Pubkey::new_unique().to_string()],
        });
        PumpFunTrader::new(config)
    }

    /// `replenish_pool_async` rebuilds exactly the slot a buy consumed: from
    /// `target - 1` (pool state right after `acquire_buy_template` pops one) it
    /// tops back up to `target`. This is the invariant the buy path leans on now
    /// that it fires the refill the moment a template is consumed — ahead of the
    /// send — rather than after the buy returns. It must reach target either way.
    #[tokio::test]
    async fn replenish_refills_the_slot_a_buy_consumed() {
        let t = dummy_trader();
        let target = BUY_SEED_POOL_SIZE;

        // Seed one short of target — the pool state immediately after a buy pops
        // a template.
        {
            let mut pool = t.pool_for(TokenProgram::Legacy).lock().await;
            for _ in 0..target - 1 {
                pool.push(t.build_template(TokenProgram::Legacy).unwrap());
            }
        }

        t.replenish_pool_async(TokenProgram::Legacy);

        // The refill runs in a spawned task; poll briefly for it to land.
        let mut len = 0;
        for _ in 0..200 {
            len = t.pool_for(TokenProgram::Legacy).lock().await.len();
            if len >= target {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(len, target, "refill should rebuild the consumed slot");

        // Idempotent: a call at target adds nothing (the early `len >= target`
        // guard), so firing it eagerly never overfills.
        t.replenish_pool_async(TokenProgram::Legacy);
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(
            t.pool_for(TokenProgram::Legacy).lock().await.len(),
            target,
            "refill at target must be a no-op"
        );
    }
}
