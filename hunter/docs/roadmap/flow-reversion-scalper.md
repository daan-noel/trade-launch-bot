# Flow-reversion scalper - external-wallet analysis + strategy blueprint (2026-07-21)

Reverse-engineering of three profitable scalper wallets the user tracks, from the local
pump.fun curve firehose (PG `trades`, 2026-07-20 20:49 -> 07-21 22:47 UTC, ~26h,
wallet-attributed). Goal: extract their logic and design a similar strategy for hunter.

Wallets (user nicknames):
- `omego` = omegoMAe1AMY5MFKQQr3JwXVy8F4eCvmBAfcpo8XAfq  <- fully analyzed (1,396 legs, 92 mints in window)
- `Co6` = Co6qnh3eHYd8FjyS5N6YXutUb3Z2GyKNPQHPURHaCK7T   <- absent locally (~2 tx/hour; trades outside our curve/fresh-token scope)
- `trunoest` = ardinRsN1mNYVeoJWTBsWeYeXvuR9UUDGMsCDKpb6AT <- absent locally; sig scan shows 1000/1000 recent txs FAILED within minutes = latency-race spam bot (snipe/arb); user sees only its landed residue

Analysis scripts + episode CSV live in the session scratchpad (throwaway); this doc is
the permanent record. Web research: all three are anonymous ground-vanity wallets; no
public open-source bot implements this strategy family (public repos = launch snipers or
wash-volume bots), so this reverse-engineering is proprietary edge.

## omego: the cracked logic

Headline (26h): 706 round-trip episodes on 92 mints, 62% win rate, gross net +27.9 SOL
on 614 SOL cycled (episode-consistent fold: closed realized + open-position mark;
excludes pre-window sell proceeds), every 3h bucket positive. After estimated pump.fun
fees (~1%/side, not in `amount_lamports`) + tips (~0.001/tx) the true net is roughly
+14..16 SOL/day - still clearly, stably profitable.

NOT what it looks like on a token page: "many buys and sells per token" is NOT
position laddering. Every episode is **1 buy -> 1 full sell** (median and p90 both 1
leg each side; sells are always 100% of balance). The multi-leg look comes from
**rapid re-entry**: 78/92 mints got >1 episode, max 31 episodes on one mint, median
gap exit->re-entry 24s (p25 4s).

### Universe selection (what it trades)
- Hot, established mid/late-curve tokens - NOT launch snipes:
  - token age at entry: med 13min, p10 2.8min, min 12s
  - curve depth (virtual SOL reserves): 37-115, med 75 (p90 104, near migration)
  - prior volume med 663 SOL, ~1,400 prior trades, ~450 unique wallets, ~2 trades/sec
- omego is only ~1.7% (med) of a token's volume - it reads flow, it does not make it.

### Entry trigger (the core insight - REJECTS the "sequential buy sum" hypothesis)
It **buys dips into sell pressure**, not buy surges:
- 81% of entries are >1% below the 30s rolling high; med entry is **-12.5% below the
  30s high** (-13.8% vs 60s high) and only +8.6% above the 30s low (p25 +1.7% = often
  within 2% of the exact local bottom).
- Others' cumulative sell SOL from the rolling high to his entry: med 9.4 SOL
  (p25 3.5, p75 21). Dip age (time since the high): med 13s.
- Net market flow 1-5s BEFORE entry is negative (med -0.7..-1.0 SOL; p25 -4.7)
  while the 30s window is >= 0 -> short dip inside an otherwise-hot token.
- Net flow 5s AFTER entry flips positive (med +0.4) -> it times sell-exhaustion
  inflections well (or its entry triggers other momentum bots).
- NOT copy/big-buy triggered: only 36% of entries have a >=0.5 SOL market buy in the
  prior 1s; the immediately-preceding trade is a buy only 53% of the time.
- Reaction speed: med 0.112s after the previous market trade, median slot delta 0
  (same-slot landing) - custom low-latency bot.

### Sizing
- 0.43-1.34 SOL per entry, med 0.87 - scales with curve depth at a near-constant
  **~1.1-1.2% of virtual SOL reserves** (0.61 @ vsol 40-60 -> 1.26 @ vsol 100-120),
  i.e. constant price impact per trade (~2.4%/round trip). Max ~6 concurrent positions.

