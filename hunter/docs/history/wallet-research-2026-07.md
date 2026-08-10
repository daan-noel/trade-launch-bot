# Wallet research journal — external scalper reverse-engineering (2026-07-21 → 07-31)

> **History.** The run-by-run investigation record: every measurement pass, the
> hypotheses that were rejected, the verdicts that were later overturned, and the
> intermediate gate readings. Kept because the raw mechanical data (dip %, trail %,
> sizing %, entry timing, per-episode PnL by bucket) is not reproducible — the scratch
> tables and corpora it was measured on are gone.
>
> **The surviving conclusions live in
> [`@plans/strategies/wallet-analysis.md`](../plans/strategies/wallet-analysis.md).**
> Read that instead unless you need the primary data. Sections below marked *retired*,
> *superseded* or *STOP* were overturned later in this same file — do not act on them.

---

Original scope note (2026-07-21): reverse-engineering of three profitable scalper
wallets the user tracks, from the local pump.fun curve firehose (PG `trades`,
2026-07-20 20:49 -> 07-21 22:47 UTC, ~26h, wallet-attributed).

Wallets (user nicknames):
- `omego` = omegoMAe1AMY5MFKQQr3JwXVy8F4eCvmBAfcpo8XAfq  <- fully analyzed (1,396 legs, 92 mints in window)
- `Co6` = Co6qnh3eHYd8FjyS5N6YXutUb3Z2GyKNPQHPURHaCK7T   <- absent locally (~2 tx/hour; trades outside our curve/fresh-token scope)
- `trunoest` = ardinRsN1mNYVeoJWTBsWeYeXvuR9UUDGMsCDKpb6AT <- 07-21: absent locally; sig scan showed 1000/1000 recent txs FAILED = latency-race spam. **07-31 UPDATE: now present in the rebuilt window (730 landed legs / 255 mints) and fully analyzed — see the `trunoest` section at the bottom. It is a momentum-IGNITION pump-rider (Axiom + durable-nonce spam), a different family from the dip-reversion scalpers.**

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

## Re-derivation on a fresh 5-day window (2026-07-27) - logic CONFIRMED, knobs drifted

The local DB was rebuilt: the old 07-20..07-22 window is gone; the new window is
**2026-07-22 18:47 -> 07-27 16:08 UTC (~4.9 days, 6.48M trades, 67,806 mints)**. omego
re-analyzed from scratch on it: **3,160 buys / 3,113 sells / 446 mints / 2,974 closed
episodes** (3x the old sample; scripts re-run from the prior session's scratchpad +
a new `omego_episodes.sql`). Everything structural replicates; a few knobs moved.

**Headline (before fees/tips):** 59.1% win, **+108.4 SOL gross on 2,498 SOL cycled**,
every day positive (07-24 +17.1, 07-25 +39.6, 07-26 +24.5, 07-27 +28.6). Median win
+8.5%, median loss -6.7%, p10 -14.3%. Est. costs (~1%/side fee ~51 SOL + tips ~6 SOL)
put the true net around **+10..13 SOL/day** - same as the old estimate.

**Confirmed unchanged (new-window numbers):**
- 1 buy -> 1 full sell episodes: buys/sells per episode med AND p90 = 1 (max 4);
  2,974 of 3,050 episodes fully closed.
- Dip entry into sell pressure: entry dip vs 30s high med **-12.6%** (old -12.5);
  others' net flow before entry negative (net_1s/2s/5s med -0.09/-0.25/-0.39,
  p25 -2.7/-3.2/-4.5) and flips **+0.45 med in the 5s after**; reaction med **0.11s**
  after the previous market trade.
- Trail exit, no TP/SL: winners exit med **-2.97% off the episode peak** (p25 -6.1 /
  p75 -2.4); losers -13.2% off peak, 68.8% of loss exits at the episode low; exits in
  dense flow (med 0.10s since last market trade); winner and loser holds identical.
- Near-ATH adoption: FIRST buy med **-15.2% off lifetime ATH** (p75 -7.8; old -15.1),
  all-entries med -32.4% - re-entries ride the token down, unchanged.
- Token pick = hottest alive: chosen mint med **rank 4** by 60s trades (45% top-3,
  77% top-10 of an avg 49-mint alive pool); wallets60 med 54 vs 3 for skipped;
  60s range 57.8% vs 2.7%. Selectivity 446/67,806 = **0.66%**.
- Sizing is *literally* constant-impact: first-buy pct-of-vsol **p25 = med = p75 =
  1.18%** (0.46 SOL @ vsol~36 -> 1.25 @ ~107; med 0.82 SOL).
- Universe: vsol at entry p10-p90 48-100 (med 70); age med 15.8 min (first-entry med
  6.3 min, p10 0.9 min); prior trades med 1,178; prior wallets med 389.
- His mints vs skipped (peak vsol>=45): 1,326 vs 161 legs, 422 vs 64 wallets, 636 vs
  96 SOL vol, life 53.8 vs 4.9 min, px multiple 9.2x vs 3.5x, 36% vs 15% reach
  vsol>=110. First buy lands med 15% into the token's life.
- Abandonment is cooling, not death (trades/60s 37 -> 15 at his last leg; the token
  trades another 15.3 min).

**Changed vs the 07-21/22 window:**
- **Holds are longer:** med 22.5s (was 17.3), p75 98s, p90 249s. PnL by hold bucket:
  >1min holds contribute over half the money (+56 of +108 SOL); sub-5s scalps are
  878 eps for only +19 SOL. The trail is wider than first estimated: **~3% off peak**
  (was read as 1-1.5%).
- **Re-entry slower but deeper:** gap med 34.6s (was 24), p75 172s; episodes/mint med
  5, **max 38**; concurrency med 3 / p90 4-5 / max 7-8; adopts 3.85 new mints/hour.
- **NEW - re-entries do not decay, they improve:** episode #1 on a mint is his WORST
  (53.4% win, +0.52% med) vs 2-3 (59.8%, +2.01), 4-8 (59.3%, +2.02), 9+ (61.2%,
  +2.76). The first buy is a cheap probe; the money is in riding the confirmed runner
  via re-entries. `max_episodes_per_token` should not be small - his edge concentrates
  in episodes 4+.
- **NEW - within the gated pool, entry moments look unpicked:** at gate-passing
  moments (vsol 45-110, gross60>=10, dip30<=-12) his entered vs skipped moments are
  nearly identical (trades60 110 vs 100, wallets60 72 vs 62, gross60 51.3 vs 51.0,
  rank 3 vs 4, dip -18.9 vs -19.2). Beyond hot-list + dip there is no finer hidden
  trigger visible - which qualifying dip he takes looks capacity-limited (he holds
  only ~3-4 positions). Replication needs the gates + concurrency, not a magic signal.
- Fingerprint mix drifted: the top-5 ix_labels sequences now cover only ~71% of his
  entries ("other" is the largest single group at 902 entries / 127 mints) -
  reinforces the do-NOT-scope-by-fingerprint conclusion. IX1 is still the thin-token
  outlier (gross60 med 13.5 vs 33-56 for the rest; trail30 med 6.7 vs 14-16).
- Engine-metric calibration drift (fed the now-retired `fs-*` seed-rule knobs - the
  omego-calibrated fingerprint A/B was superseded by the `fs2-*` ladder below once
  [flow-scalper-findings.md](../plans/strategies/flow-scalper-findings.md) confirmed his gross edge doesn't
  clear the fee):
  trail30 p25/med/p75 = 3.5/**12.6**/22.7 (was 6.1/14.6/24.5); gross60 p25 = **11.2**
  (was 14.5); liquidity p25/p90 = 56.7/100.4; time p10 = 144s; net2 med -0.2 (an
  `nf>=0` floor still excludes ~half his entries - it remains a backtest-derived
  gate, not a mimic of his behavior).
- Moment-level gate grid (60s snapshots, 270K moments): best lift is
  `liq 55-115 + gross60>=40` -> 5.5% precision / 24.5% entry-moment recall (lift 6.4);
  adding rise/trail gates LOWERS precision. Mint-level, `vsol>=55 & w60>=25 &
  gross60>=10` -> 381/446 = 85% recall at 9.7% precision (was 92%/13.9%). The
  unique-wallets gate stays the top engine gap.

### Gate-payoff measurement (2026-07-28) - what a new metric would actually buy

