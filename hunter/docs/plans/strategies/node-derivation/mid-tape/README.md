# mid-tape: scripts of rule 3

The record behind [../mid-tape-rule-3.md](../mid-tape-rule-3.md) (instrument 8dtx2t). Each script
opens with its step and the question it answers. Playbook: [_!___derive.md](../../_!___derive.md).

The scripts are local scratch and not tracked (only `_paths.py` is): each step's record is its
chain row in the case file named below.

Every script imports `_paths` first. Run from this folder: `python mt_p34.py`. Phase 3-4
loads `study_exact` (every leg, wallet map on the tape). Last-leg `study` does not carry 3Xk2Eu.

| step | script | question | case file |
| --- | --- | --- | --- |
| 3-4 | `mt_p34.py` | shape and who pays, every roster member, every-leg study | rule 3 |
| 5.1-5.2 | `mt_p51.py` | print 8dtx2t reacts to, leftover at 115 ms on that print | rule 3 |
| 5.1-5.2 exclusive | `mt_p51_excl.py` | leftover on exclusive quiet restart / continuation / loud restart | rule 3 |
| lag ladder | `mt_p51_lag.py` | leftover at 10 / 50 / 80 / 115 ms on those exclusive prints | rule 3 |
