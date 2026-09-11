//! Rule-activation hydration: a rule switched on mid-run reads every live token as
//! if it had been on since that token's birth.
//!
//! A reload arms nothing on tokens that already exist, and registers the new rule's
//! windows on tracked tokens going forward only. So after every reload this module
//! finds the rules that just became armable and whether the rules now ask a track for
//! state it never folded, and rebuilds the affected tokens through
//! [`hunter_engine::hydrate_token`] from their complete history: the cache when it
//! still holds every trade since creation, else `trades` up to the oldest cached
//! trade plus the cache from there. Each rebuild is one [`Loaded`] message the loop
//! folds between events (the `trades` read runs off the loop first), so a switch-on
//! over a thousand live tokens never stalls a decision behind it.
//!
//! **Only tokens born while this process ran are rebuilt.** A token born before it
//! crossed the restart: the trades printed while the process was down reached neither
//! the cache nor `trades`, and nothing replays them. Its history has a hole, and a
//! lifetime term read over it (a buyer count, the all-time high behind `stall`) can be
//! wrong in either direction, so it stays out: fail closed.

use std::collections::BTreeSet;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::mpsc;
use tracing::{info, warn};

use hunter_engine::event::{Mint, RuleId};
use hunter_engine::metrics::TradeLite;
use hunter_engine::reduce::Effects;
use hunter_engine::{hydrate_token, EngineState, TrackRequirements};
use trading_core::state::token_cache::TokenCache;
use trading_core::storage::repositories::trade_repo::TradeRepo;

use super::producers::{history_trade_lite, ChainKey, Producer};

/// Scan slack before a token's creation for its `trades` read: block time is the
/// chunk key, and a creation stamp can trail its first print by a few seconds.
const HISTORY_SCAN_SLACK_SECS: i64 = 60;

/// Re-reads allowed when the cache trims past the split while `trades` is read.
const MAX_SPLICE_ATTEMPTS: u8 = 3;

/// The rule set as a reload found it: which rules were armable, what a track folded.
pub struct ReloadSnapshot {
    enabled: BTreeSet<RuleId>,
    track: TrackRequirements,
}

impl ReloadSnapshot {
    pub fn of(state: &EngineState) -> Self {
        Self { enabled: enabled_rules(state), track: state.track_requirements() }
    }
}

fn enabled_rules(state: &EngineState) -> BTreeSet<RuleId> {
    state.rules.iter().filter(|(_, c)| c.entry_enabled).map(|(id, _)| *id).collect()
}

/// One token to rebuild, folded by the loop in [`Hydrator::apply`].
pub struct Loaded {
    mint: String,
    /// `None`: the cache holds the whole history. `Some(k)`: `pg` holds every trade
    /// strictly before `k`, the oldest cached trade when the read was planned.
    split: Option<ChainKey>,
    pg: Vec<TradeLite>,
    admit: Arc<BTreeSet<RuleId>>,
    attempts: u8,
}

/// One `trades` read to run off the loop.
struct Job {
    mint: String,
    since: DateTime<Utc>,
    split: ChainKey,
    admit: Arc<BTreeSet<RuleId>>,
    attempts: u8,
}

/// What the planner copies out of one cache entry (no guard outlives the scan).
struct Candidate {
    mint: String,
    created_at: DateTime<Utc>,
    /// The cache holds every trade since creation.
    complete: bool,
    oldest: Option<ChainKey>,
}

pub struct Hydrator {
    trade_repo: TradeRepo,
    tx: mpsc::UnboundedSender<Loaded>,
}