Before writing any engine metric, the candidate gates were measured directly in SQL on
the new window: a 60s grid (270,027 mint-moments, 2,298 of them omego-entry moments =
0.851% base rate), then a shared base of `vsol 45-110 & trail60 >= 12` (15,305 moments,
670 omego, **4.38% precision**), then each candidate gate swept over that base.
Script: `gate_payoff.sql` (session scratchpad).

| gate | best precision (at recall) | verdict |
| --- | --- | --- |
| `gross_flow(60)` - what ships today | 4.24-4.46% at 25-88% recall | **dead** - precision is FLAT while recall collapses |
| `unique_wallets(60)` - proposed | 4.51 -> 5.02 -> 5.07% at 94 -> 63 -> 47% recall | **worth building** - monotone lift, strictly dominates gross |
| `trade_count(60)` - proposed | 4.56 -> 4.82% | **redundant** - corr 0.936 with unique_wallets |
| `range(60)` - proposed | 4.27 -> 3.91 -> 3.11% as the floor rises | **DO NOT BUILD - it anti-selects** |

Head-to-head at matched recall the crowd gate wins on *both* axes: `w60>=30` gives 71.9%
recall at 4.86% precision where `gross60>=40` gives only 46.3% at 4.24%. Combining gates
does not help (`w60>=30 & gross60>=20` = 4.82%, *worse* than `w60>=30` alone; adding
range makes it 4.71%) - `gross_flow` is pure recall cost once wallets are counted.

**The `range` reversal corrects an earlier claim in this doc.** The "his tokens run
56-60% vs 2.6%" comparison (see "Missing engine metrics", item 2) was measured against
*all* alive mints, most of which are dead. Conditioned on the base gate - already in the
liquidity band, already dipping 12% - a **wide 60s range predicts the WRONG tokens**:
precision falls 4.38 -> 3.11% as the floor rises 0 -> 90%. A token that already swung 90%
in a minute is a blow-off he avoids. Item 2's "cheap and high-value" recommendation is
**withdrawn**; `range` is cheap but negative-value here.

**The ceiling this exposes.** The best single gate available tops out near **5.0%
precision at ~63% recall** - a 1.15x lift on the 4.38% base, not the 5-6x hoped for.
Combined with the earlier finding that entered vs skipped moments inside the gated pool
are statistically identical, the conclusion is that **his token pick is not reproducible
from window features at high precision.** No metric in reach separates his 0.66% from the
qualifying pool. Replication therefore means trading a *superset* of his universe with
his mechanics (dip entry, ~3% trail, aggressive re-entry, hard concurrency cap) and
letting the concurrency cap do the rationing - not out-filtering him.

**Consequent priority change.** The largest untested lever is not a metric at all: every
backtest so far was **one-shot (no re-entry)**, and today's episode data shows episode 1
is his *worst* (53.4% win / +0.52% median) while episodes 4+ carry the money (+2.0 to
+2.8% median). A one-shot backtest samples only the weakest part of the strategy. Re-entry
already ships in the engine, so re-running the backtest with re-entry ON and the
recalibrated knobs requires **zero engine changes** and should precede any new metric.

## Data caveats / next data steps
- Window = one ~26h weekday slice; one wallet fully analyzed. Fees/tips estimated,
  not measured (amount_lamports is curve-side; pump.fun ~1%/side fee + ~0.001 tip/tx
  are on top).
