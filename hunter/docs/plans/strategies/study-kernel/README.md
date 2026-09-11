# study-kernel

The one pricing kernel every offline study in this folder books through, with its acceptance tests.

* `kernel.py` - `book()`: last print landed by decision + 115 ms on both legs (`entry`/`exit` = `lag`),
  or the end of the decision's slot (`slot_end`, the stress column); exact curve arithmetic on the virtual
  reserve; 125 bps + 0.000225 SOL a leg. Exits: clock cap, take-profit, trail, armed trail, tell close.
* `tape.py` - the week tape in memory (chain-ordered Parquet, one contiguous run per token).
* `kernel_test1.py` - reproduces a recorded exit-fill refutation table to the cent from the same inputs.
* `kernel_test3.py` - the unarmed stop (`spec['stop']`): inert when the key is absent, exact when
  present, and the reserve-to-price conversion is the square (reserve -25 % is price -43.75 %).
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

* `cvx_ldoor.py` - C1: L-door (bundle / fresh / sniper / creator-sold / dev hold) on the
  documented-project parent, trail40 c600 L-label
  ([../_!___evidence.md](../_!___evidence.md) 3.6).
* `cvx_door_export.py` - C0a: trailing slow-wall launch-door labels (`cvx_swdoor.parquet`)
  from `launch_build_day_stats` (previous UTC day) onto the last-leg tape
  ([../_!___evidence.md](../_!___evidence.md) 3.1).
* `cvx_money.py` - C0b: door-v3 MONEY at lag_115 through floor / tail / fit-hold
  ([../_!___evidence.md](../_!___evidence.md) 7.0).
* `cvx_burst.py` - C2: door x burst START
  ([../_!___evidence.md](../_!___evidence.md) 6.4).
* `cvx_c2_slowwall.py` - C2 behind the slow-wall launch door, on the fires `cvx_burst.py`
  already wrote ([../_!___evidence.md](../_!___evidence.md) 6.4).
* `cvx_c0b.py` - C0b: door-v3 MONEY re-derived from the tape and the C0a labels, at `lag_115`
  and SlotEnd, with the floor, tail and walk-forward gates
  ([../_!___evidence.md](../_!___evidence.md) 7.0). `cvx_c0b_ldoor.py` puts the L-axis on its
  fires and `cvx_c0b_control.py` is the reachability control that decides which L-terms are
  real ([../_!___evidence.md](../_!___evidence.md) 3.7).
* `cvx_c3_frozen.py` - C3: the frozen campaign-break fingerprint (`ixh` 29d9aacb…, 42,178
  prints) at `lag_115`, both exit families, ablation, seats, payer client gate
  ([../_!___evidence.md](../_!___evidence.md) 6.6). `cvx_c3_campaign.py` is the
  concentration-species widening of the same event, separately red.
* `cvx_c4_exitlab.py` - C4: the exit lab on a FIXED entry that clears the client gate. Four
  state-conditional cuts derived from the sentence's failure modes, each alone, under a trail,
  and in a loss-only form; the static abort grid is not rerun
  ([../_!___evidence.md](../_!___evidence.md) 4.7).
* `cvx_audit_seat.py` / `cvx_audit_floor.py` / `cvx_audit_refrozen.py` - **the implementation
  audit of the frozen sentence** ([../_!___evidence.md](../_!___evidence.md) 4.8): seat stress
  one leg at a time, concurrency and capital, the ticket floor PER DAY (it is a per-day refusal
  and a mean hides a rotating client), and the sentence re-priced with the wallet-identity term
  removed. Run these before believing any cell.
* `cvx_build_holdout.py` - **the client gate**: per-build books, leave-one-build-out, and a
  bootstrap over builds, for every candidate cell
  ([../_!___evidence.md](../_!___evidence.md) 3.1a, [../_!___workflow.md](../_!___workflow.md) 4).
  `cvx_client_diag.py` names the clients, `cvx_c2_diag.py` finds the term that collapses across
  the week, `cvx_creator_diag.py` clears the creator field of being the cause.
