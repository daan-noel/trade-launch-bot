# mid-tape: scripts of rule 1 and rule 2

The record behind [../mid-tape-rule-1.md](../mid-tape-rule-1.md) (9Uq8GV) and
[../mid-tape-rule-2.md](../mid-tape-rule-2.md) (9999hu). Each script opens with its step
and the question it answers. Playbook: [_!___derive.md](../../_!___derive.md).

The scripts are local scratch and not tracked (only `_paths.py` is): each step's record is its
chain row in the case file named below. The two closed lines are rows of
[_!___evidence.md](../../_!___evidence.md) 7.

Every script imports `_paths` first. Run from this folder: `python mt_hu_p5.py`. The study
tape's wallet ids are `wallet_dict` ids, so scripts that name the node's wallets need
`DATABASE_URL` in `hunter/.env` (a read of `wallet_dict`, nothing else).

| step | script | question | case file |
| --- | --- | --- | --- |
| 3-5 | `mt_p15.py` | who pays, shape, each member's trigger, DELAY columns at 115 ms | rule 1, rule 2 |
| 6.1-6.2 | `mt_p6.py` | which buy >= 1 9Uq8GV takes; public sentence and held-state spelling | rule 1 |
| 6.1 | `mt_p6_win.py` | is the 300 ms acted set the trigger or the burst? (75-300 ms) | rule 1 |
| 6.2 | `mt_p6_unpriced.py` | the same full-tape tables with price-path terms removed | rule 1 |
| 5.1-5.3 | `mt_p5b.py` | unpriced state at 9Uq8GV's buy; DELAY columns of pro / npro / nb5 | rule 1 |
| 5.2-5.3 | `mt_p5b_nb2.py` | DELAY columns of the nb2 rising edge the contrast names | rule 1 |
| 5.2-6.1 | `mt_hu_p5.py` | leftover on the sells 9999hu takes; which of those sells | rule 2 |
| 6.2 | `mt_hu_p6.py` | public sell >= 1 + unpriced terms, every coin, clock 25 | rule 2 |
| 6.2 | `mt_hu_p6b.py` | each term alone, and age bands (launch first-sell vs its fire) | rule 2 |
| 8.1-8.2 | `mt_hu_p8.py` | closing hazard on the acted pool; exit families against that bracket | rule 2 |
| 9.1 | `mt_hu_p9.py` | stop-outs vs take-profits at the fire; keep-rule P cut | rule 2 |
