# hot-tape: the scripts of rule 1's derivation, step by step

The record behind [../hot-tape-rule-1.md](../hot-tape-rule-1.md) section 2. Each script opens with
its step and the question it answers; the step numbers are the ones the chain uses. Evidence
numbers are sections of [_!___evidence.md](../../_!___evidence.md).

Every script imports `_paths` first: it puts `../toolkit` and the shared `study-kernel/` (kernel,
tape, cvx) on `sys.path`, and `data_file(name)` finds a data file in `../data/` or `study-kernel/`, writing
new outputs to `../data/`. Run a script from this folder: `python cvx_hot_trig.py`. The study
tape's wallet ids are `wallet_dict` ids, so the scripts that name the node's wallets need
`DATABASE_URL` in `hunter/.env` (a read of `wallet_dict`, nothing else).

Phase G (the `cvx_r1u*` and `r1u_*` scripts) runs on the toolkit and is the worked example of
it; the earlier steps are the record as run.

| step | script | question | evidence |
| --- | --- | --- | --- |
| H1 | `cvx_hottape.py` | instrument step. The decision moment in TAPE STATE only | 6.10 |
| H1 | `cvx_hottape_book.py` | fire the state on the WHOLE tape and book it at lag_115 | 6.10 |
| H1 | `cvx_hottape_sel.py` | is there a SELECTION inside the flush that pays the toll? | 6.10 |
| H1 | `cvx_hottape_near.py` | WHERE does the rule fire, relative to where they buy? | 6.10 |
| H1 | `cvx_hottape_replay.py` | replay the node's OWN buy and sell decisions through OUR kernel | 1.4 |
| 0 | `cvx_hot_sessions.py` | is their buying a SESSION, and do the six agree? | 1.5 |
| 1 | `cvx_hot_latency.py` | HOW SLOW ARE THEY? The go/no-go gate | 1.5 |
| 2 | `cvx_hot_event.py` | name the trigger, WITHIN THE MINT | 1.6 |
| 3 | `cvx_hot_event2.py` | the conjunction, the DWELL test, and discordance | 1.6 |
| 4 | `cvx_hot_book.py` | book the LULL-then-WAKE event on the whole tape | 1.6 |
| 5 | `cvx_hot_lull.py` | fire on the LULL, not on the WAKE | 1.6 |
| 6 | `cvx_hot_model.py` | a WITHIN-MINT model on the whole feature vector | 1.7 |
| 7 | `cvx_hot_event3.py` | the SAME within-mint design, with the control sampling artifact removed | 1.7 |
| 8 | `cvx_hot_model2.py` | is the within-mint signal real, or is it the control sampling? | 1.7 |
| 9 | `cvx_hot_score.py` | fire the within-mint model on the whole tape and book it | 1.7 |
| 10 | `cvx_hot_door.py` | THE DOOR. Which token groups do their trades live in? | 1.8 |
| 11 | `cvx_hot_exit.py` | THE EXIT SLOT, measured against the convexity they actually enter | 1.10 |
| 12 | `cvx_hot_exit2.py` | the exit SHAPE, searched instead of guessed | 1.10 |
| 13 | `cvx_hot_exit3.py` | is the exit slot empty, or is the SEAT eating it? | 1.10 |
| 14 | `cvx_hot_exit4.py` | does the exit work anywhere, on any slice of their entries? | 1.10 |
| 15 | `cvx_hot_sep.py` | IS THEIR TAIL SEPARABLE AT THEIR OWN DECISION TIME? | 1.11 |
| 16 | `cvx_hot_sep2.py` | the same split, at BOTH seats and inside a fixed reserve band | 1.11 |
| 17 | `cvx_hot_lvl.py` | THE LEVEL-ANCHORED ENTRY, public, on the full tape | 1.11 |
| 18 | `cvx_hot_up.py` | THE UP-MOVE EVENT, derived from the two members that pay at our seat | 1.11 |
| 19 | `cvx_hot_mach.py` | the MACHINE axis, which this node's event study has never used | 1.11 |
| 20 | `cvx_hot_mach2.py` | the independent-machine count as an EVENT, scored on money | 1.11 |
| 21 | `cvx_hot_trig.py` | THEIR TRIGGER PRINT, and therefore their real reaction time | 1.12 |
| 22 | `cvx_hot_seat.py` | the seat we ACTUALLY get if we fire on their trigger | 1.12 |
| 23 | `cvx_hot_dump.py` | BUY THE DUMP PRINT inside a live up-move - the event step 21 named | 1.12 |
| B2c | `b2_leftover.py` | derive 5.2 calibrated: leftover existence on five member x trigger pairs whose fate is known (`toolkit.seat.leftover`) | 1.27 |
| 24 | `cvx_hot_which.py` | WHICH dump prints does the cleanest member buy? | 1.12 |
| 25 | `cvx_hot_dump2.py` | the FRENZY-ABSORBED SELL, as a public sentence on the full tape | 1.12 |
| 26 | `cvx_hot_door2.py` | the DOOR for the frenzy-absorbed sell | 1.12 |
| 27 | `cvx_hot_arrive.py` | on 8fStGV's coins, does the frenzy-sell event pay only when it arrives? | 1.12 |
| 28 | `cvx_hot_door3.py` | the DOOR for the frenzy-absorbed sell, from the UNPRICED side | 1.13 |
| 28b | `cvx_hot_door3b.py` | the two door facts that survive the day split, checked and stacked | 1.13 |
| 29 | `cvx_hot_exit5.py` | THEIR EXIT TRIGGER - what makes the members that pay sell, when they do | 1.13 |
| 30 | `cvx_hot_exit6.py` | the member's OWN exit, booked on the frozen E | 1.13 |
| 31 | `cvx_hot_exit7.py` | a TAPE-DRIVEN exit for the frenzy-absorbed sell, against the static bracket | 1.13 |
| 32 | `cvx_hot_perm.py` | the PERMISSION - which frenzies die | 1.14 |
| 33 | `cvx_hot_perm2.py` | the permission, booked as a sentence | 1.14 |
| 34 | `cvx_hot_holdout.py` | the HOLDOUT - the frozen sentence on days it has never seen | 1.15 |
| 34 | `cvx_holdout_export.py` | The HOLDOUT tape: the lake's sealed days converted to the study tape's exact format | 1.15 |
| 35 | `cvx_hot_which2.py` | the SECOND event - which triggers the other paying operator acts on | 1.16 |
| 36 | `cvx_hot_ev2.py` | the SECOND event as a public sentence - the capitulation cascade | 1.16 |
| 37 | `cvx_hot_ev2b.py` | is the second leg's dip-buy conditioned on the OPERATOR already being in? | 1.16 |
| 38 | `cvx_hot_ev2c.py` | the second leg's decision point - its partner, and the public print behind it | 1.16 |
| 39a | `cvx_hot_exit8a.py` | 8fStGV's exit hazard, read on the pool rule 1 selects | 1.17 |
| 39b | `cvx_hot_exit8.py` | rule 1's exit, re-derived on its own pool and booked on both tapes | 1.17 |
| 40 | `cvx_hot_size.py` | rule 1's clip (the S slot) | 1.18 |
| 41 | `cvx_hot_door4.py` | rule 1's door - the arrival of a sell-reactive buyer, and the stop-outs | 1.19 |
| G1 | `cvx_r1u_cand.py` | one candidate table per tape (toolkit/candidates.py); its actor columns are G0 | 1.20 |
| G1 | `cvx_r1u.py` | rule 1 as a spec over the candidate tables (cvx_r1u_cand.py), and its loaders | 1.20 |
| G2 | `cvx_r1u_step1.py` | every threshold re-derived by walk-forward on money | 1.20 |
| G3 | `cvx_r1u_grad.py` | the exits that land on the graduation print (toolkit/graduation.py) | 1.20 |
| G4 | `cvx_r1u_step2.py` | new E terms, each a one-sided cut added to the G2-G3 sentence | 1.20 |
| G5 | `cvx_r1u_exit.py` | the exit re-read on the updated sentence's own pool | 1.20 |
| G5 | `r1u_exit_axes.py` | the exit, one axis at a time by walk-forward (toolkit/walkforward.axes) | 1.20 |
| G5 | `r1u_tp_check.py` | the take profit re-read at the kept stop and clock (-40 %, 90 s) | 1.20 |
| G6 | `r1u_reentry.py` | re-entry (R) on the updated pool | 1.20 |
| G7 | `r1u_size.py` | the clip, flat against a share of the reserve | 1.20 |
| G8 | `r1u_holdout.py` | the holdout, one change at a time | 1.20 |
| G9 | `r1_replay.py` | rule 1 replayed print by print, sharing no code with the candidate table: the parity reference for the engine | 1.21 |
| G10 | `r1_terms_audit.py` | every term's input against an independent exact field of the lake | 1.22 |
| G11-G12 | `r1_exact.py` | rule 1 spelled exactly as the engine computes it: `audit` (each correction's cost), `derive MODE STUDY`, `confirm MODE STUDY TAPE...`; writes `data/r1x_*` | 1.22 |
| G12 | `r1_exact_check.py` | an independent rebuild of `r1_exact`'s tickets from the raw prints (`MODE TAPE N [recall]`) | 1.22 |
| G13 | `r1_engine_parity.py` | `prep` writes the inputs of `hunter/lab/examples/hot_tape_rule1_parity.rs` (the engine replay); `compare` matches its positions to the frozen tickets; `book` scores an engine run | 1.23 |
| G14 | `r1b_exit.py` | rule 1's entry with an exit read the engine's way (prints, ticks, `LagMs` exit leg, partial legs); `check` reproduces rule 1's tickets, `paths`, `alone [2]`, `combine`, `book`; `ref` freezes rule 1b's tickets for the engine parity (`r1_engine_parity.py compare TAPE CSV r1b_ref`) | 1.24, 1.25 |
| G15 | `r1c_loosen.py` | rule 1's entry loosened one term at a time under both exits, the added trades judged on their own: `book` (every candidate, 115 / 200 ms, cached), `walk`, `combine`, `grid` and `slices` (read only) | 1.26 |
| - | `toolkit_check.py` | the toolkit re-runs steps 16, 21, 24 and 39 next to the recorded numbers | - |

Shared helpers other steps import: `cvx_hot_event.py` (`node_ids`, `NODE_NAME`, the
within-mint features), `cvx_hot_exit7.py` (the exit list of step 31; the engine is
`toolkit/exits.py`), `cvx_hot_which.py` (re-exports `toolkit/contrast.strat_rank`), `cvx_hot_trig.py`
(the print classes of step 21), `cvx_hot_perm2.py` (`run()`, rule 1 on any tape before phase G),
`cvx_r1u.py` (rule 1 as a spec over the candidate tables).