* `cvx_quiet.py` / `cvx_quiet_ladder.py` - C5: quiet deep-age. Reconstruct GZmUDs's decision
  print, then score token-silence burst START and returning-pro restart
  ([../_!___evidence.md](../_!___evidence.md) 6.5).
* `cvx_c6_agreement.py` - C6: the solo 26 as a creation-sequence door (refuted, concentration
  1.02) and as AGREEMENT, the count of them already in a coin at the decision print
  ([../_!___evidence.md](../_!___evidence.md) 6.8). `cvx_c6_control.py` is the leakage control
  (the ten roster wallets that failed the profit cut, plus 20 activity-matched random draws);
  `cvx_c6_oos.py` re-reads it outside the window the roster was selected on. `solo26.json`
  is the roster with its `wallet_dict` ids, from `hunter/_local/solo-traders.csv`.
* `cvx_bigclip.py` - G1 deep-age big clip: reconstruct D9Uite's decision print, then score
  a public size buy on a live mid-life tape
  ([../_!___evidence.md](../_!___evidence.md) 6.7).
* `cvx_midtape.py` - mid-tape one-shot instrument step: unused members' decision print
  (`8aaRWu` / `ApfmkS` after `9999hu` / `88887Q` / `9Uq8GV`), create-cgroup and ix_count
  census vs the tape, response rate vs base. Their prints stay out. Not a scored sentence.
* `cvx_midtape_ep.py` - mid-tape episode instrument: this node's round-trips split 1 / 2 / 3+
  on the same mint. At each open, the four unpriced facts (independent machines, still
  holding, creator sold, last-buy machine) from prints `0..i-1`, node-blind. Names D if
  something is stable across episodes and rare on the tape, then scores D x burst START.
  Their prints stay out. Mint list is never D.
* `cvx_midtape_hold.py` - the fourth fact in the terms it is defined: token balances at
  episode open (never-sold, creator share, sold-back, remaining pro-buyer share), not
  last-side. Node-blind, prints `0..i-1`. Then D x burst START if named.

* `cvx_conj4.py` / `cvx_conj4_diag.py` / [cvx_conj4.md](cvx_conj4.md) - legal D x E x P x X
  occupancy search, artifact detectors, and the re-check sheet for the SOL leader
  ([../_!___evidence.md](../_!___evidence.md) 6.13). Rank file `cvx_conj4_rank.csv`.
  `cvx_meta.parquet` is the keep+ep50 shortlist, not documented-project on the tape
  (strategy 7.4 law 30). Do not reuse it as D until metadata covers the tape.
* `cvx_conj5.py` / `cvx_conj5_diag.py` / [cvx_conj5.md](cvx_conj5.md) - C9: four new events
  (staged-leg, unfinished-budget, rebuy-under-exit, after-flush) walked against standing
  D, P, X. Age >= 60 s ranking slice beside the full tape
  ([../_!___evidence.md](../_!___evidence.md) 6.14). Rank file `cvx_conj5_rank.csv`.
* `cvx_conj6.py` / [cvx_conj6.md](cvx_conj6.md) - C10: S4 as sell-then-buy (no is_pro),
  census of the emptying cut, then walked against standing D, P, X. Age >= 60 s ranking
  slice ([../_!___evidence.md](../_!___evidence.md) 6.15). Rank file `cvx_conj6_rank.csv`.
* `cvx_conj7.py` / [cvx_conj7.md](cvx_conj7.md) - C11: late-leg (cluster 2+) of a staged-leg
  machine, walked against standing D, P, X. Age >= 60 s ranking slice
  ([../_!___evidence.md](../_!___evidence.md) 6.16). Rank file `cvx_conj7_rank.csv`.
* `cvx_day_skew.py` - tape births, door supply, and first-per-mint of the C8-C11 SOL
  leaders, per UTC day. Peak/trough and top-2 share. The TYPE check
  ([../_!___evidence.md](../_!___evidence.md) 4.9, [../_!___workflow.md](../_!___workflow.md) 4).