### Exit trigger (REJECTS the "breakage/no-trades" hypothesis)
- Hold: med 17.3s (p25 2.5s, p75 72s). Winner and loser holds are identical -> the
  exit is price-action-driven, not time- or PnL-schedule-driven.
- Winners exit at med **-1.4% off the episode peak** (p75 -1.0%) -> a ~1-1.5%
  trailing stop off the post-entry peak.
- Losers exit med -8.8% off peak (the same trail, gapped through by fast dumps);
  76/259 loss exits are NOT at the episode low - it cuts, it does not bag-hold.
- No fixed TP/SL: the exit-PnL histogram is smooth (no walls at +-10/15%), median win
  +7.6%, median loss -5.3%, fat right tail (30 episodes closed >+40%), p10 loss -25%.
- Exits happen in DENSE flow (med 0.1s since last market trade) - it needs flow for
  exit liquidity; there is no "no-trades-for-N-sec" exit.
- 22 episodes still open at window end were mostly WINNERS still running (med +7%,
  p75 +17%) - the trail lets big winners ride for hours.

### Infrastructure fingerprint
Direct pump.fun program calls (no Photon/Axiom/Jupiter router): ComputeBudget + ATA
create + Buy/BuyV2/Sell(+V2) + CloseAccount + tip transfer, exactly 1 curve leg per tx,
~23% submit-fail rate, same-slot reaction. (Sell "Pump.Fun: Unknown" label = SellV2
discriminator missing from our decode table - minor decode gap worth adding.)

### The ecosystem (validation)
At least 5-6 other wallets run the same shape bigger in the same window: ARu4n5mF
(2,535 legs / 87 mints / 920 SOL), GVVP8N7j (1,233 / 64 / 930), 64hP97Bw (802 / 78 /
1,015), JDfuh8jY (1,239 / 68 / 498), FYTVwP5h (807 / 65 / 393), SQHK48QT (532 / 76 /
676). 45 wallets show >=100 legs, balanced buy/sell, >=10 mints. This is a validated
strategy family with real competition (and those wallets are follow-up study targets).

## Family-wide validation (same 26h window, ALL 1.1M local trades)

Widened the scan to every wallet with >=100 legs, 35-65% buys, >=10 mints: **1,033
analyzable bot wallets, 668 net-positive (65%), +3,580 SOL gross extracted as a group**
(gross = before ~2%/round-trip fees+tips, and includes some launch-sniper outliers).
The ecosystem splits into archetypes by entry geometry + hold time:

