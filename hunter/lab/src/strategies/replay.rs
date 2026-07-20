//! Analysis replay driver (plan 5.1) — the lab-side producer/consumer around the
//! pure engine [`reduce`](hunter_engine::reduce). It turns a set of tokens' lake
//! trade histories into one **globally time-ordered** engine event stream, folds
//! it through the same `reduce` the live loop uses, fills submits with a
//! transaction-free sim model, and collects the resulting position lifecycle into
//! per-token result rows.
//!
//! Why one global stream (not per-token): the engine's concurrency/lifetime caps
//! are **per-rule counters shared across tokens**, so a faithful backtest must let
//! tokens arm/enter/exit in real time order against one [`EngineState`] — exactly
//! as live does. That is what makes simulate parity-correct by construction
//! (redesign decision 6): the live loop and this driver differ only in *who
//! produces the events*, never in the decision logic.
//!
//! Determinism / bounding:
//! * Events are merged by `(timestamp, mint, kind)` so the fold is reproducible.
//! * Synthetic `Tick`s are emitted on a fixed [`TICK`] grid (from `TICK_MS`)
//!   constant, single-sourced) between event timestamps and after the last trade
//!   up to `as_of` (the run's wall-clock present — the deadness "now", so a token
//!   quiet past `DEAD_QUIET_SECS` books `Dead`, matching the live clock).
//! * Ticking **stops the moment no token is active** and jumps to the next event,
//!   so a multi-day corpus never replays empty ticks — tokens resolve within
//!   `DEAD_QUIET_SECS` of their last trade and drop out of the tracked set.
//!
//! The sim fill model mirrors `live::strategies::engine::exec_paper`: a submit
//! fills at the **worst-case** adverse price in the trigger/fire slot window
//! ([`trading_core::strategies::paper_fill`]), so paper-live and simulate price a
//! round-trip identically.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};

use hunter_engine::deadness::DEAD_QUIET_SECS;
use hunter_engine::event::{
    Effect, Event, ExitReason, Fill, FillFailReason, IntentId, LoadedRule, Mint, PositionId,
    PositionStatus, RuleId,
};
use hunter_engine::fingerprint::Fingerprint as EngineFingerprint;
use hunter_engine::grouping::{TokenFingerprint, LAMPORTS_PER_SOL_F64};
use hunter_engine::metrics::Ts;
use hunter_engine::{reduce, EngineState};

use trading_core::strategies::paper_fill::{
    find_worst_case_paper_entry_at, find_worst_case_paper_exit_at,
};

use crate::sweep::projection::CorpusTrade;

/// The clock tick — derived from the engine's [`hunter_engine::TICK_MS`]
/// SSOT so the live decision loop and this replay driver tick at the one cadence
/// (plan 5.3). The guard test below asserts the derivation.
pub const TICK: Duration = Duration::milliseconds(hunter_engine::TICK_MS);

/// pump.fun SPL token decimals — mirrors `exec_paper::TOKEN_SCALE`. Paper/sim
/// `token_amount` is cosmetic (PnL is price-ratio based), so it only needs to be
/// consistent across the paper-live and simulate fill models.
const TOKEN_SCALE: f64 = 1_000_000.0;

/// Margin past `DEAD_QUIET_SECS` to keep ticking the tail so the dead verdict (and
/// any exit it drives) definitely lands before we stop.
pub const TAIL_MARGIN_SECS: i64 = 60;

/// One token to replay: the metadata a result row needs, its observed creation
/// axes (built through the shared [`observed_axes`](trading_core::strategies::fingerprint_axes::observed_axes)
/// SSOT so it matches live), and its canonical-ordered lake trades.
#[derive(Clone)]
pub struct ReplayToken {
    pub mint: String,
    pub symbol: String,
    pub created_at: Ts,
    /// Observed creation axes; `first_slot_*` are `None` here and resolved by the
    /// driver from the creation-slot trades (a `FirstSlotSettled` event).
    pub tf: TokenFingerprint,
    pub trades: Arc<Vec<CorpusTrade>>,
    /// Creator wallet FNV hash for volume-flow classification (`None` when unknown).
    pub creator_wallet_hash: Option<u64>,
}

/// One replayed position's realized lifecycle — the engine-neutral outcome the
/// simulate/analysis layers turn into a result row. `exit_reason == None` ⇒ the
/// position was still open at `as_of` (mark-to-`last_price`).
#[derive(Clone, Debug)]
pub struct PositionOutcome {
    pub mint: String,
    pub rule: RuleId,
    /// Trigger-trade snapshot that armed the buy (distinct from the worst-case
    /// `entry_*` fill — the gap is modeled adverse slippage).
    pub target_price: Option<f64>,
    pub target_token_amount: Option<f64>,
    pub target_time: Option<Ts>,
    pub target_tx: Option<String>,
    pub entry_price: f64,
    pub entry_token_amount: u64,
    pub entry_time: Ts,
    pub entry_tx: String,
    pub exit_price: Option<f64>,
    pub exit_time: Option<Ts>,
    pub exit_tx: Option<String>,
    pub exit_reason: Option<ExitReason>,
    /// Last observed spot for the token — the mark for an open (unexited) position.
    pub last_price: f64,
}

