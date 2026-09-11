# mid-tape: scripts of rule 1 and rule 2

The record behind [../mid-tape-rule-1.md](../mid-tape-rule-1.md) (9Uq8GV) and
[../mid-tape-rule-2.md](../mid-tape-rule-2.md) (9999hu). Each script opens with its step
and the question it answers. Evidence: [_!___evidence.md](../../_!___evidence.md)
5.8, 5.9, 5.10, 5.11, 5.12, 5.13. Playbook: [_!___derive.md](../../_!___derive.md).

The scripts are local scratch and not tracked (only `_paths.py` is): each step's record is its
chain row in the case file and its numbers the evidence section named below.

Every script imports `_paths` first. Run from this folder: `python mt_hu_p5.py`. The study
tape's wallet ids are `wallet_dict` ids, so scripts that name the node's wallets need
`DATABASE_URL` in `hunter/.env` (a read of `wallet_dict`, nothing else).

5.8-5.10 score a hold-matched clock and every-fire occupancy on 9Uq8GV. Those columns stay
as diagnostics. 5.11 is leftover existence on 9999hu's acted sell >= 1, then which sells,
then public occupancy. 5.12 is phase 8 on that acted pool. 5.13 is phase 9 (permission) on
the same pool.

| step | script | question | evidence |
| --- | --- | --- | --- |
| 3-5 | `mt_p15.py` | who pays, shape, each member's trigger, DELAY columns at 115 ms | 5.8 |
| 6.1-6.2 | `mt_p6.py` | which buy >= 1 9Uq8GV takes; public sentence and held-state spelling | 5.9 |
| 6.1 | `mt_p6_win.py` | is the 300 ms acted set the trigger or the burst? (75-300 ms) | 5.9 |
| 6.2 | `mt_p6_unpriced.py` | the same full-tape tables with price-path terms removed | 5.9 |
| 5.1-5.3 | `mt_p5b.py` | unpriced state at 9Uq8GV's buy; DELAY columns of pro / npro / nb5 | 5.10 |
| 5.2-5.3 | `mt_p5b_nb2.py` | DELAY columns of the nb2 rising edge the contrast names | 5.10 |
| 5.2-6.1 | `mt_hu_p5.py` | leftover on the sells 9999hu takes; which of those sells | 5.11 |
| 6.2 | `mt_hu_p6.py` | public sell >= 1 + unpriced terms, every coin, clock 25 | 5.11 |
| 6.2 | `mt_hu_p6b.py` | each term alone, and age bands (launch first-sell vs its fire) | 5.11 |
| 8.1-8.2 | `mt_hu_p8.py` | closing hazard on the acted pool; exit families against that bracket | 5.12 |
| 9.1 | `mt_hu_p9.py` | stop-outs vs take-profits at the fire; keep-rule P cut | 5.13 |
