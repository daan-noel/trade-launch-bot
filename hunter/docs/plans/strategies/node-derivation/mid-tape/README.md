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
| 6.1 | `mt_p61_3Xk2Eu.py` | which class prints 3Xk2Eu takes: acted share per class, rank inside `alone_in_slot`, leftover on it (0.08 % acted; a corner at 2.08 % / 5.6 % cover) | rule 3 |
| 6.1 ladder | `mt_p61b_3Xk2Eu.py` | the E ladder on all its fires: depth 3 in both folds (flow_spike AND thin tape AND mv10) | rule 3 |
| 5.2 on the spelled E | `mt_p61c_3Xk2Eu.py` | is the laddered E reachable: the whole sentence, each term, each pair, the earlier footprint | rule 3 |
| 6.1 ladder in the pair | `mt_p61d_3Xk2Eu.py` | tighten the reachable pair with the price-move family barred (depth 1: `mvk >= 4.246`) | rule 3 |
| 5.2 + coverage | `mt_p61e_3Xk2Eu.py` | 1-in-N, share of its buys covered, and leftover per tightened spelling | rule 3 |
| 6.2 occupancy | `mt_p62_3Xk2Eu.py` | the sentence on every coin of the tape under a hold-matched clock (red 0/6) | rule 3 |
| 7.1 | `mt_p71_3Xk2Eu.py` | its coins split at its first buy: before +13.12 % 6/6, after -1.32 % | rule 3 |
| 7.3 | `mt_p73_3Xk2Eu.py` | do the tape's coin facts pick its coins before it arrives (no: every quintile red) | rule 3 |
| 7.3b | `mt_p73b_3Xk2Eu.py` | the loss door as D on its tightened class (-3.92 % 0/5: D none) | rule 3 |
| entry place | `mt_where_3Xk2Eu.py` | where in the coin it buys: move before, distance from the high, buyers ahead of it, move left above it (mid-move, ~79th buyer, +20 % left in 60 s) | rule 3 |
| A co-fire | `mt_who_3Xk2Eu.py` | who else buys within +-0.2 / 1 / 5 s of it, against a same-coin chance anchor (the tight set follows it; nobody leads it) | rule 3 |
| A2 live door | `mt_who2_3Xk2Eu.py` | the leading wallets as a walk-forward door: does the mark beat its first buy, and does the marked class pay (75 % caught at 4 % precision; money still red) | rule 3 |
| B1 modes | `mt_b1_3Xk2Eu.py` | split its positions six ways and describe each bucket by what it was not cut on (clip flat at 0.72 SOL; dip and chase are two behaviours) | rule 3 |
| B2 ladder per mode | `mt_b2_3Xk2Eu.py` | the 6.1 ladder inside each mode (chase 68 % cover, mild 23.5 %, dip 13.8 %) | rule 3 |
| B3 mode money | `mt_b3_3Xk2Eu.py` | every mode on every coin under six exits, its coins beside (24 cells, none green on every coin) | rule 3 |
| 8.1 / 9.1 | `mt_p9_3Xk2Eu.py` | `hazard` its closing hazard at fine bins per mode (no take-profit band: a loss-capped ride), `p` the first P search on this member (AUC then the money curves; best cut -4.46 % 0/6) | rule 3 |
| D1 / D3 | `mt_d1_3Xk2Eu.py` | the creation fingerprint with a prior-only launcher history, and the coin's life shape before the fire (both red; best cell creator history -1.10 % 4/6) | rule 3 |
| D2 / D5 | `mt_d2_3Xk2Eu.py` | the creator's document and off-chain attention; checks sidecar coverage first and finds it is the whole effect (law 30, keep+ep50) | rule 3 |
| 8 exit read | `mt_x_3Xk2Eu.py` | which static rule sells where it sells, on first crossing (none does: best 12.4 %) | rule 3 |
| 5.1-5.2 | `mt_p51_8aaRWu.py` | print 8aaRWu reacts to (WHO / history / priced), leftover at 115 ms | rule 3 |
| overlap / exclusive / 6.1 | `mt_p61_8aaRWu.py` | identity-family overlap, exclusive leftover, which prints it takes (loudness pass; class too wide) | rule 3 |
| 6.1 this printer | `mt_p61b_8aaRWu.py` | leftover on ignored family prints, then this-printer / live-vs-ghost rank (not another loudness cut) | rule 3 |
| 6.1 inside fresh_return | `mt_p61c_8aaRWu.py` | remaining this-printer / returning-printer rank inside `fresh_return` (not AND, not D) | rule 3 |
| 6.1 named builds | `mt_p61d_8aaRWu.py` | which `build` hashes it follows among family / `fresh_return` prints (market or a corner; not AND, not D) | rule 3 |
| 6.1 run-K / silence-then-K | `mt_p61e_8aaRWu.py` | how the print arrives: this structure's run K and coin-silent-then-K (corner / 0.63 % slice; not AND, not D) | rule 3 |
| 6.1 Two lists agree | `mt_p61f_8aaRWu.py` | K buys from >= 2 structures in the slot (K>=2 leftover cover<10 at 1.77 % slice; 2bu K>=1 is 0.50 %; not AND, not D) | rule 3 |
| 6.1 First run here | `mt_p61g_8aaRWu.py` | this structure's first run on this coin (rank 0.501; leftover PASSES at 0.50 % slice; median acted 0; not AND, not D) | rule 3 |
| 6.1 Size buy / Alone | `mt_p61h_8aaRWu.py` | this print's SOL and alone in its slot (ssize rank 0.653 / buy>=1 leftover PASSES at 0.56 % slice; alone rank 0.354 low, leftover PASSES at 0.08 % slice; not AND, not D) | rule 3 |
| 6.2 | `mt_p62_8aaRWu.py` | 6.1 terms one at a time on every coin, leftover, opposite-side control | rule 3 |
| 6.2 acted | `mt_p62_tail.py` | occupancy on the fires it takes (not a sentence) | rule 3 |
| 7.1 | `mt_p71_8aaRWu.py` | identity-family gap: its coins vs other; occupancy on its coins still red so D cannot fix this class | rule 3 |
| 7.3 | `mt_p73_8aaRWu.py` | door search on the wide class (D3/D4); not a freeze of E | rule 3 |
| 5.1-5.2 exclusive | `mt_p51_excl.py` | leftover on exclusive quiet restart / continuation / loud restart | rule 3 |
| lag ladder | `mt_p51_lag.py` | leftover at 10 / 50 / 80 / 115 ms on those exclusive prints | rule 3 |
| anatomy | `mt_d8_anatomy.py` | where 8dtx2t's decision buys land: slot, place in the burst, what printed before | rule 3b |
| 5.1-5.2 wide | `mt_d8_p51wide.py` | 8dtx2t's full class scan, 5.2 at 115 and 83 ms | rule 3b |
| 5.1 / 6.1 / 6.2 | `mt_d8_e.py` | `scan` first positions, `pick` which prints it takes, `spell` every coin, `seats` the ladder | rule 3b |
| 7 | `mt_d8_d.py` | 7.1 split and 7.3 door walk (`walk`); door facts from `mt_d8_door.py`'s running sums | rule 3b |
| 8 | `mt_d8_x.py` | `hazard` its closes, `book` the families, `cut` its own cut walked, `final` on the final pool | rule 3b |
| 9 / 12 | `mt_d8_p.py` | `p` permissions, `rs` re-entry and clip, `gates`, `pfinal` | rule 3b |
| seat audit | `mt_d8_fillcheck.py` | does the fill model price the state our real buys met (their signatures on the lake) | rule 3b |
| 3-4 .. 7.3 all | `mt_d8_all.py` | 8dtx2t's one sentence on all its buys: `p34` its positions, `scan` the class and its leftover, `pick` which prints it takes, `spell` the terms on every coin, `door` 7.1, `door73` 7.3, `exit` 8 (hazard, both families walked), `perm` 9 (study fires stop 09-06 12:00) | rule 3b |
| 12.10-12.11 | `mt_d8_replay.py` | rule 3b on the lake from 09-01, sharing no code with the study: `slim`, `cands`, `repro` (the study's tickets), `book` (one change at a time, every window) | rule 3b |