/// Trigger-trade snapshot stashed when a worst-case entry fill is queued, then
/// copied onto the result row when the position reaches `Holding`.
#[derive(Clone, Debug)]
struct TargetSnap {
    price: f64,
    token_amount: f64,
    time: Ts,
    tx: String,
}

/// Config for a replay run.
#[derive(Clone, Copy)]
pub struct ReplayConfig {
    /// The run's wall-clock present — the deadness "now" every synthetic tick
    /// advances toward (plan 5.1: run-time as-of). Captured once by the caller.
    pub as_of: Ts,
}

impl Default for ReplayConfig {
    fn default() -> Self {
        Self { as_of: Utc::now() }
    }
}

/// Drive one global replay over `tokens` with `rules`/`fps` loaded, returning every
/// position that entered (open positions included, marked to their last price).
pub fn run_replay(
    rules: &[LoadedRule],
    fps: &[EngineFingerprint],
    tokens: Vec<ReplayToken>,
    cfg: ReplayConfig,
) -> Vec<PositionOutcome> {
    let mut driver = Replay::new(rules, fps, cfg);
    driver.load_tokens(tokens);
    driver.run();
    driver.finish()
}

/// A queued producer event (everything except synthetic `Tick`s), tagged with its
/// sort key so the global merge is deterministic.
struct Queued {
    at: Ts,
    /// Kind rank for a stable tie-break at equal `(at, mint)`:
    /// `TokenCreated` < `FirstSlotSettled` < `Trade` (creation before its trades).
    rank: u8,
    mint: Mint,
    event: Event,
    /// The corpus signature for a `Trade` (its fill tx link); `None` otherwise.
    sig: Option<Box<str>>,
    /// Corpus index of a `Trade` event into [`Replay::trades`]; `None` otherwise.
    trade_idx: Option<usize>,
}

/// The replay engine: owns the fold state plus the small adapter maps the sim
/// fill model + result collector need.
struct Replay {
    state: EngineState,
    cfg: ReplayConfig,
    /// Merged, sorted producer events.
    queue: Vec<Queued>,
    /// Per-mint lake trades — worst-case fill resolvers key by index into these.
    trades: HashMap<Mint, Arc<Vec<CorpusTrade>>>,
    /// Per-mint last folded trade's corpus index (trigger/fire for paper fills).
    last_trade_idx: HashMap<Mint, usize>,
    /// Per-mint last trade signature (entry/exit `tx` for a fill on that token).
    last_sig: HashMap<Mint, String>,
    /// Per-mint last observed spot (open-position mark).
    last_price: HashMap<Mint, f64>,
    /// Latest trade timestamp across the whole corpus (bounds the tail tick loop).
    last_trade_at: Option<Ts>,
    /// Buys awaiting a first finite price (enter-on-arm before any trade priced the
    /// token) — resolved at the token's first trade, mirroring `exec_paper`'s
    /// wait-for-price.
    pending_buys: HashMap<Mint, IntentId>,
    /// Worst-case fills whose pricing trade is still ahead in the stream. Applied
    /// when that corpus index is folded so `current_price` has caught up to the
    /// fill (immediate apply at the trigger would SL/TP against a stale spot).
    deferred_fills: HashMap<Mint, DeferredFill>,
    /// Trigger snapshot for an in-flight entry on this mint (set when the fill is
    /// queued, consumed when `Holding` records the row).
    pending_targets: HashMap<Mint, TargetSnap>,
    /// In-flight result rows, keyed by engine position id.
    builders: HashMap<PositionId, Builder>,
    /// Finished outcomes.
    done: Vec<PositionOutcome>,
}

/// A paper fill waiting for its pricing trade to be folded.
struct DeferredFill {
    fill_idx: usize,
    event: Event,
}

impl DeferredFill {
    fn fills_at(&self, trade_idx: usize) -> bool {
        self.fill_idx == trade_idx
    }
}

/// A result row under construction (from `BuySubmitted` to `End`/open).
struct Builder {
    mint: String,
    rule: RuleId,
    target: Option<TargetSnap>,
    entry_price: f64,
    entry_token_amount: u64,
    entry_time: Ts,
    entry_tx: String,
    /// Set once the entry fills (`Holding`); an entry that gives up before filling
    /// stays `false` and produces no row (never entered).
    entered: bool,
}

impl Replay {
    fn new(rules: &[LoadedRule], fps: &[EngineFingerprint], cfg: ReplayConfig) -> Self {
        let mut state = EngineState::new();
        reduce(
            &mut state,
            Event::RulesReloaded { rules: rules.to_vec().into(), fps: fps.to_vec().into() },
        );
        Self {
            state,
            cfg,
            queue: Vec::new(),
            trades: HashMap::new(),
            last_trade_idx: HashMap::new(),
            last_sig: HashMap::new(),
            last_price: HashMap::new(),
            last_trade_at: None,
            pending_buys: HashMap::new(),
            deferred_fills: HashMap::new(),
            pending_targets: HashMap::new(),
            builders: HashMap::new(),
            done: Vec::new(),
        }
    }

