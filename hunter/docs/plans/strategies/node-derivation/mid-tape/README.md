# mid-tape: scripts of rule 3

The record behind [../mid-tape-rule-3.md](../mid-tape-rule-3.md) (instruments 8dtx2t, 88887Q, 3Xk2Eu, 8aaRWu).
Each script opens with its step and the question it answers. Method:
[_!___derive.md](../../_!___derive.md).

A study script is one-time: written for one test on the toolkit, and deleted once its numbers are a chain row ([derive](../../_!___derive.md) section 13).

Every script imports `_paths` first. Run from this folder: `python hr_synth.py`. Kept here:

| script | what it does | re-run when |
| --- | --- | --- |
| `hr_prep.py` | the curve tape 09-01 .. 09-06, every leg, from the lake (`data/hr_tape.parquet`) | the proof below needs its tape |
| `hr_core.py` | that tape in memory and the facts the proof's rules read | - |
| `hr_synth.py` | the proof of `toolkit/hitrate.py`: a fake trader with known rules, measured (evidence 5.8) | any change to `hitrate` |
| `hr_hand.py` | the same chances and hits counted print by print, sharing no code with `hitrate` | any change to `hitrate` |
| `mt_d8_replay.py` | rule 3b on the lake, sharing no code with the study | [backtest-audit.md](../../backtest-audit.md) V1 |
| `mt_d8_fillcheck.py` | whether the fill model prices the state our real buys met | [backtest-audit.md](../../backtest-audit.md) F2 |