* `cvx_conj_reread.py` - C8-C11 re-read under laws 29-31: sidecar funnel, k==0 share,
  documented dropped, TYPE then SOL
  ([../_!___evidence.md](../_!___evidence.md) 4.10).
* `cvx_conj8.py` / [cvx_conj8.md](cvx_conj8.md) - C12: this-coin second-attempt burst
  (any ix structure, one fire per (coin, structure)), walked against standing D, P, X.
  documented is not a door. Age >= 60 s, TYPE then SOL
  ([../_!___evidence.md](../_!___evidence.md) 6.17). Rank file `cvx_conj8_rank.csv`.
* `cvx_conj9.py` / [cvx_conj9.md](cvx_conj9.md) - C13: arrives from another coin
  (latest print is a buy on a different coin, slot gap < 10), walked against standing
  D, P, X. documented is not a door. Age >= 60 s, TYPE then SOL
  ([../_!___evidence.md](../_!___evidence.md) 6.18). Rank file `cvx_conj9_rank.csv`.
* `cvx_conj10.py` / [cvx_conj10.md](cvx_conj10.md) - C14: first buy after a run of
  sells (>= 3 consecutive sells, then >=0.5 non-racer buy), walked against standing
  D, P, X. documented is not a door. Age >= 60 s, TYPE then SOL
  ([../_!___evidence.md](../_!___evidence.md) 6.19). Rank file `cvx_conj10_rank.csv`.
* `cvx_conj11.py` / [cvx_conj11.md](cvx_conj11.md) - C15: first operator structure
  after only creator and seed racers, walked against standing D, P, X plus
  group-live-now as a door. documented is not a door. Age >= 60 s, TYPE then SOL
  ([../_!___evidence.md](../_!___evidence.md) 6.20). Rank file `cvx_conj11_rank.csv`.
* `cvx_conj12.py` / [cvx_conj12.md](cvx_conj12.md) - C16: first run of an operator
  structure on a coin that already has others, walked against standing D, P, X
  plus group-live-now as a door. documented is not a door. Age >= 60 s, TYPE then SOL
  ([../_!___evidence.md](../_!___evidence.md) 6.21). Rank file `cvx_conj12_rank.csv`.

* The hot-tape node's scripts (`cvx_hottape*.py`, `cvx_hot_*.py`, the rule 1 update `cvx_r1u*.py`,
  `r1u_*.py`, `cvx_holdout_export.py`) live in
  [../node-derivation/hot-tape/](../node-derivation/hot-tape/README.md), beside the method they ran
  ([../node-derivation/method.md](../node-derivation/method.md)) and its toolkit. They import this
  folder's `kernel.py`, `tape.py` and `cvx.py`, and read the study tape here.

* `cvx_replay3.py` - **the reconciliation, and it is the one to run before believing any node
  verdict** ([../_!___evidence.md](../_!___evidence.md) 1.4). It prices the roster's OWN buy and
  sell decisions through this kernel with our clip, at four seats: THEIRS, RACE (sequenced before
  their print), PEER (same slot, the leader's coin flip) and FOLLOW (after it, with and without
  115 ms). Episodes are built from the POSITION, tracked to the token from `K = vsol * vtok`,
  open-to-flat, and bags are reported rather than dropped. It reconciles with `margin_pct` across
  25 wallets, which is what makes the seat numbers trustworthy.
  **Two things it exists to stop you repeating**, both preserved in the superseded versions
  `cvx_replay2.py` and `../node-derivation/hot-tape/cvx_hottape_replay.py`: `v[k]` is the reserve AFTER print k, so pricing an
  entry at `v[their buy]` charges you THEIR displacement (use `v_before(k) = v[k] - signed
  amount`); and scoring only 1-buy-1-sell episodes takes a re-entry trader's WORST trades.

Inputs are a week's curve prints exported chain-ordered from `aa.pxf` (`cvx_export.py`) or
from `trades`. A tape is read with `Tape('<week>_prints.parquet')`.
