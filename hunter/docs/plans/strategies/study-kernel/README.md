# study-kernel

The one pricing kernel every offline study books through, and the study tape it reads.

## Tracked

* `kernel.py` - `book()`: last print landed by decision + 115 ms on both legs (`entry`/`exit` = `lag`),
  or the end of the decision's slot (`slot_end`, the stress column); exact curve arithmetic on the
  virtual reserve; 125 bps + 0.000225 SOL a leg. Exits: clock cap, take-profit, trail, armed trail,
  tell close; `spec['stop']` is the unarmed stop, and the reserve-to-price conversion is the square
  (reserve -25 % is price -43.75 %).
* `tape.py` - the week tape in memory (chain-ordered Parquet, one contiguous run per token). A tape
  is read with `Tape('<week>_prints.parquet')`.
* `cvx.py` - the study tape's universe and constants (`DAY0` = 2026-08-30 00:00 UTC).
* [frozen-sentences.md](frozen-sentences.md) - every sentence frozen before its holdout, never
  edited after its date.

The node-derivation toolkit and the hot-tape scripts import these three files
([../node-derivation/](../node-derivation/README.md)).

## Local, not tracked

The study tape and the study scripts live on the workstation only (the root `.gitignore` ignores
`*.py` and `*.parquet` here). The tape is a week of curve prints exported chain-ordered from
`aa.pxf` by `cvx_export.py`: `cvx_prints.parquet` with its sidecars (`cvx_tok`, `cvx_ix`,
`cvx_swdoor`, `cvx_meta`). A study script's result lives in its evidence section, not in the script.

The local tools that are checks rather than studies, and are run before believing a book:

| tool | what it checks | evidence |
| --- | --- | --- |
| `kernel_test1.py`, `kernel_test2.py`, `kernel_test3.py` | the kernel: a recorded exit-fill table to the cent; random fires lose on every exit at the verdict fill; the unarmed stop | - |
| `funnel.py` | the raw tape is the universe: how many tokens and prints survive each column, so a filter cannot hide in the query that builds a table | strategy law 15 |
| `cvx_replay3.py` | the roster's own decisions through this kernel at four seats (THEIRS, RACE, PEER, FOLLOW); episodes built from the position, tracked to the token, bags reported. Run it before believing any node verdict. It stops two errors: pricing an entry at `v[their buy]` charges their displacement, and scoring only 1-buy-1-sell episodes takes a re-entry trader's worst trades | 1.4 |
| `cvx_build_holdout.py` | the client gate: per-build books, leave-one-build-out, a bootstrap over builds | 3.1a |
| `cvx_audit_seat.py`, `cvx_audit_floor.py`, `cvx_audit_refrozen.py` | the implementation audit: seat stress one leg at a time, concurrency and capital, the ticket floor per day | 4.8 |
| `audit_bar.py` | the tail bar calibrated on the prize book at matched sample size | 2.3 |
