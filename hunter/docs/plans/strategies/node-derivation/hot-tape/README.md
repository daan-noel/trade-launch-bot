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
| `r1c_loosen.py` | rule 1's entry loosened one term at a time under both exits, the added trades judged on their own net of the rule 1 tickets they displace (derive 12.4); its docstring holds the eight bars: `book`, `walk`, `combine`, `grid`, `slices` | 1.26 |
| `b2_leftover.py` | derive 5.2 calibrated: leftover existence on five member x trigger pairs whose fate is known, then applied to the mid-tape node's 9999hu and 88887Q (`toolkit.seat.leftover`) | 1.27 |
| `toolkit_check.py` | the toolkit re-runs steps 16, 21, 24 and 39 next to the recorded numbers | - |

The step scripts the chain cites and this folder does not hold stay in git at `8b01c18b`, the
record as run:

```powershell
git show 8b01c18b:hunter/docs/plans/strategies/node-derivation/hot-tape/<script>
```

They import each other (`cvx_hot_event.py` is the shared helper, `cvx_hot_exit7.py` the exit list,
`cvx_r1u.py` rule 1 over the candidate tables), so restore a step together with what it imports.