    /// Expand every token into its producer events and merge them into one sorted
    /// stream.
    fn load_tokens(&mut self, tokens: Vec<ReplayToken>) {
        for t in tokens {
            let mint = Mint::from(t.mint.as_str());
            let trades = &t.trades;
            self.trades.insert(mint.clone(), Arc::clone(trades));

            // TokenCreated at the token's creation time (first-slot axes still None).
            self.queue.push(Queued {
                at: t.created_at,
                rank: 0,
                mint: mint.clone(),
                event: Event::TokenCreated {
                    mint: mint.clone(),
                    fp: Box::new(t.tf.clone()),
                    at: t.created_at,
                    creator_wallet_hash: t.creator_wallet_hash,
                },
                sig: None,
                trade_idx: None,
            });

            if let Some(first) = trades.first() {
                let creation_slot = first.slot;
                // First-slot SOL sums (buy/sell) from the creation-slot trades — the
                // same axes the live producer settles from the cache's first-slot
                // accumulators.
                let mut fs_buy = 0.0;
                let mut fs_sell = 0.0;
                let mut settle_at: Option<Ts> = None;
                for (trade_idx, ct) in trades.iter().enumerate() {
                    if ct.slot == creation_slot {
                        if ct.is_buy {
                            fs_buy += ct.amount_sol;
                        } else {
                            fs_sell += ct.amount_sol;
                        }
                    } else if settle_at.is_none() {
                        // First trade in a later slot ⇒ the creation slot has closed.
                        settle_at = Some(ct.block_time);
                    }
                    // One Trade event per trade.
                    self.queue.push(Queued {
                        at: ct.block_time,
                        rank: 2,
                        mint: mint.clone(),
                        event: Event::Trade {
                            mint: mint.clone(),
                            trade: crate::sweep::projection::to_trade_lite(ct),
                        },
                        sig: ct.tx_signature.clone(),
                        trade_idx: Some(trade_idx),
                    });
                    self.last_trade_at =
                        Some(self.last_trade_at.map_or(ct.block_time, |m| m.max(ct.block_time)));
                }

                // Settle the first slot: at the first later-slot trade if one exists,
                // else immediately after the last creation-slot trade (a token that
                // never traded past its creation slot still resolves its full identity).
                let settle_at = settle_at.unwrap_or_else(|| {
                    trades.last().map(|t| t.block_time).unwrap_or(t.created_at)
                });
                self.queue.push(Queued {
                    at: settle_at,
                    rank: 1,
                    mint: mint.clone(),
                    event: Event::FirstSlotSettled {
                        mint,
                        buy_lamports: sol_to_lamports(fs_buy),
                        sell_lamports: sol_to_lamports(fs_sell),
                        at: settle_at,
                    },
                    sig: None,
                    trade_idx: None,
                });
            }
        }

        // Deterministic global order: time, then mint, then kind rank.
        self.queue.sort_by(|a, b| {
            a.at.cmp(&b.at).then(a.mint.cmp(&b.mint)).then(a.rank.cmp(&b.rank))
        });
    }

    /// Fold the merged stream, interleaving synthetic ticks, then the tail.
    fn run(&mut self) {
        // Move the queue out so we can borrow `self` mutably in the fold helpers.
        let queue = std::mem::take(&mut self.queue);
        let mut next_tick = queue.first().map(|q| q.at + TICK);

        for q in queue {
            if let Some(nt) = next_tick.as_mut() {
                self.emit_ticks_until(nt, q.at);
            }
            // Remember a trade's signature / price / corpus index for fills/marks
            // on this token before the fold consumes the event.
            if let Event::Trade { ref mint, ref trade } = q.event {
                if let Some(sig) = q.sig {
                    self.last_sig.insert(mint.clone(), sig.into());
                }
                self.last_price.insert(mint.clone(), trade.price);
                if let Some(idx) = q.trade_idx {
                    self.last_trade_idx.insert(mint.clone(), idx);
                }
            }
            self.fold_event(q.event, q.at);
        }

        // Tail: keep ticking up to `as_of`, but no further than the last trade plus
        // the dead-verdict window + margin (beyond which every remaining token is
        // dead and has been pruned). Stops early the moment nothing is active.
        if let Some(mut nt) = next_tick {
            let cap = self
                .last_trade_at
                .map(|t| t + Duration::seconds(DEAD_QUIET_SECS + TAIL_MARGIN_SECS));
            let mut tail_end = self.cfg.as_of;
            if let Some(c) = cap {
                if c < tail_end {
                    tail_end = c;
                }
            }
            self.emit_ticks_until(&mut nt, tail_end);
        }
    }

    /// Emit ticks on the fixed grid for every point in `[next_tick, until)`, folding
    /// each. Jumps `next_tick` forward without ticking whenever no token is active
    /// (an empty tracked set makes ticks pure no-ops), so a long quiet gap is O(1).
    fn emit_ticks_until(&mut self, next_tick: &mut Ts, until: Ts) {
        while *next_tick < until {
            if self.state.tokens.is_empty() {
                // No active tokens: skip the whole gap, realign to the grid past `until`.
                let gap_ms = (until - *next_tick).num_milliseconds();
                let steps = gap_ms / TICK.num_milliseconds() + 1;
                *next_tick += TICK * (steps as i32);
                return;
            }
            let now = *next_tick;
            self.fold_event(Event::Tick { now }, now);
            *next_tick += TICK;
        }
    }

