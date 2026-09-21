# hot-tape: the scripts rule 1 rests on

The chain of steps that produced rule 1 is the table in
[../hot-tape-rule-1.md](../hot-tape-rule-1.md) section 2; evidence numbers are sections of
[_!___evidence.md](../../_!___evidence.md). This folder holds only what a rule or a method gate
needs re-run.

Every script imports `_paths` first: it puts `../toolkit` and the shared `study-kernel/` (kernel,
tape, cvx) on `sys.path`, `data_file(name)` finds a data file in `../data/` or `study-kernel/`
(new outputs go to `../data/`), and `NODE_NAME` is the node's roster label. Run a script from
this folder: `python r1_exact.py audit`. The study tape's wallet ids are `wallet_dict` ids, so a
script that names the node's wallets needs `DATABASE_URL` in `hunter/.env` (a read of
`wallet_dict`, nothing else).

| script | what it does | evidence |
| --- | --- | --- |
| `r1_exact.py` | rule 1 spelled exactly as the engine computes it: `audit` (each correction's cost), `derive MODE STUDY`, `confirm MODE STUDY TAPE...`; writes `data/r1x_*` | 1.22 |
| `r1_exact_check.py` | an independent rebuild of `r1_exact`'s tickets from the raw prints, sharing no code with it (`MODE TAPE N [recall]`) | 1.22 |
| `r1_engine_parity.py` | `prep` writes the inputs of `hunter/lab/examples/hot_tape_rule1_parity.rs` (the engine replay); `compare` matches its positions to the frozen tickets; `book` scores an engine run | 1.23 |
| `r1b_exit.py` | rule 1's entry with an exit read the engine's way (prints, ticks, `LagMs` exit leg, partial legs); `check` reproduces rule 1's tickets, `paths`, `alone [2]`, `combine`, `book`; `ref` freezes rule 1b's tickets for the engine parity (`r1_engine_parity.py compare TAPE CSV r1b_ref`) | 1.24, 1.25 |
| `r1c_loosen.py` | rule 1's entry loosened one term at a time under both exits, the added trades judged on their own net of the rule 1 tickets they displace (derive 12.4); its docstring holds the eight bars: `book`, `walk`, `combine`, `grid`, `slices` | 7 (rule 1c) |
| `b2_leftover.py` | derive 5.2 calibrated: leftover existence on five member x trigger pairs whose fate is known, then applied to the mid-tape node's 9999hu and 88887Q (`toolkit.seat.leftover`) | 1.27 |
| `r1g_members.py` | upgrade plan U1: each paying member's positions classed by the print they follow, checked against rule 1 + Door's terms and booked at our seat (115, 25, 50 ms) | - |
| `r1g_exit.py` | upgrade plan U2: r1b_exit's evaluator and bars on the + Door pool, both exits, with the speed, decay, under-water, sell-on-sell and net-flow exits: `alone`, `alone2`, `combine`, `book` | - |
| `r1g_loosen.py` | upgrade plan U3a: r1c_loosen's walk with the door as a fixed term, study from 09-02: `check`, `grid`, `walk`, `combine` | - |
| `r1g_score.py` | upgrade plan U3b: one additive monotone score over rule 1's six fitted terms in place of the AND, walk-forward | - |
| `r1g_size.py` | upgrade plan U4: rule 1 + Door's tickets re-priced at a clip sized by a score at the fire (0.05-0.5 SOL), against a flat clip of the same mean and random sizing, walk-forward | - |
| `r1w_census.py` | step A of the 8fStGV re-read: its 1,916 positions on study_exact at its own seat - clip, legs, hold, give-back, re-entry, and the slice rule 1 + Door takes | - |
| `r1w_exit.py` | step B: its exit fitted to reproduce ITS closes, family by family and as a greedy OR set, both folds (`alone`, `build`) | - |
| `r1w_his_exit.py` | step B2: that exit frozen and booked on rule 1 + Door's fires, three tapes, r1u_curve_pop's evaluator and bars | - |
| `r1w_eladder.py` | step C: the 6.1 ladder on its acted label over the class prints on its coins (`rank`, `ladder [N] [withp]`) | - |
| `r1w_shold.py` | step C2: the one term both folds hold, a floor under the seller's hold, booked on both rules | - |
| `toolkit_check.py` | the toolkit re-runs steps 16, 21, 24 and 39 next to the recorded numbers | - |

The step scripts the chain cites and this folder does not hold stay in git at `8b01c18b`, the
record as run:

```powershell
git show 8b01c18b:hunter/docs/plans/strategies/node-derivation/hot-tape/<script>
```

They import each other (`cvx_hot_event.py` is the shared helper, `cvx_hot_exit7.py` the exit list,
`cvx_r1u.py` rule 1 over the candidate tables), so restore a step together with what it imports.
