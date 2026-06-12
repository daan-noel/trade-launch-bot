# tpsl2 Parameter Analysis — Real DB Check

_Generated 2026-06-12. Source: `meme_bot` Postgres DB (local), `tokens` + `trades` tables._

## What's actually in the DB

| Metric | Value |
|---|---|
| Tokens total | 133 (47 Mayhem → excluded by the strategy) |
| Non-Mayhem tokens **with any trades** | **44** |
| Median trades/token | **7** |
| Median token lifespan | **46 s** |
| Median ATH multiple (first→peak) | **1.04×** (basically flat) |
| Tokens with ≥30 trades (enough to form a continuation pattern) | **11** |
| Tokens with ≥60 trades | 8 |

The tpsl2 scalp gates compute over a *trade stream* (higher-lows, cohort dump, organic
flow). With a median of 7 trades and 46 s of life, **most tokens have no stream to
evaluate** — they're dead before the `min_age` gate (30 s) even opens.

## The 11 evaluable tokens, entered at the 30 s mark

| Token | post-entry trades | max gain | secs to peak | worst dip |
|---|---|---|---|---|
| EXht… | 469 | **3.29×** | 41 s | 0.85 |
| Gpb…  | 65  | 1.36× | 64 s | 0.51 |
| DeaF… | 34  | 1.33× | 16 s | 0.59 |
| DZSX… | 43  | 1.10× | 11 s | 0.56 |
| BFip… | 13  | 1.01× | 13 s | 0.94 |
| CoGQ… | 11  | 1.00× | 0 s  | 0.80 |
| 3nit… | 70  | 1.00× | 37 s | 0.96 |

_(max gain = best-case, assumes a perfect exit at the peak.)_

## Honest conclusion

**The data contains exactly one continuation winner (EXht, 3.3×). Everything else is
flat-to-down with –40% to –50% drawdowns.** No parameter set optimized against n=1
winner is *reliable* for real money — it would be a curve-fit to that single token and
would not generalize. Fake-precise "reliable" numbers are deliberately omitted.

The in-app `run_backtest` wouldn't fix this — at this sample size its PnL is noise.
(Also: this DB lacks the `tpsl2_strategy_rules` table — only the older
`strategy_tpsl_rules` exists; migrations are at 0002 here — so the harness can't run
against it as-is.)

## Candidate parameter sets — PAPER-MODE ONLY until validated

Grounded in the one shape the data shows: the winner stayed alive past the spike, kept
volume, and held real reserves ~29 SOL while the flat losers sat at 3–7 SOL. These are
**hypotheses, not reliable values.**

### Candidate A — "survivor continuation" (selective)

```
p_entry_min_age_secs     = 30      # skip the spike (winner peaked at 41s, not at launch)
p_entry_min_liquidity_sol= 15      # winner ~29 SOL; flat losers were 3–7 → this cut is the real filter
p_entry_min_alive_sol    = 3       # require ongoing volume in the 10s window
p_entry_max_cohort_held  = 0.40    # launch cohort must have sold down
p_entry_min_organic_sol  = 2       # outside demand, not cohort wash
p_exit_take_profit       = 70      # winner ran 3.3×; bank a real scalp, don't be greedy
p_exit_stop_loss         = 20      # winner only dipped to 0.85 (-15%); 20% keeps it, cuts the bleeders
p_exit_trailing_stop_pct = 20
p_exit_stall_secs        = 25      # winners moved fast; flat tokens bleed when held
p_exit_time_stop_secs    = 90
p_exit_liquidity_drop_pct= 35
p_exit_cohort_ratio      = 0.10    # E5 rug-dump bail
p_max_concurrent_tokens  = 2
buy_amount               = small   # paper / tiny size
```

### Candidate B — same gates, `p_entry_min_liquidity_sol = 22`

Even more selective; in this data it isolates the EXht/4eDK (20+ SOL) class and skips the
marginal 10-SOL tokens. Fewer entries, higher hit-quality — but also fewer samples, so
it's *more* overfit, not less.

## To make these genuinely reliable

The bar isn't a better sweep — it's more data. You need **hundreds of non-Mayhem tokens
with full trade streams** (not 44 tokens averaging 7 trades). Let the capture firehose
run longer, then re-run this analysis. When you have ~150+ tokens with ≥30 trades and a
winner rate stable across two halves of the sample, a parameter sweep produces values you
can trust with real SOL.