    /// Fold one event at `now`, then drain the effects it produced — including the
    /// synthetic fills our sim executor feeds straight back in (so an entry that
    /// fills reaches `Holding` within the same event, like the live fill channel but
    /// inline). `now` is the deciding instant a fill is timestamped at.
    fn fold_event(&mut self, event: Event, now: Ts) {
        let mut work = vec![event];
        while let Some(ev) = work.pop() {
            // Note the mint a Trade touches so we can resolve a pending buy after.
            let traded_mint = match &ev {
                Event::Trade { mint, .. } => Some(mint.clone()),
                _ => None,
            };
            // SubmitBuy from a Trade uses that trade as the worst-case trigger;
            // tick / arm decisions defer until the next print (same as the sweep
            // scan's `entry_trigger_trade_idx`).
            let trade_trigger = traded_mint.is_some();
            let trade_idx_now = traded_mint
                .as_ref()
                .and_then(|m| self.last_trade_idx.get(m).copied());
            let effects = reduce(&mut self.state, ev);
            for fx in effects {
                self.on_effect(fx, now, &mut work, trade_trigger);
            }
            // After the print updates spot: confirm any worst-case fill priced on
            // this corpus index (scan's `fill_row`), then resolve pending entries
            // that were waiting for a trigger trade.
            if let (Some(mint), Some(idx)) = (traded_mint.as_ref(), trade_idx_now) {
                if self.deferred_fills.get(mint).is_some_and(|d| d.fills_at(idx)) {
                    let def = self.deferred_fills.remove(mint).unwrap();
                    work.push(def.event);
                }
            }
            if let Some(mint) = traded_mint {
                if let Some(intent) = self.pending_buys.remove(&mint) {
                    let lamports = self
                        .state
                        .rules
                        .get(&intent.rule)
                        .map(|c| c.buy_amount_lamports)
                        .unwrap_or(0);
                    self.queue_entry_fill(&mint, intent, lamports, &mut work);
                }
            }
        }
    }

    /// Handle one effect: sim-fill submits (feeding the fill event back onto the
    /// work stack) and record position-lifecycle updates.
    fn on_effect(&mut self, fx: Effect, now: Ts, work: &mut Vec<Event>, trade_trigger: bool) {
        match fx {
            Effect::SubmitBuy { intent, rule: _, mint, lamports } => {
                if trade_trigger {
                    self.queue_entry_fill(&mint, intent, lamports, work);
                } else {
                    // Tick / enter-on-arm: next print becomes the trigger.
                    self.pending_buys.insert(mint, intent);
                }
            }
            Effect::SubmitSell { intent, position, reason: _ } => {
                let token_amount =
                    self.builders.get(&position).map(|b| b.entry_token_amount).unwrap_or(0);
                let mint = intent.mint.clone();
                self.queue_exit_fill(&mint, intent, token_amount, now, work);
            }
            Effect::PositionUpdate(delta) => self.on_position_update(delta),
            Effect::ArmedChanged(_) => {}
        }
    }

    /// Resolve the worst-case entry after the mint's last folded trade (trigger).
    /// Empty window → `FillFailed`. Otherwise confirm now if the pricing trade is
    /// already the current print, else defer until that corpus index is folded.
    fn queue_entry_fill(
        &mut self,
        mint: &Mint,
        intent: IntentId,
        lamports: u64,
        work: &mut Vec<Event>,
    ) {
        let Some(&trigger_idx) = self.last_trade_idx.get(mint) else {
            self.pending_buys.insert(mint.clone(), intent);
            return;
        };
        let Some(trades) = self.trades.get(mint).cloned() else {
            self.pending_buys.insert(mint.clone(), intent);
            return;
        };
        match find_worst_case_paper_entry_at(trades.as_slice(), trigger_idx) {
            None => {
                self.pending_targets.remove(mint);
                work.push(Event::FillFailed {
                    intent,
                    reason: FillFailReason::Timeout,
                });
            }
            Some(fill) => {
                let trigger = &trades[trigger_idx];
                self.pending_targets.insert(
                    mint.clone(),
                    TargetSnap {
                        price: trigger.price_per_token,
                        token_amount: trigger.token_amount,
                        time: trigger.block_time,
                        tx: trigger.tx_signature.as_deref().unwrap_or("").to_string(),
                    },
                );
                let event = self.buy_fill(intent, lamports, fill.price, fill.block_time);
                self.emit_or_defer(mint, fill.trade_idx, event, work);
            }
        }
    }

    /// Worst-case exit after the mint's last folded trade (fire). Analysis path:
    /// market-fill at the fire trade when the window is empty.
    fn queue_exit_fill(
        &mut self,
        mint: &Mint,
        intent: IntentId,
        token_amount: u64,
        now: Ts,
        work: &mut Vec<Event>,
    ) {
        if let (Some(&fire_idx), Some(trades)) =
            (self.last_trade_idx.get(mint), self.trades.get(mint).cloned())
        {
            if let Some(fill) = find_worst_case_paper_exit_at(trades.as_slice(), fire_idx, true) {
                let sol = (token_amount as f64 / TOKEN_SCALE) * fill.price;
                let event = Event::FillConfirmed {
                    intent,
                    fill: Fill {
                        price: fill.price,
                        sol,
                        token_amount,
                        at: fill.block_time,
                    },
                };
                self.emit_or_defer(mint, fill.trade_idx, event, work);
                return;
            }
        }
        let price = self.price_of(mint).unwrap_or(0.0);
        let sol = (token_amount as f64 / TOKEN_SCALE) * price;
        work.push(Event::FillConfirmed {
            intent,
            fill: Fill { price, sol, token_amount, at: now },
        });
    }