- **Dip-reversion scalpers** (the omego family; most common consistent earners):
  benchmark `64hP97Bwr5` (+97.8 gross, 2,030 episodes, hold med 20.7s, entry -20.7% vs
  30s high, trail -3.5%, size 1.86% of vsol, 100% one-buy purity, re-entry 30s). Also
  `SQHK48QT` (+70.3 / 1,107 eps), `9999huSCf6` (+57.8 / 553), `GVVP8N7jnx` (+35.9 /
  1,336), `CCCCQCrL6z` (+36.5 / 355), omego (+27.9 / 683, rank ~#29).
- **Fast momentum scalpers** (buy strength, 1-3s holds): `Anubis512h` (+59.3, 850 eps,
  hold 1.7s, entry +5% ABOVE 30s high, re-entry 0.4s), `24678QKx2D` (+30.3, 898 eps).
- **Sub-second sandwich/MEV-ish** (hold <1s): `GmNi3xSt4z` (+43.9, 294 eps, hold 0.7s),
  `Gku8cuthhv` (+21.4, 543 eps, 78% win).
- **Launch snipers/insiders** (huge per-episode wins, tiny buys at creation, look
  unreplicable/insider): `7p4AkPb9AQ` (+455 on 23 episodes, 100% win). Excluded from
  the blueprint stats.

**Blueprint parameter distributions - profitable dip-buyers only (~48K episodes):**
- entry dip vs 30s high: p25 -28.2% / med -14.3% / p75 -3.6%
- winners' trail retrace off episode peak: p25 -12.2% / med -4.7% / p75 -1.3%
- hold: p25 5s / med 15.3s / p75 45s ; re-entry gap: p25 9s / med 31s / p75 99s
- size: p25 0.31% / med 0.82% / p75 1.84% of vsol
- episode pnl: p25 -12.4% / med ~0% / p75 +13.5% / p90 +34.9% -> the edge is the
  right tail, not the median round trip; fee drag (~2%/round) kills marginal configs.

Implication for the sweep: dip depth {8,15,25}%, trail {1.5,3,5,10}%, size
{0.5,1,2}% of vsol, re-entry cooldown {5,30}s are the empirically-supported ranges.

## Strategy blueprint for hunter: "dip-reversion scalper"

- Universe gate (entry AND-conditions): age >= ~120s; liquidity (vsol) in ~[45, 110];
  m_time_window(30s) gross_flow >= ~10 SOL (hot-token filter).
- Dip trigger: drawdown from the 30-60s rolling high >= ~8-15% AND short-window (2s)
  net flow no longer strongly negative (dump pausing). v1 can skip the exhaustion
  refinement and accept knife-catches (omego eats them too: p10 loss -25%).
- Size: ~1% of vsol, clamped [0.4, 1.3] SOL (constant impact).
- Exit (OR): trailing stop ~1.5-3% off the SINCE-ENTRY peak; catastrophe SL ~-25%;
  deadness verdict (exists); long stall (>=15s no trades) as safety net only.
- Re-entry: re-arm the same (token, rule) after exit with a ~2-5s cooldown;
  concurrent_cap ~4-8 across tokens; bound episodes/token (max seen: 31).
- Expected profile (from omego, before our latency delta): ~60% win, med +2.3%/round,
  edge concentrated in busy hours; quiet-hour rounds are ~breakeven after fees.

### Engine fit (hunter-engine) - what exists vs what is missing
Exists already: m_time_window {buy, sell, net_flow, gross_flow} with window_size_sec;
liquidity + time (m_snapshot); stall; TP/SL; deadness; the generic replay/sweep
backtester inherits any fold change for free.
Missing (the actual work):
1. **Windowed-high drawdown metric** (entry): `trail` is % off the LIFETIME peak; the
   dip trigger needs % off a ROLLING window high (new metric in m_price_path or a new
   dynamic group with window_size_sec).
2. **Since-entry-peak retrace metric** (exit): the trailing stop needs the post-entry
   peak, not the lifetime peak.
3. **Re-entry lifecycle**: ArmState is one-shot (Done is terminal per (token, rule)).
   Needs re-arm after Done + cooldown + episode counter (extend RuleCounters).
4. **Liquidity-proportional sizing**: buy size is a fixed rule param today; needs
   pct-of-vsol sizing with clamps.
5. Optional later: sell-run-exhaustion detector; unique-wallets-window metric.

### Verification path
1. Add metrics (1)+(2); backtest a ONE-SHOT variant (no re-entry) via the existing
   simulate/sweep on the lake (8+ sealed days available; the wallet column is NOT
   needed for backtesting). Sweep dip depth {5,8,12,15}%, trail {1,2,3,5}%, window
   {30,60}s, liquidity band, hot-flow threshold.
2. Compare sim distributions (entry dip depth, hold, pnl%) against omego's actuals.
3. Add re-entry lifecycle (3) + sizing (4); re-sweep; then paper-trade live.

## Phase 3 — one-shot backtest results (2026-07-21, engine metrics (1)+(2) shipped)

Ran the real engine fold (`run_replay`, not the sweep) over the **whole sealed lake —
11 days, 7,636 tokens, 730,179 curve trades** — via a headless harness
(`hunter/lab/tests/flow_scalper_validation.rs`, `#[ignore]`; same `CostModel` +
fill model the live `engine_sim` uses). Every token armed (a broad fingerprint; `tf`
feeds only matching, never a metric), so the rule's metric gates do the filtering —
the faithful model of a universe defined by age/liquidity/flow, not creation shape.

**Costs + fill realism were ALREADY modelled** (this corrects the plan's Ph3 §2/§3
premises — no new knob needed):
- `kernel::CostModel::pumpfun_default()` charges ~1%/leg fee + ~1%/leg slippage + tip
  + priority fee ≈ **~4%/round** (more conservative than the plan's 2%/round), applied
  to realized PnL at close by `round_trip_with_costs` — the exact path `outcome_to_row`
  already used. Numbers below are net of this unless marked "before costs".
- Fills are **worst-case adverse in the slot window AFTER the signal** (entry = highest
  qualifying buy price, exit = lowest price, in trigger-slot + next slot ≤
  `MAX_FILL_WAIT_SLOTS`), so the sim already books our feed-reaction slippage — no
  "fill at signal price" optimism to re-add.

**One-shot grid (no re-entry — that is Phase 4), net of ~4%/round costs:**

| config (dip% / win s / retrace%) | fired | win% before | hold med | dip med | ep pnl% med | p10 | realized after | realized before |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| MIN core 12/30/3 (no gates)      | 2752 | 23.0 | 3.2s | 15.3 | −7.63 | −20.5 | **−179.6** | −66.9 |
| GATED 12/30/3                    |   98 | 55.1 | 4.3s | 13.0 | −3.65 | −11.1 | −3.69 | +0.40 |
| GATED 12/30/5                    |   98 | 57.1 | 6.0s | 13.0 | −3.65 | −11.9 | −3.83 | +0.26 |
| GATED 12/30/8                    |   98 | 57.1 | 7.0s | 13.0 | −3.36 | −13.5 | −3.86 | +0.22 |
| GATED 12/30/12                   |   98 | 57.1 | 8.4s | 13.0 | −3.36 | −15.7 | −4.10 | −0.01 |
| GATED 15/30/5                    |   68 | 57.4 | 4.5s | 15.9 | −3.09 | −12.7 | **−2.25** | **+0.60** |
| GATED 12/60/5                    |   98 | 58.2 | 6.0s | 13.0 | −3.48 | −11.9 | −3.53 | +0.56 |

**Sell-exhaustion probe** (top recommended lever; base = GATED 15/30/5, then trade the
30s gross gate for a 2s `net_flow` floor — schema forces one or the other, see note):

| variant | fired | win% before | hold med | ep pnl% med | realized after | realized before |
| --- | --- | --- | --- | --- | --- | --- |
| base GATED 15/30/5 (30s gross gate) | 68 | 57.4 | 4.5s | −3.09 | −2.25 | +0.60 |
| + `net_flow(2s) ≥ −1` (drop gross)  | 63 | 55.6 | 4.1s | −3.57 | −2.14 | +0.50 |
| + `net_flow(2s) ≥ 0` (drop gross)   | 61 | **59.0** | 4.1s | −3.05 | **−1.72** | **+0.85** |

`net_flow(2s) ≥ 0` (buy only once the 2 s sell pressure is no longer negative) lifts
win% 57.4 → 59.0 and cuts the loss ~24% (best after-cost −1.72, best before-cost +0.85)
— so the exhaustion gate **is** the right lever, but it does NOT clear the ~4%/round
hurdle. This probe had to **drop the 30s gross hot gate** (the single-`m_time_window`
limit) — now resolved below. Reinforces the STOP verdict.

**Both gates together (2026-07-22, schema limit lifted).** The single-`m_time_window`-
per-side limit is gone: `strategy-redesign` now lets a side carry **multiple windows per
group** (`SideConditions` holds a `Vec<GroupConditions>`; a dynamic group parses as a
JSON array of window clauses — `rule_params.rs`, engine-only, no DB migration). Re-ran
the probe with the **combined** rule keeping BOTH the 30s `gross_flow ≥ 10` hot gate and
the 2s `net_flow` floor in one rule:

| variant | fired | win% before | ep pnl% med | realized after | realized before |
| --- | --- | --- | --- | --- | --- |
| net_flow(2s) ≥ −1 only (drop gross) | 63 | 55.6 | −3.57 | −2.14 | +0.50 |
| **gross(30s) + net_flow(2s) ≥ −1**  | 63 | 55.6 | −3.57 | −2.14 | +0.50 |
| net_flow(2s) ≥ 0 only (drop gross)  | 61 | 59.0 | −3.05 | −1.72 | +0.85 |
| **gross(30s) + net_flow(2s) ≥ 0**   | 61 | 59.0 | −3.05 | −1.72 | +0.85 |

The combined result is **byte-identical** to net-flow-only at each floor: the 30s
`gross_flow ≥ 10` gate is **non-binding** on this universe — every token that has dropped
15%+ over a 30s window with 45–110 SOL liquidity already clears 10 SOL of 30s gross flow,
so the hot gate removes nothing the age/liquidity/dip/net_flow gates haven't. (The 2s net
gate IS binding: combined fires 61/63 vs the gross-only base's 68; and the engine unit
test `multi_window_group_compiles_to_distinct_reqs_and_windows` confirms both windows
compile to distinct reqs and are registered — so this is genuine redundancy, not a
dropped gate.) **The earlier probe did not understate the refinement; the STOP verdict
stands unchanged** — best after-cost is still −1.72 SOL. Lever #1 is now *tested and
exhausted*, not blocked: the gross hot gate is not the missing edge. The remaining levers
are fill/latency realism (#2) and, if pursued at all, a fundamentally different entry
signal — not more flow gating.

### Read vs the acceptance gates
- entry dip depth (median 13–16%, ≥12 by construction) — inside/near the family band
  [−8, −20%]. ✔ (threshold-driven, so this gate is near-circular for a `trail>=k` entry).
- hold median in [5, 60]s — ✔ for retrace ≥ 5% (6.0–8.4s); marginal at retrace 3% /
  dip 15 (4.3–4.5s). Wider retrace lengthens holds exactly as the too-tight-trail
  diagnosis predicted (4.3s → 8.4s), but even retrace 12 only reaches 8.4s vs omego's
  17s — our feed-reactive fills + the 15s stall net cap the hold.
- win rate 55–70% **before costs** — ✔ (gated 55–58%; the entry mechanism finds the
  regime). The universe filter is decisive: it flips win% 23 → 55+ and realized-before
  −66.9 → ~breakeven (delta **+67 SOL before costs / +176 after**).
- losses bounded, p10 ≥ −25% — ✔ (−11 to −16%; the −25 catastrophe SL rarely fires).
- **POSITIVE total after costs on the busy subset — ✘.** Best is GATED 15/30/5 at
  **−2.25 SOL** after ~4%/round (**+0.60 before**). The universe gates already select
  the busy 07-20/07-21 window (all fires land there), so busy-subset == total here.

### Verdict — STOP before Phase 4/5 (the plan's own gate)
The 2-metric core is **directionally validated** (55–58% before-cost win, decisive
universe-filter delta, bounded losses, entry/hold in-band) but the one-shot variant is
only **~breakeven before costs and net-negative after** the realistic haircut — the
median round trip is ~0% and the edge lives entirely in a **thin right tail** a
one-shot, no-re-entry backtest under-samples. Per Ph3 §4 ("if the one-shot variant is
not clearly positive after costs, STOP and re-examine (entry refinement, exhaustion
gate) before building re-entry/sizing"), **do not proceed to Phase 4/5 on this v1.**

Wider retrace does NOT help (realized-before degrades +0.40 → −0.01 as r3 → r12): a
wider trail gives winners back more and lets losers ride (p10 −11 → −16), and the exit
mix shifts to the 15s stall net firing first. The levers that matter, in priority:
1. ~~**Sell-exhaustion entry gate**~~ — **TESTED & EXHAUSTED (2026-07-22).** The 2s
   `net_flow ≥ 0` floor is the best single lever (−1.72 SOL after cost, +0.85 before), but
   it does not clear the ~4%/round hurdle. The schema limit that once forced dropping the
   30s gross gate is lifted (multi-window per group; see "Both gates together" above), and
   with BOTH gates the result is **byte-identical** — the 30s `gross_flow` hot gate is
   non-binding on this universe. Flow-gating is not the missing edge.
2. **Fill/latency reality** — our worst-case feed fills pay slippage omego avoids with
   same-slot landing; needs reprice-on-retry + busy-hours-only before real money.
3. **Re-entry (Phase 4) amplifies but does not create per-episode edge** — building it on
   a ≤0 per-episode expectancy just scales the loss. Gate it on a positive-after-costs
   one-shot first.

## Data caveats / next data steps
- Window = one ~26h weekday slice; one wallet fully analyzed. Fees/tips estimated,
  not measured (amount_lamports is curve-side; pump.fun ~1%/side fee + ~0.001 tip/tx
  are on top).
- Exit inference: a flow-based exit ("sell into the first sizeable market sell after a
  bounce") is observationally near-identical to a 1-1.5% trail; either implementation
  should reproduce the profile.
- EC2 holds ~30 days of wallet-attributed trades + 7 days raw_txs; a full
  `scripts/db-incremental-sync.ps1` run extends this analysis to ~a month at ZERO
  Helius cost (local raw_txs is empty; lake days 07-01..07-08 lack the wallet column).
- Helius spend is critically sensitive (user directive): no RPC fetches for analysis
  without explicit approval. Co6/trunoest cannot be characterized further from local
  data - they never touch the fresh-curve tokens our ingest tracks.
