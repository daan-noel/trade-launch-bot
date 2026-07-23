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
  m_flow_window(30s) gross_flow >= ~10 SOL (hot-token filter).
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
Exists already: m_flow_window {buy, sell, net_flow, gross_flow} with window_size_sec;
liquidity + time (m_snapshot); stall; TP/SL; deadness; the generic replay/sweep
backtester inherits any fold change for free.
Missing (the actual work):
1. **Windowed-high drawdown metric** (entry): `trail` is % off the LIFETIME peak; the
   dip trigger needs % off a ROLLING window high (new metric in m_price_lifetime or a new
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
hurdle. This probe had to **drop the 30s gross hot gate** (the single-`m_flow_window`
limit) — now resolved below. Reinforces the STOP verdict.

**Both gates together (2026-07-22, schema limit lifted).** The single-`m_flow_window`-
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

**Deeper-dip + right-tail sweep (2026-07-22, `flow_scalper_deep_dip`).** The last two
untested levers the family distribution pointed at — both REJECTED:

| config | fired | winB | holdMed | pnlMed | realB | realA |
| --- | --- | --- | --- | --- | --- | --- |
| GATED d15/w30/r5 | 68 | 57.4 | 4.5s | −3.09 | **+0.60** | −2.25 |
| GATED d20/w30/r5 | 42 | 61.9 | 5.2s | −3.00 | +0.22 | −1.54 |
| GATED d25/w30/r5 | 18 | 44.4 | 3.6s | −4.23 | −0.05 | −0.80 |
| GATED d28/w30/r5 | 15 | 53.3 | 4.8s | −3.04 | +0.11 | −0.51 |
| GATED d20/w30/r8 | 42 | 59.5 | 5.4s | −3.00 | +0.15 | −1.61 |
| GATED d25/w30/r8 | 18 | 44.4 | 5.1s | −4.23 | −0.19 | −0.93 |
| TAIL d25/r8 stall=15s | 18 | 44.4 | 5.1s | −4.23 | −0.19 | −0.93 |
| TAIL d25/r8 stall=30s | 43 | 39.5 | 5.3s | −7.50 | −2.26 | −3.98 |
| TAIL d25/r8 stall=none | 76 | 32.0 | 8.4s | −10.65 | −5.22 | −8.19 |

- **(A) Deeper dip does not help — the trend runs backwards.** `realB` *decreases* as the
  dip deepens (+0.60 @ d15 → ~0/negative by d25) and `fired` collapses 68→15; win% *drops*
  57→44. Deeper dips thin the sample, they don't find a better subset. The shallowest
  config (d15/r5) is still the best one measured; no config anywhere is positive after cost.
- **(B) The 15s stall net was cutting LOSERS, not winners.** Dropping it *did* extend holds
  toward omego's 17s (p75 8.4s→20.6s) but wrecked everything: win% 44→32, pnlMed −4.2→−10.7,
  p10 −14→−35, realB −0.19→−5.22. Our positions are worse than omego's *at the same age*
  (feed-reactive entry, not same-slot near the bottom), so more time just lets them bleed.
  The hold-truncation gap vs omego is **structural latency, not a tunable exit.**

**Flow-gating byte-identical; deeper dips worse; exit-mix/hold worse.** These three
levers ARE spent. But the "STOP FINALIZED" conclusion first drawn here was **premature —
it double-counted slippage** (see the fill-sensitivity correction below).

**Fill-sensitivity probe = STOP verdict OVERTURNED (2026-07-22, `flow_scalper_fill_sensitivity`).**
The `−2.25`/`−1.72` "after-cost" figures every prior phase quoted (`realA`) are **wrong
when a fill model is in play**: `CostModel::pumpfun_default` charges ~1%/leg *slippage* on
top of the fill model's own adverse fill price — the same slippage counted twice. The
honest metric is `realFee` (fee + tip + priority only; the fill model is the sole slippage
source). Repricing the SAME taken set (identical fires; only price differs) under three
fill models:

| config / fill | realFee (**honest**) | realA (double-counts) | realB (before cost) |
| --- | --- | --- | --- |
| GATED d15/r5 · worst | −0.90 | −2.25 | +0.60 |
| GATED d15/r5 · **first** | **+0.52** | −0.85 | +2.04 |
| GATED d15/r5 · signal | −0.74 | −2.09 | +0.76 |
| EXH nf≥0 · worst | −0.50 | −1.72 | +0.85 |
| EXH nf≥0 · **first** | **+0.61** | −0.62 | +1.98 |
| EXH nf≥0 · **signal** | **+0.51** | −0.72 | +1.88 |

- **`signal` fill (zero-slippage) is POSITIVE for EXH (+0.51)** → the entry SIGNAL has
  genuine edge; the strategy is **not dead**. This is the plan's "worst ≤0 but signal >0 →
  loss is execution/latency, not the signal" branch.
- **`first` fill (next print — a realistically fast bot) is positive for BOTH configs**
  (+0.52 / +0.61). Only the adversarial `worst`-in-slot bound is negative (−0.50/−0.90).
- The **exhaustion config `EXH d15/r5 nf≥0` is the robust winner**: positive under `first`
  AND `signal`, negative only under `worst`. (GATED is fragile — `signal` −0.74 because its
  exit-side fill interaction hurts; the net_flow gate stabilizes it.)

**Corrected verdict: the one-shot clears breakeven (fee-only) under any fill better than
worst-case-adverse — so Phase 4's gate ("positive-after-costs one-shot") IS met, conditional
on the executor landing next-print-quality fills rather than worst-in-slot.** Caveats: the
margin is thin (+0.6 SOL fee-only / 61 eps / 11 lake days) and rests entirely on fill
quality — live paper currently books the `worst` model (−0.5), so realizing the edge needs
reprice-on-retry + busy-hours (Phase 6 §4). Phase 4 (re-entry) is now **justified** — it
amplifies a now-positive per-episode edge (up to 31 eps/token) instead of a negative one.

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
- **POSITIVE total after costs on the busy subset — ✔ (CORRECTED).** Using the honest
  `realFee` accounting (not the slippage-double-counting `realA`), the exhaustion config
  `EXH d15/r5 nf≥0` is **+0.61 SOL** under a `first`-print fill and **+0.51** under a
  `signal` fill — positive under every fill better than worst-in-slot. All fires land in
  the busy 07-20/07-21 window, so busy-subset == total. (The earlier ✘ used `realA` under
  worst-case fill = slippage counted twice; see the fill-sensitivity correction above.)

### Verdict — Phase 4 GATE MET (corrected 2026-07-22; supersedes the STOP below)
The original STOP was drawn from `realA` (full cost model) under a worst-case fill, which
**double-counts slippage**. Corrected to `realFee` + an explicit fill model, the 2-metric
core (with the `net_flow(2s)≥0` exhaustion gate) is **positive after fees under realistic
and zero-slippage fills** (+0.61 / +0.51 SOL), negative only under the adversarial
worst-in-slot bound. The entry signal has genuine edge (`signal` fill > 0); the residual
loss under `worst` is **execution/latency**, not the signal. Per Ph3 §4 this clears the
"positive-after-costs one-shot" gate, so **Phase 4 (re-entry) is unblocked** — conditional
on the executor achieving next-print fills (reprice-on-retry + busy-hours, Phase 6 §4).
Margin is thin, so re-validate with re-entry under the `first` fill before real money.

<details><summary>Superseded STOP verdict (kept for history — corrected above)</summary>

The 2-metric core is **directionally validated** (55–58% before-cost win, decisive
universe-filter delta, bounded losses, entry/hold in-band) but the one-shot variant is
only **~breakeven before costs and net-negative after** the realistic haircut — the
median round trip is ~0% and the edge lives entirely in a **thin right tail** a
one-shot, no-re-entry backtest under-samples. Per Ph3 §4 ("if the one-shot variant is
not clearly positive after costs, STOP and re-examine (entry refinement, exhaustion
gate) before building re-entry/sizing"), **do not proceed to Phase 4/5 on this v1.**

</details>

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

## Token filtering: what omego actually enters (2026-07-22, 42h window)

Re-analysed on the grown local window (2026-07-20 20:49 -> 07-22 14:43, 1.58M trades,
17,115 mints, omego = 1,013 buys / 136 mints). Scripts in the session scratchpad
(`omego_entry_features.sql`, `omego_selection.sql`, `omego_universe.sql`,
`omego_tokenlevel.sql`, `omego_lifecycle.sql`).

**Headline: token selection is not a side condition, it IS most of the edge.** He touches
136 of 17,115 mints = **0.8% of the universe**. The dip trigger only runs on a
pre-filtered, extremely narrow hot-list.

### Token-level profile (his 133 mints vs the 3,195 other mints that reached vsol >= 45)

| lifetime stat (median) | OMEGO mints | skipped (peak vsol >= 45) |
| --- | --- | --- |
| trade legs | **1,483** | 159 |
| unique wallets | **446** | 64 |
| volume | **674 SOL** | 94 SOL |
| peak vsol | **94.2** (p10 69, p90 115) | 55.8 |
| life (first->last trade) | **39.4 min** | 5.0 min |
| price max/min multiple | **9.2x** | 3.3x |
| reached vsol >= 110 (near migration) | **38.3%** | 10.9% |

He trades the small set of tokens that actually *run*: deep curve, hundreds of distinct
participants, tens of minutes of life. (Partly survivorship - those stats include the
future - hence the entry-time-observable set below.)

### Entry-time-observable state at his FIRST buy on a mint (n=136, no lookahead)

| feature | p10 | p25 | med | p75 | p90 |
| --- | --- | --- | --- | --- | --- |
| age | 1.1 min | 2.8 min | **5.3 min** | 12.5 min | 30.3 min |
| vsol | 51.8 | 61.7 | **73.5** | 82.4 | 95.3 |
| **off lifetime ATH** | -39.2% | -25.3% | **-15.1%** | -8.1% | -0.1% |
| prior trades | 206 | 303 | **561** | 913 | 1,163 |
| prior unique wallets | 103 | 140 | **221** | 325 | 404 |
| trades in last 60s | 10 | 40 | **92** | 243 | 389 |

Across **all** entries (incl. re-entries) off-ATH median drifts to **-31.8%** while
prior-trades rises to 1,307. So the *adoption* decision happens while the token is still
near its high; the re-entries then ride it down. **Token pick = near-ATH; episode
entries = anywhere.**

### The chosen token is the hottest thing on the chain at that instant

At each of the 136 first-buy moments, ranking every mint alive in the prior 60s:

- chosen token sits at the **88th-91st percentile** of the alive pool on trades/wallets/
  gross-flow/vsol (avg pool = 42 alive mints).
- rank by 60s trade count: **median 3**; **75/136 are top-3**, **111/136 are top-10**.
- chosen vs skipped medians at those moments: 92.5 vs 3.0 trades/60s, 66 vs 3 unique
  wallets/60s, 47.7 vs 1.1 SOL gross/60s, vsol 74.4 vs 31.1, and 60s price range
  **59.5% vs 2.6%**.

The rank is not a separate mechanism though - a 60s-grid funnel shows
`rank<=10 & vsol>=55 & w60>=25` (900 mints) is essentially the **same set** as the
rank-free `vsol>=55 & w60>=25 & gross60>=10` (898 mints). A **unique-wallets-per-window
threshold reproduces the ranking**. Rank is the readable diagnosis; the wallet count is
the implementable gate.

### Universe funnel over the 42h window (recall vs precision on his 136 mints)

| gate | mints passing | his mints | recall | precision |
| --- | --- | --- | --- | --- |
| everything traded | 17,115 | 136 | 100% | 0.8% |
| **our blueprint gate** `vsol 45-110 & gross60>=10` | 1,721 | 129 | 94.9% | **7.5%** |
| `vsol>=55 & w60>=25 & gross60>=10` | 898 | 125 | 91.9% | **13.9%** |
| `rank<=3 & vsol>=55` | 682 | 111 | 81.6% | 16.3% |
| token-level `peak_vsol>=60 & vol>=500 SOL` | 360 | 95 | 70.4% | 26.4% |

The blueprint gate the Phase-3 backtest used is **~2x too loose** and it was already
measured **non-binding** (the "both gates together" result above was byte-identical) -
consistent with the finding here: `gross_flow >= 10 SOL` is satisfied by almost anything
in the liquidity band, so it filters nothing. **The binding dimension is unique wallets
and trade velocity, which the engine has no metric for.**

### Engagement lifecycle (how the hot-list turns over)

- adopts **3.26 new mints/hour**; median **5 episodes** per mint (max 31); stays engaged
  **9.4 min** (p25 1.1, p90 75.5).
- concurrency: median **2** mints active per 5-min bucket, p90 4, max 6.
- abandonment is **cooling, not death**: at his last leg the token still does 17 trades in
  the next 60s (down from 55 in the prior 60s) and keeps trading another 13.4 min. He
  leaves when velocity halves, not when the token dies.
- his first buy lands at **17.6% (median) into the token's eventual life** - early in the
  run, not at the tail.

### Missing engine metrics implied by this (beyond the 2 in the impl plan)

1. **`unique_wallets` in `m_time_window`** - the single most discriminative gate
   (66 vs 3 per 60s). Requires a per-window distinct-wallet estimator (HLL or a small
   ring of hashed wallet ids); this is the highest-value new metric.
2. **`range` / realized volatility in `m_price_window`** - 60s high/low spread; his
   tokens run 56-60%, skipped ones 2.6%. Cheap: the `m_price_window` monotonic deques
   from Phase 1 already carry both extrema, so `range = (hi-lo)/lo` is free.
3. **Near-ATH adoption gate** - needs NO new metric: existing `m_price_path.trail`
   (% off lifetime peak) with `trail <= ~25` reproduces the first-buy condition
   (med -15%, p75 -8%). This is a one-line rule addition and should be tested first.
4. Optional: `trade_count` in `m_time_window` (velocity) - his 92/60s median vs pool 3.

### Fingerprint-axis grouping of his tokens (2026-07-22) - creation shape carries NO signal

Grouped his 136 mints by every engine fingerprint axis
(`hunter/engine/src/fingerprint.rs` + `grouping.rs`): `ix_labels` (ordered sequence),
`cu_limit`, `cu_price`, `init_buy_lamports`, `max_cost_lamports`,
`first_slot_{buy,sell}_lamports` (0.1 SOL buckets, engine `bucket_index` semantics),
`token_program_id`, `is_cashback_enabled`, `is_mayhem_mode`. Script:
`omego_fingerprint.sql` / `omego_fp_lift.sql`.

Axis coverage on his mints: ix_labels 136/136, first_slot_{buy,sell} 136/136,
init_buy 135, cu_price 110, cu_limit 109, max_cost 104. `spendable_lamports_in` is
**absent from every creation row in this window** (only `max_cost_lamports` and
`token_amount` are written) - that axis is currently unusable. `token_program_id` is a
single value (Tokenz...) and `is_mayhem_mode` is false for all 17,115 mints: both are
**constants, zero information**.

**The test that matters** is conditional on hotness - otherwise a fingerprint axis just
re-measures "this token got big". Restricting to the hot pool he picks from (950 mints
that reached vsol >= 45 and 200 SOL volume; 123 of them are his = **12.95% base rate**):

| axis | groups | chi2 | df | chi2/df | max lift (groups >= 20 mints) |
| --- | --- | --- | --- | --- | --- |
| first_slot_buy_sol | 320 | 337.9 | 320 | **1.06** | 1.93 (n=28) |
| cu_limit | 184 | 224.7 | 184 | **1.22** | 1.08 |
| cu_price | 142 | 164.9 | 142 | **1.16** | 1.76 |
| max_cost_sol | 85 | 110.1 | 85 | **1.29** | 1.76 |
| init_buy_sol | 94 | 83.8 | 94 | **0.89** | 1.76 |
| ix_labels | 15 | 25.0 | 15 | 1.67 | 1.26 |
| first_slot_sell_sol | 46 | 13.8 | 46 | 0.30 | 1.09 |
| is_cashback_enabled | 2 | 0.1 | 2 | **0.05** | 1.03 |

chi2/df ~ 1.0 is exactly the null expectation. **No fingerprint axis discriminates which
hot token he picks.** His mints spread across 17 of 54 ix_labels variants, 42 of 184
cu_limits, 82 of 418 full fingerprints - roughly proportional occupancy, not a cluster.
The largest lifts (1.76-1.93) sit on groups of 25-123 mints where the excess is 3-4
tokens - noise at this sample size.

The only directional hint: two `ix_labels` variants that lack any Compute-Budget
instruction (bare `Create_v2 | CreateIdempotent | Buy...`) go **0/53 and 4/86** vs ~13%
expected - he appears to skip manually/unsophisticatedly-created tokens. Weak, and
confounded with those tokens simply being thinner.

**Where the fingerprint DOES have power: predicting hotness, not his pick.** Over all
17,115 mints (base hot rate 5.55%): `init_buy >= 2 SOL` -> 10.0%, `>= 5 SOL` -> 13.2%,
`< 0.5 SOL` -> 1.6%. But as a tracking pre-filter its recall on his mints degrades in
lockstep with its recall on the hot pool:

| pre-filter | % of universe | recall of his 136 | recall of the 950 hot |
| --- | --- | --- | --- |
| `init_buy >= 0.5` | 56.9% | **89.7%** | 87.7% |
| `init_buy >= 1.0` | 48.4% | 80.9% | 80.6% |
| `init_buy >= 2.0` | 37.0% | 66.9% | 66.7% |
| `init_buy >= 3.0` | 29.1% | 55.1% | 57.2% |
| has Compute-Budget ix | 70.4% | 80.9% | 68.2% |

His-recall tracks hot-recall to within ~2pp at every threshold - conclusive that the
dev-buy axis selects for **hotness**, and adds nothing once hotness is known.

#### The combos he actually enters (ranked by entries, 1,013 buys / 136 mints)

`ix_labels` sequences (the only axis with any measurable effect), IDs by his entry count:

| id | sequence | mints | entries | hot pool | hit rate | lift |
| --- | --- | --- | --- | --- | --- | --- |
| IX1 | `SetComputeUnitLimit \| SetComputeUnitPrice \| Create_v2 \| CreateIdempotent \| BuyV2` | 47 | 273 | 167 | 22.2% | **1.71** |
| IX2 | `...Limit \| ...Price \| Create_v2 \| CreateIdempotent \| Buy \| Transfer` | 24 | 187 | 191 | 12.0% | 0.93 |
| IX3 | `Transfer \| Transfer \| Create_v2 \| ExtendAccount \| CreateIdempotent \| BuyExactSolIn` | 15 | 168 | 101 | 14.9% | 1.15 |
| IX4 | `...Limit \| ...Price \| Create_v2 \| CreateIdempotent \| Buy \| Transfer \| Transfer` | 12 | 126 | 91 | 13.2% | 1.02 |
| IX5 | `Transfer \| Transfer \| Create_v2 \| CreateIdempotent \| BuyExactQuoteInV2` | 7 | 88 | 43 | 16.3% | 1.26 |
| IX6 | `...Limit \| ...Price \| Create_v2 \| CreateIdempotent \| Buy \| Transfer x3` | 7 | 59 | 30 | 23.3% | **1.80** |
| IX7 | `...Limit \| ...Price \| Create_v2 \| CreateIdempotent \| BuyV2 \| Transfer` | 5 | 22 | 13 | 38.5% | 2.97 (n=13) |
| IX8 | `Create_v2 \| CreateIdempotent \| Buy` (bare, no compute budget) | 2 | 21 | 79 | 2.5% | **0.20** |

This is the one honest signal in the whole fingerprint space, and it is weak: modern
tool-built creates (`BuyV2` / compute-budget present) run ~1.7-1.8x, bare hand-rolled
creates (IX8) run 0.2x (2 of 79). It is a **"created by a bot/tool" proxy**, not a
strategy signal - and the axis-level chi2/df of 1.67 on 15 groups is only borderline.

Top combos at finer granularity (lift vs the 12.95% hot-pool base):

| level | combo | mints | entries | hot n | hit rate | lift |
| --- | --- | --- | --- | --- | --- | --- |
| B | IX3, cu `-`/`-` | 15 | 168 | 101 | 14.9% | 1.15 |
| B | IX2, cu 300000 / 3333333 | 21 | 163 | 162 | 12.3% | 0.95 |
| B | IX4, cu 300000 / 3333333 | 11 | 124 | 80 | 13.8% | 1.06 |
| C | IX2, 300000/3333333, init_buy 3.0 | 15 | 131 | 64 | 23.4% | 1.81 |
| C | IX4, 300000/3333333, init_buy 3.0 | 6 | 81 | 27 | 22.2% | 1.72 |
| C | IX3, `-`/`-`, init_buy 2.4 | 4 | 67 | 11 | 36.4% | 2.81 |

Fragmentation as axes are added - this is why fingerprint scoping fails here:

| grouping level | distinct combos (for 136 mints) | top-1 | top-5 | top-10 (% of his 1,013 entries) |
| --- | --- | --- | --- | --- |
| A `ix_labels` | 18 | 26.9% | 83.1% | 96.4% |
| B `+ cu_limit + cu_price` | 69 | 16.6% | 57.1% | 66.9% |
| C `+ init_buy` | 95 | 12.9% | 34.6% | 47.6% |
| D full engine identity (7 axes) | **128** | 3.4% | 14.3% | 24.4% |

At full identity his 136 mints scatter into 128 fingerprints - effectively one per token.
Only level A stays usable (5 sequences = 83% of his entries), and level A's lift is
~1.0-1.8. A rule scoped even to IX1 would cover 27% of his activity while still admitting
77.8% non-omego tokens.

**Design consequence.** Do NOT scope this strategy's rule to a narrow fingerprint. Use a
maximally-broad fingerprint (arm everything) and let the runtime metric gates filter;
optionally `init_buy >= 0.5 SOL` purely as a **tracking-cost** reduction (-43% of the
universe for -10% recall), never as an edge filter. This matches the Phase-3 harness
choice ("a broad fingerprint; `tf` feeds only matching, never a metric") and the earlier
sweep finding that fingerprint grouping over-fragments.

### Why a hand-picked few-token test cannot validate this strategy

Running the scalper logic over a small chosen token set removes the filter that produces
almost all of the edge (0.8% selectivity) and replaces it with an arbitrary, usually
lookahead-tainted universe. The Phase-3 harness is right in principle - arm everything
and let the metric gates filter - but its gates admit ~12x too many mints. Any validation
must run over the full lake with a gate whose precision is measured against this table.

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