    fn emit_or_defer(&mut self, mint: &Mint, fill_idx: usize, event: Event, work: &mut Vec<Event>) {
        let current = self.last_trade_idx.get(mint).copied();
        if current == Some(fill_idx) {
            work.push(event);
        } else {
            self.deferred_fills.insert(mint.clone(), DeferredFill { fill_idx, event });
        }
    }

    /// Build the `FillConfirmed` for an entry buy (mirrors `exec_paper::run_entry`).
    fn buy_fill(&self, intent: IntentId, lamports: u64, price: f64, at: Ts) -> Event {
        let sol = lamports as f64 / LAMPORTS_PER_SOL_F64;
        let token_amount = ((sol / price) * TOKEN_SCALE).round().max(0.0) as u64;
        Event::FillConfirmed { intent, fill: Fill { price, sol, token_amount, at } }
    }

    /// Record a position transition into the in-flight result row.
    fn on_position_update(&mut self, delta: hunter_engine::event::PositionDelta) {
        let sig = self.last_sig.get(&delta.mint).cloned().unwrap_or_default();
        match delta.status {
            PositionStatus::BuySubmitted => {
                self.builders.insert(
                    delta.position,
                    Builder {
                        mint: delta.mint.to_string(),
                        rule: delta.rule,
                        target: None,
                        entry_price: 0.0,
                        entry_token_amount: 0,
                        entry_time: Utc::now(),
                        entry_tx: String::new(),
                        entered: false,
                    },
                );
            }
            PositionStatus::Holding => {
                if let (Some(b), Some(fill)) = (self.builders.get_mut(&delta.position), delta.fill) {
                    b.entered = true;
                    b.target = self.pending_targets.remove(&delta.mint);
                    b.entry_price = fill.price;
                    b.entry_token_amount = fill.token_amount;
                    b.entry_time = fill.at;
                    // Prefer the fill trade's sig (adverse print); fall back to the
                    // last folded trade's sig when the slim corpus omitted it.
                    b.entry_tx = sig;
                }
            }
            PositionStatus::ExitPending => {}
            PositionStatus::End => {
                if let Some(b) = self.builders.remove(&delta.position) {
                    if let Some(fill) = delta.fill {
                        self.push_closed(b, fill.price, fill.at, sig, delta.reason);
                    }
                }
            }
            // Terminal without a confirmed fill. An entry give-up (never `entered`)
            // produces no row; an exit give-up books at the last known spot.
            PositionStatus::ExitFailed | PositionStatus::ExitUnconfirmed => {
                if let Some(b) = self.builders.remove(&delta.position) {
                    if b.entered {
                        let mint = Mint::from(b.mint.as_str());
                        let price =
                            self.last_price.get(&mint).copied().unwrap_or(b.entry_price);
                        self.push_closed(b, price, Utc::now(), sig, delta.reason);
                    }
                }
            }
        }
    }

    fn push_closed(
        &mut self,
        b: Builder,
        exit_price: f64,
        exit_time: Ts,
        exit_tx: String,
        reason: Option<ExitReason>,
    ) {
        let mint = Mint::from(b.mint.as_str());
        let last_price = self.last_price.get(&mint).copied().unwrap_or(exit_price);
        self.done.push(outcome_from_builder(b, Some(exit_price), Some(exit_time), Some(exit_tx), reason, last_price));
    }

    /// Drain the remaining in-flight builders — every still-held position becomes an
    /// **open** outcome marked to its last observed price.
    fn finish(mut self) -> Vec<PositionOutcome> {
        let builders = std::mem::take(&mut self.builders);
        for (_, b) in builders {
            if !b.entered {
                continue; // never entered (in-flight buy at end) — no row
            }
            let mint = Mint::from(b.mint.as_str());
            let last_price = self.last_price.get(&mint).copied().unwrap_or(b.entry_price);
            self.done.push(outcome_from_builder(b, None, None, None, None, last_price));
        }
        self.done
    }

    /// The token's current finite spot, or `None` when nothing has priced it yet.
    fn price_of(&self, mint: &Mint) -> Option<f64> {
        self.state
            .tokens
            .get(mint)
            .map(|t| t.track.current_price())
            .filter(|p| p.is_finite() && *p > 0.0)
    }

}

/// Human-SOL → lamports (`u64`, saturating at 0) for the first-slot axes the
/// fingerprint matcher buckets.
fn sol_to_lamports(sol: f64) -> u64 {
    if sol.is_finite() && sol > 0.0 {
        (sol * LAMPORTS_PER_SOL_F64).round() as u64
    } else {
        0
    }
}

