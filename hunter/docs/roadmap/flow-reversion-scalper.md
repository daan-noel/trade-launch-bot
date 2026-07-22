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
