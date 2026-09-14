# mid-tape: scripts of rule 3

The record behind [../mid-tape-rule-3.md](../mid-tape-rule-3.md) (instruments 8dtx2t, 88887Q, 3Xk2Eu, 8aaRWu).
Each script opens with its step and the question it answers. Method:
[_!___derive.md](../../_!___derive.md).

The scripts are local scratch and not tracked: each step's record is its chain row in the case
file named below. Tracked: `_paths.py`, and `mt_d8_replay.py` / `mt_d8_fillcheck.py`, which
[backtest-audit.md](../../backtest-audit.md) re-runs (V1, F2).

Every script imports `_paths` first. Run from this folder: `python mt_p34.py`. Phase 3-4
loads `study_exact` (every leg, wallet map on the tape). Last-leg `study` does not carry 3Xk2Eu.
Study **fires** stop at 09-06 12:00 (derive 2.1): loading the tape does not cut them.

| step | script | question | case file |
| --- | --- | --- | --- |
| 3-4 | `mt_p34.py` | shape and who pays, every roster member, every-leg study | rule 3 |
| 4.0 | `mt_volmaker.py` | tape share / wash / clips / creator on the 26: reader or volume manufacture | rule 3 |
| 4.0 | `mt_volmaker_ix.py` | lake ix mix: VolAcc / 6Vo / TransferChecked / CreateCoinAndBuy | rule 3 |
| 5.1-5.2 | `mt_p51.py` | print 8dtx2t reacts to, leftover at 115 ms on that print | rule 3 |
| 5.1-5.2 | `mt_p51_88887Q.py` | print 88887Q reacts to (WHO / history / priced), leftover at 115 ms | rule 3 |
| lag ladder | `mt_p51_88887Q_lag.py` | leftover at 50 / 115 ms on seller_loss / sell>=1 / down>=2 % | rule 3 |
| 5.1-5.2 | `mt_p51_3Xk2Eu.py` | print 3Xk2Eu reacts to (WHO / history / priced), leftover at 115 ms | rule 3 |
| 5.1-5.2 | `mt_p51_8aaRWu.py` | print 8aaRWu reacts to (WHO / history / priced), leftover at 115 ms | rule 3 |
| overlap / exclusive / 6.1 | `mt_p61_8aaRWu.py` | identity-family overlap, exclusive leftover, which prints it takes | rule 3 |
| 6.2 | `mt_p62_8aaRWu.py` | 6.1 terms one at a time on every coin, leftover, opposite-side control | rule 3 |
| 6.2 acted | `mt_p62_tail.py` | occupancy on the fires it takes (not a sentence) | rule 3 |
| 5.1-5.2 exclusive | `mt_p51_excl.py` | leftover on exclusive quiet restart / continuation / loud restart | rule 3 |
| lag ladder | `mt_p51_lag.py` | leftover at 10 / 50 / 80 / 115 ms on those exclusive prints | rule 3 |
| anatomy | `mt_d8_anatomy.py` | where 8dtx2t's decision buys land: slot, place in the burst, what printed before | rule 3b |
| 5.1-5.2 wide | `mt_d8_p51wide.py` | 8dtx2t's full class scan, 5.2 at 115 and 83 ms | rule 3b |
| 5.1 / 6.1 / 6.2 | `mt_d8_e.py` | `scan` first positions, `pick` which prints it takes, `spell` every coin, `seats` the ladder | rule 3b |
| 7 | `mt_d8_d.py` | 7.1 split and 7.3 door walk (`walk`); door facts from `mt_d8_door.py`'s running sums | rule 3b |
| 8 | `mt_d8_x.py` | `hazard` its closes, `book` the families, `cut` its own cut walked, `final` on the final pool | rule 3b |
| 9 / 12 | `mt_d8_p.py` | `p` permissions, `rs` re-entry and clip, `gates`, `pfinal` | rule 3b |
| seat audit | `mt_d8_fillcheck.py` | does the fill model price the state our real buys met (their signatures on the lake) | rule 3b |
| 12.10-12.11 | `mt_d8_replay.py` | rule 3b on the lake from 09-01, sharing no code with the study: `slim`, `cands`, `repro` (the study's tickets), `book` (one change at a time, every window) | rule 3b |