/// A per-token result row in the exact serde shape the Simulated-tokens table
/// grammar ([`crate::strategies::sim_query`]) reads — same field names as the
/// legacy `BacktestTokenResult`, so the frontend + server-side query work
/// unchanged. `token` (enrichment) + `ath_price` are filled by the handler after a
/// bounded batch fetch.
///
/// Matched-but-never-entered tokens are emitted with `fired: false`,
/// `exit_reason: "NoEntry"`, and null entry/exit fill fields (parity with the
/// sweep combo drill-in's `ComboTokenResult`).
#[derive(Clone, serde::Serialize)]
pub struct EngineBacktestResult {
    pub mint_address: String,
    pub symbol: String,
    pub created_at: DateTime<Utc>,
    /// Whether the strategy took a position (entry fill landed).
    pub fired: bool,
    /// Trigger-trade snapshot (scalp/signal) — gap vs `entry_*` is adverse slippage.
    pub target_price: Option<f64>,
    pub target_token_amount: Option<f64>,
    pub target_time: Option<DateTime<Utc>>,
    pub target_tx: Option<String>,
    /// `None` when not fired (`NoEntry`).
    pub entry_price: Option<f64>,
    pub ath_price: Option<f64>,
    /// SOL notional the rule would deploy; `None` when not fired.
    pub entry_token_amount: Option<f64>,
    pub entry_tx: Option<String>,
    pub entry_time: Option<DateTime<Utc>>,
    pub exit_price: Option<f64>,
    pub exit_tx: Option<String>,
    pub exit_time: Option<DateTime<Utc>>,
    pub holding_secs: Option<i64>,
    pub pnl_percent: Option<f64>,
    pub pnl_sol: Option<f64>,
    /// `TakeProfit | StopLoss | Metrics | Dead | Manual | Migrated | Open | NoEntry`.
    pub exit_reason: String,
    #[serde(flatten)]
    pub token: crate::strategies::token_enrich::TokenEnrichment,
}

