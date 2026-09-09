# study-kernel

The one pricing kernel every offline study in this folder books through, with its acceptance tests.

* `kernel.py` - `book()`: last print landed by decision + 115 ms on both legs (`entry`/`exit` = `lag`),
  or the end of the decision's slot (`slot_end`, the stress column); exact curve arithmetic on the virtual
  reserve; 125 bps + 0.000225 SOL a leg. Exits: clock cap, take-profit, trail, armed trail, tell close.
* `tape.py` - the week tape in memory (chain-ordered Parquet, one contiguous run per token).
* `kernel_test1.py` - reproduces a recorded exit-fill refutation table to the cent from the same inputs.
* `kernel_test2.py` - random fires on the full tape must lose on every admissible exit at the verdict fill;
  the zero-lag column beside it is the size of the reactive-exit artifact (`kernel_test2_results.csv`).
* `frozen-sentences.md`, `n4_holdout.py`, `holdout_v2.py` - every sentence frozen before its holdout, and the runners that
  prints the detectors (no-print share, gap to next print, trail fire rate, top-token share, max open).
* `n4_probe.py`, `n4_money.py`, `n4_crsold.py`, `wallet_ctx.py`, `wk_census.py` - the N4 study
  ([../_!___evidence.md](../_!___evidence.md) 5.4, and the audit that reopened the node:
  [2026-09-09-closure-ledger-audit](../../../../../docs/history/2026-09-09-closure-ledger-audit.md)).

* `funnel.py` - the raw tape is the universe: prints how many tokens and prints survive each column, so a
  filter cannot hide inside the query that builds a table. Run it at the top of a study.
* `census_graph.py` - Phase 1 as a relationship graph: launch build to creation-slot buy builds, the top
  post-creation buy machine per token, and the buy build to sell build pairing (the extraction tell).
* `cadence.py`, `cadence_event.py`, `tell_rule.py` - the machine-cadence and extraction-tell studies
  ([../_!___evidence.md](../_!___evidence.md) 7 books table).

* `episodes.py` - every up-episode on a week of tape (swing low to swing high, 15 % retracement confirms a
  peak) with the reachable remainder after a 115 ms fill; `breakout.py` / `breakout2.py` fire on the rise and
  on the pullback with the demonstrated-convexity door; `selection.py` / `selection2.py` label candidate
  prints by whether an N4 wallet acted and price EVERY fire in each learned cell
  ([2026-09-08-convexity-is-reachable-selection-is-the-gap](../../../../../docs/history/2026-09-08-convexity-is-reachable-selection-is-the-gap.md)).

* `holderbook.py` / `analyze_hb.py` - the holder book rebuilt from our own prints (holders, top-1/top-5
  share, HHI, bundle share, never-sold share, fresh-wallet share, dev share, supply sold back).
* `tokenpick.py` - token-level selection decided at the first candidate print, including the meta-cluster
  feature; `convergence.py` - distinct private buy builds already in the token; `grid.py` - the full age x
  reserve grid
  ([2026-09-08-the-edge-is-token-level-and-not-yet-reproducible](../../../../../docs/history/2026-09-08-the-edge-is-token-level-and-not-yet-reproducible.md)).

* `cvx_export.py` / `cvx.py` / `cvx_slice.py` - last-leg `aa.pxf` week as the universe; every
  up-episode, the demonstrated-episode door, the zigzag up-turn as the event, leftover after
  115 ms, honest money vs the oracle ceiling
  ([../_!___evidence.md](../_!___evidence.md) 2.1).
* `cvx_ix_export.py` / `cvx_gates.py` - exact `ix_labels` sidecar; each 8dtx decision-print
  structure, create-sequence door, holder-state gates, and the first-print-off-low event,
  scored on P(swing >= 50%) and 115 ms money
  ([../_!___evidence.md](../_!___evidence.md) 6.1).
* `cvx_meta_export.py` / `cvx_fetch_meta.py` / `cvx_restart.py` - project-document door
  (website and (telegram or description > 80 chars)) plus the campaign-restart event
  (episode-1 pusher or new professional after >= 10 silent buy-slots), scored against
  the zigzag-turn parent
  ([../_!___evidence.md](../_!___evidence.md) 3.3, 6.1).
* `cvx_feed.py` - unique new-on-mint buyers in a short slot window as the event
  (no price percent in the trigger), keep vs dump-factory control, scored against
  the zigzag-turn parent
  ([../_!___evidence.md](../_!___evidence.md) 6.1).
* `cvx_oracle.py` - daily-profitable wallets (defined on days 0-2, scored on
  day >= 3) as a token oracle with before-arrival honesty, plus episode-1
  wallet resume on other mints, both on the zigzag-turn parent
  ([../_!___evidence.md](../_!___evidence.md) 5.5, 7).
* `cvx_attn.py` / `cvx_door_urls.py` / `cvx_link_fetch.py` / `cvx_attn_fetch.py` -
  telegram-first document door, website/telegram link resolution, and delayed
  pump.fun coin attention, on both the zigzag parent and the documented-project
  survival parent
  ([../_!___evidence.md](../_!___evidence.md) 3.3, 8).
* `cvx_harvest.py` - harvester sentence on a **machine-print** event (Terminal buy in
  the dip after a demonstrated episode, creator in, last-hill crowd gone), ranked on
  total SOL with ablation and tail share
  ([../_!___evidence.md](../_!___evidence.md) 6.2).
* `cvx_stack.py` - story-derived conjunctions scored on the zigzag-turn parent
  (leftover description as the event grain, not unpriced state).

* `fetch_meta.py` / `analyze_meta.py` - fetch the metadata JSON behind `tokens.meta->>'uri'` (website,
  telegram, description) and price every field; `stacked.py` / `stacked2.py` - the story-derived conjunction,
  one term at a time; `smartset.py` - a wider oracle built on the previous week; `frozen-sentences.md` and
  `holdout_v2.py` - the frozen sentence and its holdout runner
  ([2026-09-08-documented-project-rule-first-positive-holdout](../../../../../docs/history/2026-09-08-documented-project-rule-first-positive-holdout.md)).

* `audit_exits.py` / `audit_exit2.py` - the same fires under six and then fifteen exits, with each
  cell's OWN realised break-even and the loss-distribution buckets
  ([../_!___evidence.md](../_!___evidence.md) 4.2, 4.3).
* `audit_conj.py` / `audit_conj2.py` / `audit_conj3.py` - the conjunction space on both event
  families: every 2- to 4-term cell, both exits, the ticket floor, tail share and a fit/hold
  walk-forward ([../_!___evidence.md](../_!___evidence.md) 6).
* `audit_tailbar.py` / `audit_bar.py` - the tail bar calibrated on the prize book at matched
  sample size, which is what "carried by the top 1 %" is read against
  ([../_!___evidence.md](../_!___evidence.md) 2.3).
* `audit_final.py` - the document ablation on the machine-print parent
  ([../_!___evidence.md](../_!___evidence.md) 6.2).

Method, gates and the campaign queue: [../_!___workflow.md](../_!___workflow.md).

Inputs are a week's curve prints exported chain-ordered from `aa.pxf` (`cvx_export.py`) or
from `trades`. A tape is read with `Tape('<week>_prints.parquet')`.