impl Hydrator {
    pub fn new(trade_repo: TradeRepo) -> (Self, mpsc::UnboundedReceiver<Loaded>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { trade_repo, tx }, rx)
    }

    /// Run after a reload folded into `state`: arm the rules that just became armable
    /// on every live token born since the loop started, rebuilding the tracks the new
    /// rule set reads more of. Arming a tracked token whose track needs nothing new is
    /// done here; every rebuild is queued for [`Self::apply`], the ones whose cache
    /// lost the oldest trades after a `trades` read off the loop.
    pub fn after_reload(
        &self,
        before: &ReloadSnapshot,
        state: &mut EngineState,
        producer: &mut Producer,
        token_cache: &TokenCache,
    ) -> Effects {
        let mut fx = Effects::new();
        let admit: Arc<BTreeSet<RuleId>> =
            Arc::new(enabled_rules(state).difference(&before.enabled).copied().collect());
        let rebuild = state.track_requirements().adds_to(&before.track);
        if admit.is_empty() && !rebuild {
            return fx;
        }
        let now = Utc::now();
        let since = producer.started_at();
        let candidates: Vec<Candidate> = token_cache
            .iter()
            .filter(|e| e.value().token.created_at >= since && !e.value().is_dead(now))
            .map(|e| {
                let s = e.value();
                Candidate {
                    mint: e.key().clone(),
                    created_at: s.token.created_at,
                    complete: s.trades_base == 0 && s.trade_count == s.trades.len() as u64,
                    oldest: s.trades.iter().map(|t| (t.slot, t.tx_index, t.leg_index)).min(),
                }
            })
            .collect();

        let (mut armed_only, mut queued, mut jobs) = (0usize, 0usize, Vec::new());
        for c in candidates {
            let tracked = state.tokens.contains_key(c.mint.as_str());
            if !tracked && admit.is_empty() {
                continue;
            }
            let Some(facts) = producer.hydrate_facts(&c.mint) else { continue };
            let mint = Mint::from(c.mint.as_str());
            if tracked && !rebuild {
                fx.extend(hydrate_token(state, &mint, &facts, None, now, |id| admit.contains(&id)));
                armed_only += 1;
                continue;
            }
            if c.complete {
                queued += 1;
                let _ = self.tx.send(Loaded {
                    mint: c.mint,
                    split: None,
                    pg: Vec::new(),
                    admit: Arc::clone(&admit),
                    attempts: 0,
                });
                continue;
            }
            let Some(split) = c.oldest else { continue };
            jobs.push(Job {
                mint: c.mint,
                since: c.created_at - chrono::Duration::seconds(HISTORY_SCAN_SLACK_SECS),
                split,
                admit: Arc::clone(&admit),
                attempts: 0,
            });
        }
        info!(
            admitted_rules = admit.len(),
            rebuild,
            armed_only,
            from_cache = queued,
            from_trades = jobs.len(),
            "engine: rule activation hydration"
        );
        self.spawn(jobs);
        fx
    }

    /// Rebuild and arm one queued token: its `trades` part (if any) spliced onto the
    /// cache. A cache that trimmed past the split meanwhile is read again from its new
    /// oldest trade.
    pub fn apply(
        &self,
        loaded: Loaded,
        state: &mut EngineState,
        producer: &mut Producer,
        token_cache: &TokenCache,
    ) -> Effects {
        let Some(facts) = producer.hydrate_facts(&loaded.mint) else {
            return Effects::new();
        };
        let Some(tail) = producer.hydration_history(&loaded.mint, loaded.split) else {
            let oldest = token_cache.get(&loaded.mint).and_then(|e| {
                e.value().trades.iter().map(|t| (t.slot, t.tx_index, t.leg_index)).min()
            });
            match oldest {
                Some(split) if loaded.attempts < MAX_SPLICE_ATTEMPTS => self.spawn(vec![Job {
                    mint: loaded.mint,
                    since: facts.created_at - chrono::Duration::seconds(HISTORY_SCAN_SLACK_SECS),
                    split,
                    admit: loaded.admit,
                    attempts: loaded.attempts + 1,
                }]),
                _ => warn!(mint = %loaded.mint, "engine: hydration dropped - the cache kept trimming past the trades read"),
            }
            return Effects::new();
        };
        let mint = Mint::from(loaded.mint.as_str());
        let mut history = loaded.pg;
        history.extend(tail);
        let admit = loaded.admit;
        hydrate_token(state, &mint, &facts, Some(&history), Utc::now(), |id| admit.contains(&id))
    }

    /// Read each job's `trades` history, one query at a time, off the loop.
    fn spawn(&self, jobs: Vec<Job>) {
        if jobs.is_empty() {
            return;
        }
        let repo = self.trade_repo.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            for job in jobs {
                match repo.find_by_mint_before(&job.mint, job.since, job.split).await {
                    Ok(rows) => {
                        let loaded = Loaded {
                            mint: job.mint,
                            split: Some(job.split),
                            pg: rows.iter().map(history_trade_lite).collect(),
                            admit: job.admit,
                            attempts: job.attempts,
                        };
                        if tx.send(loaded).is_err() {
                            return;
                        }
                    }
                    Err(e) => warn!(mint = %job.mint, "engine: hydration trades read failed: {e}"),
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc as StdArc;

    use chrono::Duration;
    use hunter_engine::event::{Effect, Event, LoadedRule, TradeMode};
    use hunter_engine::fingerprint::{Criteria, Fingerprint, FingerprintId};
    use hunter_engine::metrics::Side;
    use hunter_engine::reduce;
    use hunter_engine::rule_params::RuleParams;
    use sqlx::postgres::PgPoolOptions;
    use trading_core::models::token::Token;
    use trading_core::models::trade::{Trade, TradeType};
    use trading_core::state::token_cache::TokenState;
    use uuid::Uuid;

    fn fp() -> Fingerprint {
        Fingerprint {
            id: FingerprintId(Uuid::from_u128(0xF)),
            wildcard: true,
            criteria: Criteria::new(),
            metric_config: serde_json::json!({}),
        }
    }

    /// Buys a 1 SOL sell once 3 wallets other than the creator have bought since birth.
    fn crowd_rule() -> LoadedRule {
        let params = serde_json::json!({
            "entry": {
                "m_crowd_after_age": {"after_age_sec": 0,
                                      "non_creator_buyers": [{"operator": ">=", "value": 3}]},
                "m_flow_window": {"window_size_prints": 1, "sell": [{"operator": ">=", "value": 1}]}
            },
            "take_profit": 50
        });
        LoadedRule {
            id: RuleId(Uuid::from_u128(1)),
            fingerprint_id: fp().id,
            trade_mode: TradeMode::Paper,
            buy_amount_lamports: 100_000_000,
            max_concurrent_tokens: 0,
            max_total_tokens: 0,
            params: RuleParams::parse(&params).expect("valid"),
            entry_enabled: true,
        }
    }

    fn reload(state: &mut EngineState, rules: Vec<LoadedRule>) {
        let _ = reduce(state, Event::RulesReloaded { rules: StdArc::from(rules), fps: StdArc::from(vec![fp()]) });
    }

    /// A cached token born `born_ago` s back with three buyers, none of them the creator.
    fn cache_with(mint: &str, born_ago: i64) -> StdArc<TokenCache> {
        let now = Utc::now();
        let token = Token::new(
            mint.into(), "creator".into(), "Name".into(), "SYM".into(),
            None, None, None, None, None, None, None, false, false,
            serde_json::Value::Array(vec![]), "create-sig".into(), None,
            now - Duration::seconds(born_ago),
        );
        let mut state = TokenState::new(token);
        for (i, w) in ["w1", "w2", "w3"].iter().enumerate() {
            state.add_trade(Trade::new(
                mint.into(), (*w).into(), TradeType::Buy, 0.5, 1_000_000,
                format!("sig-{i}"), 10 + i as u64, now - Duration::seconds(born_ago - 1 - i as i64),
            ));
        }
        let cache = TokenCache::new();
        cache.insert(mint.into(), state);
        StdArc::new(cache)
    }

    fn sell(at: DateTime<Utc>) -> TradeLite {
        TradeLite { side: Side::Sell, sol: 1.5, price: 1e-7, at, wallet_hash: 7, slot: 50, on_curve: true, ..Default::default() }
    }

    fn hydrator() -> (Hydrator, mpsc::UnboundedReceiver<Loaded>) {
        Hydrator::new(TradeRepo::new(
            PgPoolOptions::new()
                .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/none")
                .expect("lazy pool"),
        ))
    }

    /// Switch the crowd rule on over `cache` and fold whatever the hydrator queues.
    fn switch_on(cache: &StdArc<TokenCache>, started_ago: i64) -> (EngineState, Effects) {
        let (h, mut rx) = hydrator();
        let mut producer = Producer::new(StdArc::clone(cache), Utc::now() - Duration::seconds(started_ago));
        let mut state = EngineState::new();
        reload(&mut state, vec![]);
        let before = ReloadSnapshot::of(&state);
        reload(&mut state, vec![crowd_rule()]);
        let mut fx = h.after_reload(&before, &mut state, &mut producer, cache);
        while let Ok(loaded) = rx.try_recv() {
            fx.extend(h.apply(loaded, &mut state, &mut producer, cache));
        }
        (state, fx)
    }

    #[tokio::test]
    async fn a_token_born_while_the_process_ran_is_armed_and_decides() {
        let mint = "MINT-born-after-start";
        let cache = cache_with(mint, 60);
        let (mut state, fx) = switch_on(&cache, 300);
        assert_eq!(fx.iter().filter(|e| matches!(e, Effect::ArmedChanged(_))).count(), 1);
        let out = reduce(&mut state, Event::Trade { mint: Mint::from(mint), trade: sell(Utc::now()) });
        assert_eq!(out.iter().filter(|e| matches!(e, Effect::SubmitBuy { .. })).count(), 1);
    }

    #[tokio::test]
    async fn a_token_that_crossed_the_restart_stays_out() {
        let mint = "MINT-born-before-start";
        let cache = cache_with(mint, 600);
        let (mut state, fx) = switch_on(&cache, 300);
        assert!(fx.is_empty(), "fail closed: its history has the downtime's hole");
        let out = reduce(&mut state, Event::Trade { mint: Mint::from(mint), trade: sell(Utc::now()) });
        assert!(out.iter().all(|e| !matches!(e, Effect::SubmitBuy { .. })));
    }
}