fn outcome_from_builder(
    b: Builder,
    exit_price: Option<f64>,
    exit_time: Option<Ts>,
    exit_tx: Option<String>,
    exit_reason: Option<ExitReason>,
    last_price: f64,
) -> PositionOutcome {
    let (target_price, target_token_amount, target_time, target_tx) = match b.target {
        Some(t) => (Some(t.price), Some(t.token_amount), Some(t.time), Some(t.tx)),
        None => (None, None, None, None),
    };
    PositionOutcome {
        mint: b.mint,
        rule: b.rule,
        target_price,
        target_token_amount,
        target_time,
        target_tx,
        entry_price: b.entry_price,
        entry_token_amount: b.entry_token_amount,
        entry_time: b.entry_time,
        entry_tx: b.entry_tx,
        exit_price,
        exit_time,
        exit_tx,
        exit_reason,
        last_price,
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use hunter_engine::event::{RuleId, TradeMode};
    use hunter_engine::fingerprint::FingerprintId;
    use hunter_engine::rule_params::RuleParams;
    use uuid::Uuid;

    fn base() -> Ts {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap()
    }
    fn at(secs: f64) -> Ts {
        base() + Duration::milliseconds((secs * 1000.0) as i64)
    }

    /// A fingerprint keyed on cu_limit only (instant match), so every test token
    /// with the same cu_limit arms.
    fn fp(id: u128) -> EngineFingerprint {
        EngineFingerprint {
            id: FingerprintId(Uuid::from_u128(id)),
            cu_limit: Some(200_000),
            cu_price: None,
            ix_labels: None,
            init_buy_lamports: None,
            max_cost_lamports: None,
            spendable_lamports_in: None,
            first_slot_buy_lamports: None,
            first_slot_sell_lamports: None,
            bucket_size_amount: 0.1,
            metric_config: serde_json::json!({}),
        }
    }

    fn rule(id: u128, fp_id: u128, params: serde_json::Value, cap: u32) -> LoadedRule {
        LoadedRule {
            id: RuleId(Uuid::from_u128(id)),
            fingerprint_id: FingerprintId(Uuid::from_u128(fp_id)),
            trade_mode: TradeMode::Paper,
            buy_amount_lamports: 1_000_000_000,
            max_concurrent_tokens: cap,
            max_total_tokens: 0,
            params: RuleParams::parse(&params).unwrap(),
        }
    }

    /// Observed axes matching `fp` (cu_limit 200_000).
    fn tf() -> TokenFingerprint {
        TokenFingerprint { cu_limit: Some(200_000), ..Default::default() }
    }

    fn trade(secs: f64, is_buy: bool, sol: f64, price: f64, real_reserve: f64) -> CorpusTrade {
        CorpusTrade {
            block_time: at(secs),
            amount_sol: sol,
            token_amount: 1000.0,
            price_per_token: price,
            reserve_sol: Some(real_reserve + 30.0),
            reserve_token: Some(1_000_000.0),
            real_reserve_sol: Some(real_reserve),
            real_token_reserves: None,
            slot: (secs as u64) + 1,
            leg_index: 0,
            is_buy,
            tx_signature: Some(format!("sig{secs}").into_boxed_str()),
            ix_labels: None,
            wallet: None,
        }
    }

    fn token(mint: &str, created: f64, trades: Vec<CorpusTrade>) -> ReplayToken {
        ReplayToken {
            mint: mint.to_string(),
            symbol: mint.to_uppercase(),
            created_at: at(created),
            tf: tf(),
            trades: Arc::new(trades),
            creator_wallet_hash: None,
        }
    }

    /// Enter-on-arm rule (no entry conditions), TP +100% — a price that doubles
    /// past the worst-case entry fill fires TakeProfit.
    #[test]
    fn enters_on_arm_and_takes_profit() {
        let rules = [rule(1, 1, serde_json::json!({ "take_profit": 100 }), 1)];
        let fps = [fp(1)];
        // Trigger → adverse entry buy in-window → later TP print (beyond the
        // MAX_FILL_WAIT_SLOTS entry window so it isn't absorbed into the fill).
        let t = token(
            "aaa",
            0.0,
            vec![
                trade(0.0, true, 1.0, 1.0, 100.0), // trigger
                trade(0.4, true, 1.0, 1.1, 100.0), // worst-case entry fill
                trade(10.0, true, 1.0, 2.5, 100.0), // +127% vs entry 1.1 → TP
            ],
        );
        let out = run_replay(&rules, &fps, vec![t], ReplayConfig { as_of: at(400.0) });
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].exit_reason, Some(ExitReason::TakeProfit));
        assert_eq!(out[0].target_price, Some(1.0), "trigger trade is the target");
        assert_eq!(out[0].entry_price, 1.1, "worst-case fill is the entry");
        assert!(out[0].exit_price.unwrap() >= 2.0);
    }

    /// Stop-loss fires when the price falls past the SL threshold.
    #[test]
    fn enters_on_arm_and_stops_out() {
        let rules = [rule(1, 1, serde_json::json!({ "stop_loss": 30 }), 1)];
        let fps = [fp(1)];
        let t = token(
            "bbb",
            0.0,
            vec![
                trade(0.0, true, 1.0, 1.0, 100.0),  // trigger
                trade(0.4, true, 1.0, 1.05, 100.0), // worst-case entry fill
                trade(10.0, false, 1.0, 0.5, 100.0), // −52% vs entry → SL
            ],
        );
        let out = run_replay(&rules, &fps, vec![t], ReplayConfig { as_of: at(400.0) });
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].exit_reason, Some(ExitReason::StopLoss));
    }

    /// A token whose reserves are gone and that goes quiet past the dead window
    /// books a tick-driven `Dead` exit (no trade drives it — the synthetic tail
    /// ticks do), proving the run-time as-of deadness.
    #[test]
    fn quiet_token_books_dead_via_tail_ticks() {
        let rules = [rule(1, 1, serde_json::json!({ "take_profit": 1000 }), 1)];
        let fps = [fp(1)];
        // Low real reserves (<30) + no trade after entry → dead 300 s later.
        let t = token(
            "ccc",
            0.0,
            vec![
                trade(0.0, true, 1.0, 1.0, 1.0),
                trade(0.4, true, 1.0, 1.0, 1.0), // entry fill
            ],
        );
        let out = run_replay(&rules, &fps, vec![t], ReplayConfig { as_of: at(1000.0) });
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].exit_reason, Some(ExitReason::Dead));
    }

    /// A token still trading (healthy reserves, no exit hit) at `as_of` stays open,
    /// marked to its last price.
    #[test]
    fn open_position_marks_to_last_price() {
        let rules = [rule(1, 1, serde_json::json!({ "take_profit": 1000 }), 1)];
        let fps = [fp(1)];
        let t = token(
            "ddd",
            0.0,
            vec![
                trade(0.0, true, 1.0, 1.0, 100.0),
                trade(0.4, true, 1.0, 1.05, 100.0), // entry fill
                trade(10.0, true, 1.0, 1.7, 100.0),
            ],
        );
        // as_of shortly after the mark print — not quiet long enough to die.
        let out = run_replay(&rules, &fps, vec![t], ReplayConfig { as_of: at(12.0) });
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].exit_reason, None, "still open");
        assert_eq!(out[0].last_price, 1.7);
    }

    /// The keystone cross-token cap parity: a `max_concurrent = 1` rule with one
    /// token held open must **block** a second matching token from entering — a
    /// property only a single shared `EngineState` in global time order can honor
    /// (a per-token-independent replay would enter both). Proves simulate applies
    /// caps exactly as the live loop does.
    #[test]
    fn concurrency_cap_shared_across_tokens() {
        let rules = [rule(1, 1, serde_json::json!({ "take_profit": 1000 }), 1)];
        let fps = [fp(1)];
        // Both healthy (never die) and never hit TP → the first holds open forever.
        // Each needs a post-trigger buy so the worst-case entry window can fill.
        let a = token(
            "aaa",
            0.0,
            vec![trade(0.0, true, 1.0, 1.0, 100.0), trade(0.4, true, 1.0, 1.05, 100.0)],
        );
        let b = token(
            "bbb",
            1.0,
            vec![trade(1.0, true, 1.0, 1.0, 100.0), trade(1.4, true, 1.0, 1.05, 100.0)],
        );
        let out = run_replay(&rules, &fps, vec![a, b], ReplayConfig { as_of: at(5.0) });
        assert_eq!(out.len(), 1, "cap=1 lets only one token hold at a time");
        assert_eq!(out[0].mint, "aaa", "the earlier token wins the single slot");
    }

    /// Replaying the identical scenario twice yields identical outcomes
    /// (determinism — the engine is pure and the producer is order-stable).
    #[test]
    fn replay_is_deterministic() {
        let rules = [rule(1, 1, serde_json::json!({ "take_profit": 100 }), 5)];
        let fps = [fp(1)];
        let mk = || {
            vec![
                token(
                    "aaa",
                    0.0,
                    vec![
                        trade(0.0, true, 1.0, 1.0, 100.0),
                        trade(0.4, true, 1.0, 1.1, 100.0),
                        trade(10.0, true, 1.0, 2.5, 100.0),
                    ],
                ),
                token(
                    "bbb",
                    1.0,
                    vec![
                        trade(1.0, true, 1.0, 1.0, 50.0),
                        trade(1.4, true, 1.0, 1.05, 50.0),
                        trade(10.0, false, 1.0, 0.4, 50.0),
                    ],
                ),
            ]
        };
        let cfg = ReplayConfig { as_of: at(1000.0) };
        let a = run_replay(&rules, &fps, mk(), cfg);
        let b = run_replay(&rules, &fps, mk(), cfg);
        let key = |o: &[PositionOutcome]| {
            let mut v: Vec<_> = o
                .iter()
                .map(|p| {
                    (
                        p.mint.clone(),
                        p.exit_reason.map(|r| r.label().into_owned()),
                        p.entry_price.to_bits(),
                    )
                })
                .collect();
            v.sort();
            v
        };
        assert_eq!(key(&a), key(&b));
    }

    /// Plan 5.3 tick guard — the replay driver ticks at the engine SSOT cadence, the
    /// same constant the live decision loop derives its interval from.
    #[test]
    fn tick_matches_engine_ssot() {
        assert_eq!(TICK.num_milliseconds(), hunter_engine::TICK_MS);
    }
}