- Exit inference: a flow-based exit ("sell into the first sizeable market sell after a
  bounce") is observationally near-identical to a 1-1.5% trail; either implementation
  should reproduce the profile.
- EC2 holds ~30 days of wallet-attributed trades + 3 days raw_txs (was 7, tightened
  2026-08-09 — see `docs/plans/database/raw-txs-storage.md`); a full
  `scripts/db-incremental-sync.ps1` run extends this analysis to ~a month at ZERO
  Helius cost (local raw_txs is empty; lake days 07-01..07-08 lack the wallet column).
- Helius spend is critically sensitive (user directive): no RPC fetches for analysis
  without explicit approval. Co6/trunoest cannot be characterized further from local
  data - they never touch the fresh-curve tokens our ingest tracks.

## `64hP` - second scalper wallet, same family, better economics (2026-07-28)

`64hP97Bwr5PubotcTeGgfhkFrGiLVVxT2kVo9M9b4AEz` - the "benchmark dip-reversion" wallet
already flagged in the family-wide scan. Re-derived properly on the same rebuilt window
as the omego re-derivation (2026-07-22 18:47 .. 07-27 16:08, 6.48M trades / 67,806
mints). His slice: 13,326 legs / 6,765 buys / 2,581 mints / 6,742 episodes
(2.3x omego's episode count, 5.8x his mint count).

Same *family* as omego (dip-reversion, 1-buy-1-full-sell, pct-of-vsol sizing, unlimited
re-entry), but every knob is set differently - and his per-episode economics clear the
pump.fun fee where omego's do not.

### Mechanical fingerprint (all impact-corrected)

His own price impact is a constant +3.82% on entry / -3.7% on exit (a direct
consequence of exact-fraction sizing), so raw entry/exit prices are biased; every number
below uses the pre-trade market price.

| Knob | `64hP` | omego (07-27 re-derivation) |
| --- | --- | --- |
| Sizing | **1.859% of vsol**, p25=p50=p75 identical | 1.18% of vsol |
| Size cap | **exactly 1.5 SOL gross** (1.4810 net) | none observed |
| vsol band at entry | **30.6 - 113** (hard floor + ceiling) | 45-110 |
| vsol at *first* buy | med **44.5** (p25 36, p75 56) | med 73.5 |
| Token age at first buy | med **0.8 min** | med 5.3 min |
| Trades in prior 60s | med **135** | med 92 |
| Unique wallets prior 60s | med 68 | med 66 |
| Selectivity | **3.8%** of mints (2,573/67,806) | 0.66% |
| Entry dip vs 30s high | med **-22.7%** (p25 -38.3, p75 -11.4) | med -12.6% |
| Entry vs prior ATH | med **-36.3%** | med -15.2% |
| Exit: retrace off since-entry peak | med **-6.8%** | ~-3% |
| Hold | med **21.3s** (p75 47, p95 268) | med 22.5s |
| Re-entry gap | med **30.5s** | med 34.6s |
| Concurrency | med 2, p90 4, max 10 | med 3, max 7-8 |
| Episodes/mint | med 2 (44% are one-and-done) | ~7 |

Structure is identical to omego's: 6,742 opening buys vs 23 add-on buys, 6,539
full-exit sells vs 22 partials. No scaling in or out, ever.

**Exit is ONE rule, not three.** Bucketing exit-retrace by MFE shows a flat band of
-4.9% .. -7.4% for every bucket with MFE >= 5%. Episodes that never rose exit at a
median -7.16% vs entry - i.e. the same trailing stop with `peak` initialised to the
entry price. There is no take-profit and no separate stop-loss. The -33% p10 tail on
losers is gap risk, not a wider stop.

Re-entries improve monotonically with index (ep1 52.0% win / +5.18% avg -> ep9+
65.0% / +7.73%) - replicates the omego finding. Do not cap `max_episodes` low.

### Economics - and the fee number that decides everything

`amount_lamports` is the **net curve-side** SOL, *excluding* the pump.fun fee
(`shared/ingest/pumpfun/src/decode/trade.rs:20-71` stores `TradeEvent.sol_amount`
verbatim; the trailing `fee` / `fee_basis_points` IDL fields are never decoded). So raw
`sell - buy` sums overstate PnL on both legs.

The fee rate is measurable from our own data. First-buy (dev-buy) amounts cluster
hard on `0.98765432 x round SOL` - and 0.98765432 = `10000/10125` exactly, matching the
IDL's `net_sol = spendable * 10_000 / (10_000 + total_fee_bps)`:

```
3000000000 (3.0)   x5504     2962962962 (3.0 x .98765) x2083
5000000000 (5.0)   x3806     4938271604 (5.0 x .98765) x2045
1000000000 (1.0)   x2171      987654320 (1.0 x .98765) x2718
                              1481481480 (1.5 x .98765) x473   <- his size cap
```

=> **total_fee_bps = 125 (1.25%/leg, 2.53% round trip).** This supersedes the sim
kernel's `FEE_BPS_PER_LEG = 100.0` (`hunter/core/src/strategies/kernel.rs:94`), which is
25 bps/leg too cheap - a 0.5pp round-trip understatement on every backtest run so far.

Net-of-fee accounting over the window:

| Cohort | eps | SOL deployed | SOL returned | net |
| --- | --- | --- | --- | --- |
| closed episodes | 6,515 | 6,632.6 | 6,801.2 | **+168.7** |
| unclosed ("bags") | 227 | 250.9 | 25.7 | **-225.3** |

Closed-episode edge = **+2.54% per SOL cycled, net of the 1.25%/leg fee**, 56.5% win,
median episode +2.39% gross. This is the headline: unlike omego, his mechanics clear
the fee with ~2.5pp to spare. Every day in the window is positive.

### The bags - the dominant open question

227 episodes (3.3%) have no recorded exit. The bag rate is constant across days
(2.92 / 3.50 / 3.59 / 3.85%) including gap-free days, so these are *not* ingest-gap
artifacts, and only 2/227 mints ever traded on AMM so they are not migrations.

But they are also not rugs. Marking them to the market price at a fixed horizon
after entry:

| bail-out horizon | median px vs entry | bag cohort net |
| --- | --- | --- |
| +15s | **+0.2%** | +8.9 SOL |
| +60s | -1.9% | +16.5 SOL |
| +300s | -5.8% | +21.0 SOL |

The tokens did not collapse - their trade stream simply stopped while price was still
near his entry, and he never sold. At the stream end, 86/227 bag mints were still busy
(>=10 trades in the last 60s) vs 12.6% for control mints, so a minority are our feed
dropping the subscription (his sell then happened off-record). The other ~62% go
genuinely cold.

So the wallet's true result is bracketed:
- bags marked to zero: **-56.6 SOL** (net negative)
- bags exited on a dead-flow bailout: **~+180 SOL** over 4.2 days (see below)

Actionable regardless of where the truth sits: the trading mechanics are profitable
net of fees, and 100% of the downside risk is concentrated in "no mandatory exit when a
token goes cold". A price-based hard stop does **not** substitute - overlaying stops at
15/20/25/30/40/50% moves the total only from -56.6 to -43.0 SOL at best, because the
stop shreds the right tail the strategy depends on. **Correction:** an earlier pass
through this data recommended `m_price_lifetime.stall` for the bailout. That metric is
seconds-since-the-last-**new-all-time-high**, not since the last trade, so on a
dip-entry rule it is true by construction and just caps position lifetime - it is the
same defect that pinned every `fs-*` hold to ~15s. Validated instead:
`m_flow_window(30).gross_flow <= 3` replayed on these 227 bags fires on 146 of them,
median 54.8s after entry at -11.5% vs entry, taking the cohort from -225.3 to -19.1 SOL
(the `held >= 90` time cap closes the remaining 81). Seeded as `fs2-00`'s exit in
`hunter/scripts/seed-flow-scalper-64hp-rules.sql`.

### Where the edge actually lives (net-of-fee, per-episode)

| dim | bucket | n | net SOL | net %/ep |
| --- | --- | --- | --- | --- |
| dip vs 30s high | > -8% | 1,167 | **-4.5** | **-0.81** |
| | -8..-15% | 1,009 | +24.2 | +2.27 |
| | -15..-25% | 1,366 | +28.6 | +1.92 |
| | **-25..-40%** | 1,469 | **+77.1** | **+5.37** |
| | < -40% | 1,503 | +43.1 | +3.79 |
| rise off 30s low | **< 3%** | 1,599 | **+57.4** | **+3.96** |
| | 3-10% | 1,029 | +31.1 | +3.08 |
| | 10-25% | 1,316 | +48.5 | +3.71 |
| | 25-60% | 1,440 | +23.3 | +1.66 |
| | > 60% | 1,130 | +8.2 | +0.68 |
| vsol at entry | < 40 | 1,472 | +22.3 | +2.35 |
| | **40-55** | 2,105 | **+78.8** | **+4.23** |
| | 55-75 | 1,864 | +33.2 | +1.47 |
| | > 75 | 1,073 | +34.2 | +2.26 |
| hold | < 90s | 5,690 | +159.2 | +2.4..+4.3 |
| | **> 90s** | 824 | **+9.3** | **-0.28** |

Reading: shallow dips are the only losing bucket he has (-0.81%/ep) - a >=15% dip
gate would have removed 1,167 losing episodes; buying nearer the 30s low beats buying
after a bounce (contradicting the naive read of the median +16.6% rise, which is where
the *mass* is, not the *edge*); vsol 40-55 is the sweet spot; and holds beyond 90s stop
paying despite carrying half the gross PnL.

### Proposed knob deltas for the `fs-%` seed rules (retired - superseded by `fs2-*`)

These deltas against the old omego-calibrated `fs-*` rules were carried out in full in
the `fs2-*` ladder below (`hunter/scripts/seed-flow-scalper-64hp-rules.sql`). Kept here
as the original reasoning trail:

Against the then-current values (trail30 12, gross60 11, liq band 55-100, retrace 5,
stop_loss 25):

| knob | now | proposed | why |
| --- | --- | --- | --- |
| `m_price_window(30).trail` | >= 12 | **>= 18** | his -8% bucket is his only losing cohort; -25..-40 is his best |
| liquidity band | 55-100 | **36-70** | his first-buy vsol p25-p75; 40-55 is the sweet spot, 55-75 the worst |
| `m_flow_window(60).gross` | >= 11 | **>= 45** | his p25 gross60 = 45.6 SOL, med 85 - far hotter than omego |
| `m_position.retrace` exit | >= 5 | **>= 7** | his measured trail is 6.8%, not 3% |
| buy size | 1.0 SOL flat | **1.86% of vsol, cap 1.5 SOL** | exact, and it is what makes impact constant |
| max hold | (none) | **90s hard time-exit** | >90s is net negative for him |
| stall exit | present | **keep, and treat as mandatory** | the single thing standing between his mechanics and his -225 SOL bag charge |
| `stop_loss` | 25 | **drop or widen a lot** | stop overlays at every level 15-50 make his book *worse* |
| `max_episodes` | 12 | **keep high** | ep9+ is his best cohort (65% win / +7.73%) |

Untested here: whether entering at token age <1 min (his profile) survives *our*
latency. He is at median 0.8 min age / 135 trades per prior 60s; that is a much faster
adoption than the omego-calibrated rules assume.

### Caveats

- Same window caveats as the omego re-derivation: 07-23 is largely missing and there is
  an 11.8h ingest hole on 07-26 15:44 -> 07-27 03:34.
- Bag treatment is the dominant uncertainty in the headline PnL (see above); the
  mechanical findings (sizing, trail, dip, selection) do not depend on it.
- Priority fees / Jito tips are excluded - 13,326 legs, so at 0.001 SOL/leg that is a
  further ~13 SOL.
- Analysis tables (`s64_tr`, `s64_bal`, `s64_ep`, `s64_mkt`, `s64_first`, `s64_sel`)
  were left in the hunter PG for follow-up; drop them when done.

### `fs2-*` ladder - the knob deltas above, carried out

Seeded by [`../../../scripts/seed-flow-scalper-64hp-rules.sql`](../../../scripts/seed-flow-scalper-64hp-rules.sql)
(paper, `is_active=false`). ONE broad fingerprint (`fs2-ALL broad`; creation shape
carries no signal once hotness is known - the fingerprint-axis section above) x 12
rules, each moving exactly ONE knob off `fs2-00 base`, so every comparison is clean.

```
entry (AND)   m_snapshot.time        >= 30      his p25 first-buy age is 30 s
              m_snapshot.liquidity   36 .. 70   his first-buy vsol p25-p75
              m_price_window(30).trail >= 18    dip off the 30 s high; his >-8% bucket is
                                                his ONLY losing cohort (-0.81%/ep)
              m_flow_window(60).gross_flow >= 45  his p25 gross60 = 45.6 SOL
              m_flow_window(2).net_flow   >= 0    sell-exhaustion
exit (OR)     m_position.retrace     >= 7        his measured trail is 6.8%
              m_position.arm_above_pct = 2       <- he does NOT have this; see below
              m_position.held        >= 90       his >90 s cohort is net NEGATIVE
              m_flow_window(30).gross_flow <= 3  cold-token bailout; he does NOT have this
              stop_loss              = 12        floor while the trail is unarmed
buy           0.30 SOL fixed                     pct-of-vsol sizing is unimplemented
```

Ladder (each row = one knob off base): `01` dip 12 / `02` dip 25 / `03` trail 4 /
`04` trail 11 / `05` unarmed / `06` no time cap / `07` no dead-flow exit / `08` liq
30-110 / `09` gross60 20 / `10` size 0.10 SOL / `11` size 0.80 SOL. Param shapes were
verified against `RuleParams::parse` + `CompiledRule::compile` (entry reqs = 5;
`arm_above_pct` attaches to `retrace` only, never to `held`; the dead-flow req is
token-scoped at window 30; `stop_loss` desugars).

Two deliberate departures from his measured behaviour, both measured on his own
episodes:

1. **Armed trail.** His trail is unarmed (losers exit at a median -7.16% = the trail
   firing with `peak` still at entry). That mostly works because his median *pre-peak*
   drawdown is only -1.15%. But **23.6% of his big winners dipped >7% before peaking**,
   so an unarmed 7% trail cuts them. `arm_above_pct: 2` keeps them; `stop_loss: 12` is
   the floor until it arms. `fs2-05` reverts to his literal unarmed exit for the A/B.
   See [armed-trailing-stop.md](../plans/strategies/armed-trailing-stop.md).
2. **Dead-flow bailout.** His only defect, quantified above: 227 cold-token episodes
   costing -225.3 SOL. `fs2-07` removes it for the A/B. **Do NOT express this as
   `m_price_lifetime.stall`** - it is seconds since the last *new all-time high*, so on
   a dip-entry rule it is true by construction and just caps position lifetime (the
   defect that pinned every `fs-*` hold to ~15 s -
   [flow-scalper-findings.md](../plans/strategies/flow-scalper-findings.md) finding #2).

**Buy size is the least-grounded knob.** 64hP sizes at 1.859% of vsol capped at 1.5 SOL
gross (~0.8 SOL in this band), but [execution-costs.md](../plans/strategies/execution-costs.md)'s
impact-aware model puts the optimal *fixed* size at `sqrt(fixed_cost_per_leg * vsol)` =
~0.21-0.27 SOL on this band, not 0.30. `fs2-10`/`fs2-11` bracket it (0.10 / 0.80);
consider a rule nearer the computed optimum before sweeping.

Calibration caveats specific to this ladder, beyond the window caveats above: it rests
on **one 5-day window and one wallet**, and entering at token age <1 min (his profile)
is untested against *our* arm-to-fill latency. The `fs3-*` supersession and this ladder's
demotion to a broad-universe control are in
[`wallet-analysis.md`](../plans/strategies/wallet-analysis.md). Both engine gaps it wanted
are now closed: `unique_wallets` is built and measured — it anti-selects, see
[`metrics-reference.md`](../plans/strategies/metrics-reference.md) — and percent-of-vsol
sizing ships as `buy_pct_of_vsol`
([`execution-costs.md`](../plans/strategies/execution-costs.md)).

## Dev-buy size - the one creation axis that predicts OUTCOME (2026-07-28)

**This overturns a standing assumption, but only in one direction.** The
fingerprint-axis section above tested creation shape against token *selection* - which
token a scalper picks - and found chi2/df ~ 1.0 on every axis: no signal, hence the
"use a maximally-broad fingerprint" design rule. That result stands. What was never
tested is creation shape against *outcome* - how an episode on a token he already
picked ends. For outcome, one axis does carry signal.

Method: `s64_fp` (built from `s64_ep` + `s64_sel` + `tokens` + a creation-slot rollup),
one row per episode, PnL priced net of the measured 125 bps/leg fee as
`sol_out*0.9875 - sol_in*1.0125`. Baseline over his 6,515 **closed** episodes:
**+2.66 %/ep, 49.6% win**, sd 30.1 (t=7.1 - his edge is real; the 56.5% win quoted
earlier in this doc is gross, before the fee).

Axis-level omnibus on win/loss, groups of >= 20 pooled, null expectation chi2/df ~ 1.0:

| axis | groups | chi2/df |
| --- | --- | --- |
| `buy_ix_type` | 5 | **2.59** |
| `init_buy` (1 SOL buckets) | 19 | **2.03** |
| first-slot buy (1 SOL) | 35 | 1.85 |
| `spendable_in` (1 SOL) | 14 | 1.78 |
| `ix_labels` | 29 | 1.38 |
| `cu_limit` | 12 | 0.82 |
| `cu_price` | 29 | 0.82 |
| `is_cashback_enabled` | 2 | 0.44 |

`init_buy` is the usable one (`buy_ix_type` is a 5-value proxy for the same thing and
`cu_*` are flat). Note `spendable_lamports_in` IS populated in this window - the
earlier "absent from every creation row" note was about the older corpus.

### The threshold ladder

`initial_buy_lamports` is the **net** curve amount, so the dev-buy clusters sit at
`gross x 0.98765` (12.0 -> 11.8519, 15.0 -> 14.8148, 25.0 -> 25.0). Fit window
07-22..07-25, holdout 07-26..07-27 (the holdout was never used to pick the cut):

| `init_buy >=` | n fit | win fit | net fit | n hold | win hold | net hold | mints |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 0 (all) | 3371 | 50.3% | +2.81 | 2966 | 48.6% | +2.52 | 2434 |
| 5.8 | 472 | 53.8% | +4.27 | 418 | 55.0% | +3.40 | 291 |
| 8.8 | 323 | 54.5% | +5.79 | 299 | 57.2% | +4.01 | 196 |
| **12.0** | 143 | **57.3%** | **+7.73** | 179 | **57.0%** | **+4.45** | 101 |
| 13.0 | 136 | 58.1% | +8.26 | 151 | 61.6% | +6.15 | 87 |
| 14.7 | 131 | 59.5% | +9.38 | 147 | 61.9% | +6.46 | 81 |

Monotone in **both** windows - which is the property that separates this from the
bucket-derived gates that died (`range`, rise-at-low, `rise <= 1`). Those were all
best-of-N single buckets; this is a threshold family improving everywhere.

At the chosen cut of 12.8 SOL (the bucket edge used by the fingerprint, below):

| cohort | eps | mints | win | net %/ep |
| --- | --- | --- | --- | --- |
| dev buy < 12.8 | 6,218 | 2,400 | 49.1% | +2.44 |
| **dev buy 12.8-25.6** | **284** | **80** | **59.2%** | **+7.11** |
| dev buy >= 25.6 | 13 | 9 | 76.9% | +8.51 |

### Why it is believable

1. **Mint-level block permutation, 2,000 shuffles** (labels permuted across mints so
   the within-mint episode clustering is preserved): null win 49.52% (sd 3.19) vs
   observed 57.14% => **p = 0.006**; null net +2.57 (sd 1.82) vs +5.91 => p = 0.037.
2. **Not a liquidity proxy.** The lift survives conditioning on the entry-vsol band -
   within 40-55 it is 61.9% vs 50.7%, within 55-75 it is 62.9% vs 52.1%. (It does
   invert above vsol 75, n=81 - hence the 40-75 band in the rule.)
3. **Every day of the window**, on both metrics: win 62.5 / 56.7 / 59.7 / 62.6 for the
   big cohort vs 49.6 / 50.2 / 49.0 / 46.8 for the small one.
4. **Not concentrated**: 89 mints, top 3 carry 34% of the net, 62% of mints net-positive.
5. It also halves his one real defect - the **bag rate is 2.42%** for big dev buys vs
   3.55% for small ones, consistent with "a funded launch does not go cold".

### What does NOT support it

- **It does not replicate on omego, because it cannot.** Rebuilt from `trades` +
  `wallet_dict` over the same window: omego has **zero** episodes above a 12.8 SOL dev
  buy (10 above 8.0), and inside the range he does trade the ladder is flat-to-down
  (47.7% at >= 0, 47.9% at >= 3, 45.8% at >= 5). So this is a **one-wallet finding in a
  region the other wallet never enters** - a non-test, not a confirmation.
- ~40 groups/thresholds were examined, so the p=0.006 is not Bonferroni-safe on its
  own. The holdout replication and the day-by-day consistency are what carry it.
- One wallet, 4.2 usable days.

### Engine validation (the independent test)

Because the effect is conditional on *his* entry timing, the only real test is our own
engine on our own entries. `scripts/flow-scalper-ladder.ps1 -Plan fp13 / fp13b` against
fingerprint `fs3-dev big [12.8-25.6)`, 07-22..07-28, `pumpfun_impact`, 0.30 SOL, conc 4.
Geometry = 64hP's, plus `arm_above_pct 2`, `liquidity 40-75`, dip 25:

| run | fill | n | mints | win | mean %/ep | PF | t(mean) | t(win vs 50%) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| N1 base | first | 74 | 39 | 59.5% | +5.42 | 1.67 | 1.32 | 1.66 |
| M2 | signal | 74 | 39 | **63.5%** | +8.06 | 2.13 | 1.86 | **2.41** |
| **M1** | **worst** | 72 | 39 | 51.4% | **+3.48** | 1.35 | 0.79 | 0.24 |
| M5 one-shot | first | 39 | 39 | **66.7%** | +11.04 | 2.55 | 1.62 | **2.21** |
| M7 unarmed | first | 116 | 39 | 26.7% | -0.98 | 0.88 | -0.40 | -5.67 |

Reads:

- **M1 is the headline.** `worst` fill is what live paper books, and every prior
  configuration in this project died there (`flow-scalper-findings.md`: "-2.6 to -4.7
  %/ep vs `first`... nothing measured survives it"). This one stays **positive**. It is
  not *significantly* positive (t=0.79) - but positive-signed at the adversarial bound
  is a first.
- **The win-rate lift is the statistically solid part** (t=2.41 at `signal`, 2.21
  one-shot); the PnL is not established at any fill (t=0.79..1.86). Per-episode sd is
  35-43, so at n=74 the SE on the mean is ~4.3 pp.
- **M7 confirms the armed trail is not optional for us.** Running 64hP's literal
  unarmed exit collapses to 26.7% win with a 5 s median hold - the
  `PositionCtx::at_fill` trap (`retrace >= 7` is a hard -7% stop from entry until the
  price rises). He gets away with it because he enters earlier in the dip than our
  gates do; we do not.
- The `lock`-ladder invariant `arm > retrace` **fails here** (`fp13` rows N2-N5, all at
  a median -14.6%): arming at +8 disables the trail below +8 and leaves `stop_loss 10`
  as the only exit, so every position runs to the stop. 64hP's wide, barely-armed trail
  (arm 2 / retrace 7) is correct.
- Dip is single-peaked at 25: 18 -> 51.0%, 20 -> 56.2%, **25 -> 59.5%**, 30 -> 58.0%.
- Concurrency 12 is byte-identical to 4 - at ~110 armed tokens/day the cap never binds.

### The band control - and why WIN RATE IS THE WRONG OBJECTIVE

The N1 geometry, unchanged, run against lower dev-buy bands. Token count is held
roughly constant so the comparison is the fingerprint and nothing else:

| band | tokens | n | mints | win | mean %/ep | avg win | avg loss | W/L |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| mid [6.4, 9.6) | 1,020 | 68 | 33 | **64.7%** | **-1.01** | +13.23 | -27.11 | **0.49** |
| adj [9.6, 12.8) | 1,269 | 68 | 38 | 52.9% | +1.02 | +18.85 | -19.04 | 0.99 |
| **big [12.8, 25.6)** | **595** | **74** | **39** | 59.5% | **+5.42** | +22.70 | -19.93 | **1.14** |

**The middle band has the HIGHEST win rate in the whole study and the WORST
expectancy.** It wins 64.7% of the time and still loses money, because its payoff ratio
is 0.49 - small wins, double-size losses. Ranking these three bands by win rate picks
the only losing one; ranking by expectancy or by W/L picks the right one.

So the dev-buy fingerprint's value is **not** that it raises the hit rate. It is that
it is the only band where the hit rate and the payoff ratio are good **together** - the
winners get bigger (+22.70 vs +13.23) without the losers getting bigger. A funded
launch produces the deeper, longer reversion the strategy monetises; it does not
produce more of them.

Corollary for any future ladder here: **do not rank on `win_rate`.** Use
`expectancy_sol` / `profit_factor`, and read `win_rate` only alongside the average
win/loss pair. The control also isolates the fingerprint from the gate changes that
came with it - the dip-25 / liq-40-75 gates alone (the `adj` band) deliver +1.02 %/ep,
so ~80% of N1's +5.42 is attributable to dev-buy size.

### Practical limits found while running this

- **Do not simulate the exact complement** `fs3-dev small [0-12.8)` (~85k tokens): a
  6-day fold dies with `lake trade fetch failed: Out of Memory Error: Allocation
  failure` after the full 5400 s ladder timeout, on the 16 GB workstation. The
  size-matched bands above are both runnable (50-70 s) and better controlled.
- The fine structure below the cut is **not monotone** - the "12 SOL gross" cluster
  (11.8519 net, 35 mints) is his mediocre one, which is part of why the cut lands at
  12.8. Treat 12.8 as "the edge of a good region", not as a precisely estimated
  threshold.

### Tractability - the other reason to use it

The broad `fs2-ALL` fingerprint arms ~18,000 tokens/day, which is why its matched set
is too large to simulate or trade. `init_buy` in [12.8, 25.6) arms **933 of 107,954
tokens over the window (~110/day)**, and a 6-day simulate folds in **60 s** instead of
~20 min. It is an **instant** axis (`has_instant_criterion`), so it matches
synchronously on `TokenCreated` with no `PendingFirstSlot` deferral.

Seed: [`../../../scripts/seed-flow-scalper-dev13-rules.sql`](../../../scripts/seed-flow-scalper-dev13-rules.sql)
(`fs3-*`, paper, `is_active=false`).

## `63ot` - third scalper wallet: fixed-bracket dip sniper on a tight budget (2026-07-29)

`63otb3qfCMz5bghv2vyEEwMoZnhnqyv7mj6rVco1hwnH`, supplied by the user as a
low-capital template ("tight budget, fewer trades" vs omego/64hP). Analyzed on the
local PG window 07-22 18:54 -> 07-28 14:03 UTC (~5.8d). Scratch tables left in PG
for follow-up: `s63_tr`, `s63_ep`, `s63_ctx`, `s63_exit`, `s63_pre`, `s63_mkt`
(824K rows), `s63_first` - drop when done.

### Headline economics (net of 125 bps/leg fee)

- 2,342 legs, 1,125 buys / 1,217 sells, 491 mints, 1,098 episodes (1,088 closed).
- **+13.5 SOL closed / -3.1 SOL in 10 open bags => ~+10.4 SOL net over 5.8 days**
  on ~583 SOL cycled = **+2.3% of turnover**; win rate **65.1%**, avg **+2.24%/ep**,
  median **+11.0%/ep**. Positive 5 of 7 days (worst day -0.7% of cycled).
- Working capital is tiny: **max 3 concurrent episodes, p99 = 2, usually 1** at
  ~0.5 SOL each => the whole book runs on **~1-2 SOL**, turning ~100 SOL/day.
- If the true fee were 100 bps/leg he'd be ~+3 SOL richer; the verdict does not
  flip either way.

### Mechanics

- **Universe/selectivity**: 491 of 85,602 mints in-window = **0.57%** (omego-level).
  Runs 24/7 (every UTC hour populated) - automated.
- **Entry**: deep-dip snipe on very hot, deep-curve tokens. At entry (n=1,088,
  no lookahead): token age med **1.9 min** (p25 0.6 / p75 4.2), vsol med **69.8**
  (p25 58 / p75 82), price **-20.8% vs the 30s high** (p25 -36 / p75 -10.5),
  **-27.5% off prior ATH**, market heat med **224 trades / 119 SOL gross in the
  prior 60s** (hotter than omego's pool). NOT a bottom-tick buyer: entry sits
  mid-range of a violently swinging 30s window (med +29% above the 30s low).
  **NOT bounce-confirmed**: 56% of entries land immediately after a market SELL
  and prior-2s net flow is negative at median (-2.4 SOL) - he buys INTO the knife.
  A `m_flow_window(2).net >= 0` condition (the fs2 shape) would fight this entry.
- **Sizing**: a small fixed ladder, NOT %-of-vsol - mode ~**0.50 SOL** (~900 of
  1,125 buys in 0.48-0.54), plus exact repeated values (0.7333, 0.2933, 0.1467).
  Own impact ~0.7%/leg at 0.5 SOL into vsol 70 - same as ours would be.
- **Episode shape**: **94% are 1 buy -> 1 full sell** (same as omego/64hP; partial
  exits ~6%, scale-ins ~2%). 474/491 mints end at exactly 0 tokens.
  Re-entries: med 1 ep/mint, p75 3, max 17; win rate flat-to-up with index
  (ep1 63.7% -> ep4+ 69.3%) - no need to cap episodes.
- **Exit = a fixed TP/SL bracket, NOT a trailing stop** (the big difference vs
  omego/64hP):
  - Winners (708): gross move med **+16.9%** (p25 14.2 / p75 21.7), constant
    across dip-depth buckets (16.3-18.0) => **fixed TP ~ +17% from entry**. He
    exits within ~1% of the since-entry peak (at-touch), and the price keeps
    running afterwards: med **+17.6% further max** in the next 60s. He
    deliberately leaves the right tail.
  - Losers (380): gross move med **-27.5%** (p25 -30.8), exits within ~1% of the
    since-entry trough => **hard SL ~ -28..-30%**, with slippage scatter to -45
    in fast dumps. Post-exit drift med -0.5%: the stop is protective, not
    value-destroying.
  - Holds: med **10.6s** overall - winners 7.6s, losers 18.6s, p90 54s.
- **Bags**: only 10 episodes (0.9%) never closed, total -3.1 SOL. The 64hP bag
  problem (3.3%, -225 SOL) mostly vanishes at this depth of curve - near-migration
  hot tokens rarely go silent mid-episode.

### Where the edge concentrates (net-of-fee, per episode)

| dip vs 30s high at entry | n | win | avg net/ep |
| --- | --- | --- | --- |
| > -10% | 258 | 59.3% | +0.74% |
| -10..-20% | 262 | 64.1% | +1.73% |
| -20..-35% | 278 | 65.8% | +2.21% |
| <= -35% | 290 | 70.3% | **+4.08%** |

Same shape as 64hP: shallow dips are the weakest bucket; the money is in the
deepest dips. TP size does NOT vary with dip depth - only the win rate does.

### Comparison of the three cracked wallets

| | omego | 64hP | 63ot |
| --- | --- | --- | --- |
| closed eps (window) | 2,974 (5d) | 6,515 (4.2d) | 1,088 (5.8d) |
| win rate | 59.1% | 56.5% | **65.1%** |
| net edge | ~0 (refuted) | +2.54%/SOL cycled | +2.3%/turnover |
| median ep | ~0 | +2.39% | **+11.0%** |
| sizing | 1.18% vsol | 1.86% vsol cap 1.5 | **fixed ~0.5 SOL** |
| entry age (med) | 5.3 min | 0.8 min | 1.9 min |
| vsol (med) | 73.5 | 44.5 | 69.8 |
| dip vs 30s high | -12.6% | -22.7% | -20.8% |
| exit | ~3% trail | -6.8% trail | **TP +17% / SL -28%** |
| hold (med) | 22.5s | - | 10.6s |
| unclosed bags | - | 3.3% / -225 SOL | **0.9% / -3.1 SOL** |
| concurrency | med 3, max 8 | - | **usually 1, max 3** |
| selectivity | 0.66% | 3.8% | 0.57% |

### Engine fit - this is the EASIEST wallet to express so far

Everything needed already exists; no new metric, no trailing subtleties:

- Entry: `m_price_window(30).trail` (dip vs 30s high), `liquidity` band,
  `m_flow_window(60).gross`, `time` (age) - all shipped. Do NOT add a
  `flow(2).net >= 0` bounce gate (see above).
- Exit: the plain `take_profit` / `stop_loss` sugar (desugars to
  `m_position.pnl`) - no `arm_above_pct`, no trail, no stall dependency. The
  0.9% bag rate means even the dead-flow bailout is optional here (still cheap
  insurance: `m_flow_window(30).gross <= 3`).
- Sizing: fixed 0.5 SOL is next to the measured cost-optimal 0.27-0.5 for
  vsol ~70 (see execution-costs.md); tip drag at 0.001/leg on 0.5 SOL is
  ~0.4%/round-trip against his +2.3% margin - thin but positive.
- Latency caveat: his median winner resolves in 7.6s and both exits fill
  at-touch. Live TP/SL evaluation is feed-driven (per-trade), so the engine
  reacts at feed latency; expect a haircut vs his at-touch fills - validate via
  simulate with `pumpfun_impact` + `worst` fill before believing any number.
- Gate-recall caveat: a first-guess gate (liq 55-85, trail30 >= 15,
  gross60 >= 70, age >= 0.5 min) recalls only 27% of his entries jointly
  (60-77% each) - the bands interact and need a sweep to place, and universe
  precision is unmeasured. In-gate episodes do outperform (69.7% win / +2.83%
  vs 63.4% / +2.03% outside).

SEEDED 2026-07-29 as `fs4-00 63ot base` (fingerprint originally `fs4-ALL broad`,
`buy 0.5 SOL; TP 17; SL 28; liq 55-85; m_price_window(30).trail >= 15;
m_flow_window(60).gross >= 70; time >= 30s; reentry 5s/20; max_concurrent 2`).
`trade_mode='paper', is_active=false` (safe-by-default; arm via `UPDATE
strategy_rules SET is_active = true WHERE rule_name = 'fs4-00 63ot base'`). The
single-knob ladder variants (trail 20/25/35, TP 12/22, SL 20/35, gross60
40/120, liq band shifts) are NOT seeded - only the base row exists. The
fingerprint was narrowed the same day; see the next section.

## `63ot` - fingerprint narrowing (2026-07-29)

`fs4-00`'s first cut used `fs4-ALL broad` (matches every token, same
"creation shape carries no signal for SELECTION" convention as fs2/omego).
Live paper data on that broad fingerprint showed the same trap the dev-buy
finding surfaced for 64hP: some bucket combos post a 50-62% win rate while
losing money overall. Re-ran the dev-buy-size method (see "Dev-buy size"
above) fresh against 63ot's OWN 1,090 closed episodes (2026-07-22..28, every
episode's mint has a matching `tokens` row):

| `initial_buy_lamports` bucket | eps | win% | avg%/ep | total SOL |
| --- | --- | --- | --- | --- |
| [0, 1.6) | 131 | 67.2% | +5.57% | +4.37 |
| [1.6, 3.2) | 319 | 66.1% | +2.59% | +4.19 |
| [3.2, 6.4) | 413 | 66.3% | +3.38% | +8.41 |
| [6.4, 12.8) | 81 | 53.1% | **-5.76%** | **-2.64** |
| [12.8, 25.6) | 135 | 63.0% | **-1.18%** | **-0.53** |
| [25.6, +) | 4 | 75.0% | +5.12% | +0.10 (n too small) |

`[6.4, 25.6)` is exactly the "high win rate, negative PnL" trap: 53-63% win
but net losers, because losses there run fatter than wins. Below 6.4 SOL is
unambiguously good and NOT a single lucky day - `[0, 6.4)` is net-positive on
7 of 7 days in the window (weakest day still +0.36 SOL); `[6.4, 25.6)` is
net-negative on 5 of 7. Combined `[0, 6.4)` = 863 eps, 66.5% win, +16.97 SOL
total, vs +14.58 SOL for the unfiltered set - narrowing to 79% of episodes by
count RAISES total PnL, it isn't a volume-for-edge trade.

Finer cut tested but NOT adopted: excluding the `Pump.Fun: BuyExactQuoteInV2`
creation-ix variant within `[0,6.4)` (64 eps wallet-wide, 59.4% win but
-2.64%/ep, -1.06 SOL) bumps to 804 eps / 66.9% win / +17.56 SOL - only +0.59
SOL for -59 eps, and `ix_labels` isn't a clean bucket axis like init_buy is.
Revisit once the 30d EC2 sync gives more sample.

Tested and REJECTED as screens (flat across buckets, no separation): `cu_limit`
(only a 7-episode bucket looked good - noise), `is_cashback_enabled`
(directionally better at true, but BOTH sides net-positive, not a real
screen), creator repeat-launch count (61-66% win in every bucket - unlike
64hP's fs3-00 dev13 creator screen, 63ot's edge has nothing to do with who
deployed the token; this wallet trades market structure, not launch crews).

Applied: `hunter/scripts/narrow-flow-scalper-63ot-fingerprint.sql` added
fingerprint `fs4-dev small [0-6.4)` (`init_buy_lamports=0,
bucket_size_amount=6.4`) and repointed `fs4-00 63ot base` at it (metric
gates/TP/SL/sizing unchanged). Reference-only fingerprints `fs4-dev bad
[6.4-12.8)` and `fs4-dev bad [12.8-25.6)` were also seeded (not wired to any
rule) so a future sweep/simulate pass can re-confirm the cut. `fs4-ALL broad`
was left in place (untouched, not deleted) because a hand-made rule, `fs3-00
dev13 base -- copy`, also points at it and fingerprints have no
`ON DELETE CASCADE` from `strategy_rules`.

## `63ot` - re-narrowing attempt against every fingerprint axis (2026-07-29, later same day)

User report: `fs4-00 63ot base` (fingerprint `fs4-dev small [0-6.4)`) still
"not profitable, even win rate is high" once actually exercised. Re-derived
63ot's 1,090 closed episodes fresh (the `s63b_*` scratch tables from the
morning session were already dropped) into `s63c_*` and cross-checked against
the earlier table: 1,090 closed eps, 65.1% win, +14.58 SOL, `[0,6.4)` =
863 eps/66.5%/+16.97 SOL, `[6.4,25.6)` = 216 eps/net **-3.17** SOL - all
reproduce the morning numbers within noise. **The `[0,6.4)` cut itself is not
the bug** - it is a real, reproducible edge in the wallet's own trade history.

Pushed the joint search across **every axis the `fingerprints` table actually
supports** (`cu_limit`, `cu_price` - exact match only; `ix_labels` - exact
ordered sequence only; `init_buy`/`max_cost`/`spendable_in`/`first_slot_buy`/
`first_slot_sell` - bucket-matched via `bucket_size_amount`), inside `[0,6.4)`
and inside the excluded `[6.4,25.6)` zone:

| axis | inside `[0,6.4)` | inside `[6.4,25.6)` |
| --- | --- | --- |
| `ix_type` (creation-buy variant) | `BuyExactQuoteInV2` = 60 eps, 60.0% win, **-0.59 SOL** (all 3 others net-positive, `BuyExactSolIn` best at 73.8%/+5.52%/ep) | every variant net-negative (`Buy` -1.40, `BuyExactSolIn` -1.08, `BuyV2` -0.23, `BuyExactQuoteInV2` -0.47) - no rescue pocket |
| `cu_limit` (exact axis) | noisy per-value; modal `300000` (459 eps) is actually a below-average sub-bucket (+2.07%/ep) vs `NULL`/`220000`/`350000` (+4-7%/ep); high tail `>=400000` (46 eps) net **-0.34 SOL** but small-n | - |
| `cu_price` (exact axis) | noisy; modal bucket + `NULL` both fine, no bad exact value with real n | - |
| `is_cashback_enabled` | not a fingerprint axis at all (`fingerprints` has no such column; `observed_axes` hardcodes `false` into the matcher) - the morning session's "no signal" note was about the wallet's own column, irrelevant to fingerprint design | - |
| creator repeat-launch idx | flat 62-68% win, all buckets net-positive | flat 56-60%, all buckets net-negative |
| `max_cost_lamports`/`spendable_lamports_in` | structurally tied to ix variant (`max_cost` only set for `Buy`/`BuyV2`, `spendable_in` only for the `*SolIn`/`*QuoteInV2` pair) - gating on either would **drop `BuyExactSolIn`, the single best-performing variant**, the wrong direction | - |

**Verdict: no further narrowing is expressible as a fingerprint.** The one
real, day-stable improvement found (drop `BuyExactQuoteInV2`: 803 eps, 67.1%
win, +17.56 SOL, positive 7/7 days) is an **instruction-type** screen, and the
`fingerprints` schema has no "ix type" axis independent of the full ordered
`ix_labels` sequence - and that sequence fragments into 10+ distinct exact
variants even within one ix type (extra `Compute Budget`/`Pump.Fun:
ExtendAccount`/duplicate `System Program: Transfer` legs depending on
priority-fee and ATA state), so "match every good sequence, reject only the
bad ix" cannot be enumerated as one fingerprint row without silently missing
future variants. Expressing it would need a new engine axis (e.g. an
`ix_type` or first-label match), which is a code change, not a SQL narrowing -
out of scope here. `cu_limit`/`cu_price` are **exact-value** axes in this
schema (not ranges), so "cu_limit < 400000" also isn't expressible even though
the high tail looks weak (and at n=46 it's the same noise level the morning
session already rejected for the low end). **No changes made** to
`fs4-dev small [0-6.4)` or `fs4-00 63ot base` - the fingerprint is already at
the ceiling of what this table can express, confirmed twice now from two
independent re-derivations of the wallet's raw trades.

**Why the live "not profitable" report is very likely NOT a fingerprint
problem at all:** `strategy_positions` has **zero rows** for any `fs4-%` rule
(`fs4-00 63ot base` and `fs4-00 63ot base copy` are both `is_active=false`,
never fired) - whatever showed "high win rate, not profitable" was a
lab-side preview/simulate of the RULE, not a replay of 63ot's own trades. The
fingerprint only gates WHICH TOKENS are eligible; the rule's own entry gates
(`time>=30s`, `liquidity 55-85`, `trail30>=15`, `gross60>=70`) decide WHEN
within them, and those gates were already flagged as recalling only ~27% of
63ot's real entries jointly (60-77% each alone). That means the large
majority of what the RULE actually fires on, even restricted to the
`[0,6.4)` fingerprint, is tokens/moments 63ot never traded - an unvalidated
population whose profitability this trade-history analysis (or the morning
one) never characterized, because it only ever looked at 63ot's *actual*
episodes. Narrowing the token-creation-shape fingerprint further cannot fix a
gap caused by the rule's own timing/liquidity gates. **Next step, if pursued:
simulate `fs4-00 63ot base` itself (its real entry gates + TP/SL) over the
sealed lake, bucketed by `init_buy_lamports`/`ix_type`, to see where the
RULE's own simulated episodes - not 63ot's raw trades - are profitable; that
is the only population the "not profitable" report is actually describing.**

Scratch tables left in PG (drop when done): `s63c_tr`, `s63c_run`,
`s63c_ep_flags`, `s63c_ep_ids`, `s63c_ep`, `s63c_full`, `s63c_creator_rank`.

## `63ot` - simulating the RULE itself finds the real cause (2026-07-29, later still)

Per the previous section's recommendation, simulated `fs4-00`'s actual params
(entry gates + TP17/SL28 + reentry cooldown 5s/max 20 episodes) via `POST
/api/strategies/simulate` against the live `hunter-lab` bin (:8140) - a real
draft run, `pumpfun_impact` cost, `worst` fill (what live paper books),
`buy_amount_sol=0.5`, `max_concurrent_tokens=2` (the seeded rule's real cap),
2026-07-22..07-28 (the same window as the trade-history analysis), scoped to
the `fs4-dev small [0-6.4)` fingerprint. Run took ~55 minutes wall-clock (full
92,868-token corpus load + fold; this is normal for a re-entry rule per the
`flow-scalper-ladder.ps1` header note, not a hang).

**Result: n_matched=92,868, but only 50 tokens actually entered** (116
episodes fired, 114 closed, 2 open). **win_rate=58.8%, total_pnl_sol=-8.00,
profit_factor=0.34, worst episode -101.7%.** This is the exact "high win
rate, not profitable" pattern reported - reproduced by simulation, not
guesswork.

**Two causes, teased apart by filtering the 114 closed episodes on `pnl_pct`:**

| slice | n | win% | median pnl% | total SOL |
| --- | --- | --- | --- | --- |
| `pnl_pct < -50` (catastrophic tail) | 13 | 0% | -93.3% | **-6.15** |
| `pnl_pct >= -50` (everything else) | 101 | **66.3%** | **+8.6%** | -1.85 |

**The core edge is fine** - the 101 "normal" episodes post a 66.3% win rate
and +8.6% median, matching 63ot's own 65-67%/+11% almost exactly, and are
only marginally net-negative (-1.85 SOL, profit_factor 0.69, close to
breakeven after `pumpfun_impact` costs). **The entire net loss is a tail-risk
containment failure**: 13 episodes (11% of closed episodes) lose a median
-93.3% each, totaling -6.15 SOL - 77% of the total loss from ~11% of trades.

**Root cause, confirmed against raw `trades`:** pulled the worst episode
(`3RUkLwiMRwFfubsUKsUDsnQxVuEjNUpoqpquLWtRhHxa`, entered 00:04:56, exited
00:05:09 via `StopLoss`, 12s hold, -101.7%). The raw curve trades show the
price drifting normally (~2.1-2.9e-7 SOL/raw-token) for 12 seconds, then ONE
single sell of **48.5 SOL** at 00:05:09.09 crashes `reserve_lamports` from
~79 SOL to ~30.5 SOL in one print (price -85% instantly), and the price never
meaningfully recovers in the following minutes. This is a real on-chain event
(a large holder dumping their entire position, aka a rug) - not a data or
pricing bug. Checked the next 11 worst episodes: **12 of the 13 tail
episodes land at a near-identical terminal price band regardless of entry
price or hold time (2s to 170s)** - consistent with pump.fun's fixed initial
virtual reserves (30 SOL / ~1.073B tokens): a full dump-back-to-near-origin
crashes any curve toward roughly the same terminal state, so unrelated
tokens hit by this pattern land at similar-looking prices. **The `stop_loss:
28` does not contain this**: when the entire crash from 0% to -85%+ happens
inside one fast multi-trade burst, the `worst`-case fill model (which prices
the exit at the lowest print in the post-signal reaction window, per
`docs/arch/sweep.md`) books the bottom of the WHOLE cascade, not the price at
the moment -28% was first crossed - so the position rides the full dump down,
then costs stack on top past -100%. This is the same latency/fill-realism
gap flagged earlier ("expect a haircut vs his at-touch fills") but far more
severe than a haircut: 63ot's own measured worst slippage in fast dumps is
"scatter to -45%", not -100%+, implying his real execution reacts fast enough
to avoid riding the entire cascade the way this rule's feed-tick + worst-fill
model does.

**Verdict: not a fingerprint problem, not really an entry-quality problem -
it is a tail-risk containment problem.** The fingerprint and even the entry
gates are fine on the 89% of episodes that don't hit this pattern. Next step
(not yet run, ~1hr/combo): re-run the same 3 buckets under `fill_model=first`
(lever #2 in `flow-scalper-validation.rs`'s pattern) to bound how much of the
-6.15 SOL tail is fill-model pessimism vs genuinely unavoidable at any
realistic latency - if `first` fill keeps most of the tail, the fix is
containment (smaller size, a SOL-denominated hard stop independent of %, or
accepting the tail and sizing for it), not the entry/fingerprint logic.
Scratch: none left (this section's data pulled from the live `hunter-lab`
simulate API + `trades`, no scratch tables created). Run id (ephemeral, lab
process memory only, gone on restart): `5f7fc5bf-4efe-460d-8c79-311b17ce819b`.
## `trunoest` - momentum-IGNITION pump-rider, a different family (2026-07-31)

`ardinRsN1mNYVeoJWTBsWeYeXvuR9UUDGMsCDKpb6AT`. The 07-21 note above ("absent
locally, 1000/1000 failed txs") is half-stale: the failures are real - it is a
durable-nonce mass-rebroadcast racer whose spam residue is what a sig scan sees -
but on the rebuilt window (2026-07-22 18:58 -> 07-31 11:26, ~8.7d) its LANDED
legs are in our curve feed: **730 legs / 494 buys / 236 sells / 255 mints**
(0.25% of the 102,208-mint universe - the most selective wallet studied yet).
Scratch tables were dropped after the analysis; the numbers below are the record.

Seed: [`../../../scripts/seed-trunoest-rules.sql`](../../../scripts/seed-trunoest-rules.sql)
(`tru-00` his size / `tru-01` impact-optimal 0.30 SOL, paper, `is_active=false`;
the knob->measurement map is in the script header).

**This is NOT the omego/64hP/63ot dip-reversion family.** It does not scalp
someone else's flow - it manufactures the flow it exits into.

### The loop (one token at a time, one shot per token)

1. **Pick** a very young, very hot, violently-moving token: age at entry med
   **69s** (p10 8.8s, p75 3.1min, p90 7min; ZERO same-slot-as-creation buys - not
   a launch sniper), vsol med **60** (p25 48, p75 72), prior-60s market: **105
   trades / 58 wallets / 70 SOL gross** (hotter than anything omego touches).
   Entry price sits **-19.1% off the 30s high** (p25 -30.2 / p75 -8.5) AND +37%
   above the 30s low - a ~70% 30s range. The 30s net flow is strongly POSITIVE
   (med +8.3 SOL) while the last 2-5s are slightly negative (med -0.45/-0.63):
   he buys the micro-pullback inside an ongoing buy wave. Reaction 0.238s
   (med) after the previous market trade.
2. **Ignite**: ONE oversized buy from a discrete tier ladder of exact
   repeating-decimal constants - **1.4(6) / 1.9(5) / 2.9(3) / 4.8(8) SOL**
   (one big buy per mint, 249/255 mints) - med **3.99% of vsol** = ~+8% own
   price impact. Market net flow flips from **-0.63 (5s before) to +5.72 SOL
   (5s after)** his buy: the spike reliably pulls the crowd in.
3. **Paint the tape while holding**: sprays micro-buys of exactly **0.009(7)
   SOL** (176 legs on 48 mints, ~3.7/mint) and/or **0.24(4) SOL** adds (68 legs
   on 41 mints), 100% AFTER the big buy (med 1-2min after it), 87% while still
   holding, up to ~45-70s before the exit - keeps the token printing on
   screeners/velocity filters while the crowd pumps it.
4. **Dump on confirmed reversal**: exactly ONE full-balance sell (228/232
   sell-mints have 1 sell; 94%+ of tokens fully exited). During the hold the
   token runs a median **+50% above his entry**; he exits **-29.5% off the
   episode peak** (winners -24.4 / losers -38.1) with 2s pre-sell flow negative
   (med -0.80) - i.e. he gives back a third of the pump and leaves only once
   the dump has begun. Post-exit the token falls another **-43.4% (med) within
   2min** (further upside only +6.5%): he consistently exits into the collapse,
   before the bulk of it. This is neither a tight trail nor a fixed TP - it is
   a wide (~25-40%) off-peak reversal-confirmation exit.

### Economics (landed residue only; before the 125bps/leg pump.fun fee)

- 225 fully-closed mints: **63.6% win, med +4.6%/ep** (p25 -4.0 / p75 +17.1 /
  p90 +43.4 / p10 -14.6), closed PnL **+65.2 SOL**. Holds: win med 30.3s, loss
  med 15.0s (p90 127s).
- **Bags are the tax**: 21 big-buy mints never sold, **60.1 SOL sunk**, marked
  med -48.8% vs entry (5 collapsed >70%). Net cash over the window **+21.4
  SOL**; marking bags at current price ~+51 SOL. Est. fees on 1,202 SOL
  turnover ~15 SOL + tips -> true net roughly break-even to modestly positive
  ON WHAT LANDED (the racer presumably loses more edge on the fills it lost).
- Tier PnL says **size hurts**: 1.9(5) is the sweet spot (70 eps, 76% win,
  +10.3 med, +25.4 SOL); 2.9(3) drops to 52%/+0.5; 4.8(8) is flat (58%, +0.0
  total). More impact = more ignition but a worse exit.
- Ops shape: **1 position at a time** (avg concurrency 1.01, max 2), adopts a
  token every ~6.3min while active, and runs only **08:00-22:00 UTC** (peak
  09-14) - a human-scheduled European-daytime operation, not a 24/7 daemon.

### Infrastructure

Every tx: `AdvanceNonceAccount | SetComputeUnitLimit | SetComputeUnitPrice |
[CreateIdempotent] | Axiom Trade: Unknown | Transfer(tip)` - an **Axiom Trade
router** bot using **durable nonces**, which is what enables the mass-rebroadcast
race (same nonce tx sprayed to many senders; one lands, the rest of the spam
fails and is what the 07-21 sig scan saw).

### Replication read

- The entry gate IS expressible in the engine today (age >= ~10s, liquidity
  ~45-85, trail30 >= ~10-30, hot gross60/w60, net30 strongly positive, net2 <=
  0), and the exit is `arm_above_pct`-style wide trail (~30%) - but the +50%
  median peak he rides is partly **caused by his own 4%-of-vsol ignition buy
  and the micro-buy tape-painting**. Copying the entry/exit without the
  ignition mechanic samples a different, weaker distribution - validate via
  simulate before believing any of his numbers transfer.
- His loss containment is the weak spot we should NOT copy: 21 bags / 60 SOL
  (2.4x 63ot's bag rate in SOL terms). A -35..-40% catastrophe SL under the
  wide trail would have kept most of the closed-episode profile intact.