/// Turn a replayed [`PositionOutcome`] into a result row, pricing the round-trip
/// through the shared [`CostModel`](trading_core::strategies::kernel::CostModel) so
/// PnL matches the sweep + the old single-rule simulate byte-for-byte. `created_at`
/// / `symbol` come from the token metadata; `buy_amount_sol` sizes the round-trip.
pub fn outcome_to_row(
    outcome: &PositionOutcome,
    symbol: &str,
    created_at: Ts,
    buy_amount_sol: f64,
) -> EngineBacktestResult {
    use trading_core::strategies::kernel::{quantize_f32, round_trip_with_costs, CostModel};

    let (exit_price_for_pnl, exit_price, exit_time, exit_tx, holding_secs, exit_reason) =
        match outcome.exit_reason {
            Some(reason) => (
                outcome.exit_price.unwrap_or(outcome.last_price),
                outcome.exit_price,
                outcome.exit_time,
                outcome.exit_tx.clone(),
                outcome.exit_time.map(|t| (t - outcome.entry_time).num_seconds()),
                reason.label().into_owned(),
            ),
            // Open at end of history — mark unrealized PnL to the last price; exit
            // fields stay None (no sell fired), reason "Open".
            None => (outcome.last_price, None, None, None, None, "Open".to_string()),
        };

    let (sol, pct) = round_trip_with_costs(
        outcome.entry_price,
        exit_price_for_pnl,
        buy_amount_sol,
        &CostModel::pumpfun_default(),
    );

    EngineBacktestResult {
        mint_address: outcome.mint.clone(),
        symbol: symbol.to_string(),
        created_at,
        fired: true,
        target_price: outcome.target_price,
        target_token_amount: outcome.target_token_amount,
        target_time: outcome.target_time,
        target_tx: outcome.target_tx.clone(),
        entry_price: Some(outcome.entry_price),
        ath_price: None,
        // Match the legacy row: the "entry_token_amount" column carries the SOL
        // notional the rule deployed (the frontend renders it as size).
        entry_token_amount: Some(buy_amount_sol),
        entry_tx: Some(outcome.entry_tx.clone()),
        entry_time: Some(outcome.entry_time),
        exit_price,
        exit_tx,
        exit_time,
        holding_secs,
        pnl_percent: Some(quantize_f32(pct)),
        pnl_sol: Some(quantize_f32(sol)),
        exit_reason,
        token: crate::strategies::token_enrich::TokenEnrichment::default(),
    }
}

/// A matched candidate that never entered — same wire shape as a fired row, with
/// null fills and `exit_reason: "NoEntry"` (parity with the sweep combo drill-in).
pub fn no_entry_row(mint: &str, symbol: &str, created_at: Ts) -> EngineBacktestResult {
    EngineBacktestResult {
        mint_address: mint.to_string(),
        symbol: symbol.to_string(),
        created_at,
        fired: false,
        target_price: None,
        target_token_amount: None,
        target_time: None,
        target_tx: None,
        entry_price: None,
        ath_price: None,
        entry_token_amount: None,
        entry_tx: None,
        entry_time: None,
        exit_price: None,
        exit_tx: None,
        exit_time: None,
        holding_secs: None,
        pnl_percent: None,
        pnl_sol: None,
        exit_reason: "NoEntry".to_string(),
        token: crate::strategies::token_enrich::TokenEnrichment::default(),
    }
}
