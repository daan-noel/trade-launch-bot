# Every trailing exit in the island-search extract was a hard stop (2026-08-21)

**Symptom.** The decision-point extract's exit walk reports `trail = 0.0%` of exits for a
7% trailing stop, and the armed 18% trail never fires once in 4.88 M decision points. The
number is not implausible enough to stop anyone: a −1% stop does beat a 7% trail whenever
the in-hold peak stays under +6.45%, so the zero gets explained away rather than checked.

**Cause.** One line in `walk_exits`:

```python
np.maximum(peak[active], yj, out=peak[active])   # intended: update the running peak
```

`peak[active]` with an integer-array index returns a **copy**, not a view, so `out=` writes
into a temporary that is discarded. The running peak never updates — it stays pinned at the
entry fill for the whole hold. Therefore

```
retrace = (1 - exp(y - peak)) * 100  ==  -pnl
```

and **every trailing exit silently degenerates into a hard stop from entry**:

| shape | authored as | what actually ran |
| --- | --- | --- |
| `inc` trail 7 | 7% off the in-hold peak | a −7% hard stop, always beaten by the −1% stop ⇒ never fires |
| `w25` trail 25 | 25% off the peak | a −25% hard stop (its "trail" exits are stop-outs mislabelled) |
| `arm18` trail 18 armed +10 | armed trail | `-pnl >= 18 AND pnl >= 10` — arithmetically impossible |

NumPy raises nothing. `np.maximum(a[idx], b, out=a[idx])` is a silent no-op; the slice form
`a[i:j]` is a view and does work, which is why the idiom looks right.

**Fix.** Explicit read-modify-write: `pk = np.maximum(peak[active], yj); peak[active] = pk`.

## What the bug was hiding

Same entry, same fills, same costs, 7 cohort days, `FirstInWindow`, B = 0.05 SOL:

| exit | buggy | correct | days + |
| --- | ---: | ---: | ---: |
| `SL 1` + trail 7 + 450 s | +20.35 | **+43.21** | 7/7 |
| trail 25, 600 s | −13.98 | +10.13 | 5/7 |
| `SL −35` + trail 18 armed +10, 600 s | −19.09 | −2.87 | 1/7 |

The corrected trail is 38.7% of exits and doubles the book. The dead-exit rate falls
2.5% → 0.9%, because a working trail leaves before the pool goes quiet.

## What it voided

[`2026-08-21-island-search-refuted.md`](2026-08-21-island-search-refuted.md) carries the
headline *"the exit shape dominates the entry — fixed bracket +1.14%/episode, armed 18%
trail −45.8%, wide 25% trail −38.7%; a trail holds a token into its own death."* That
compared a bracket against **two hard stops wearing trail labels**, with dead booked at
−100% on top. Both errors push the same way, against trailing exits. Across a corrected
exit family the spread is ~25% of total SOL, not 40–50 pp, and the sign reverses: on this
tape a working trail exits *before* death rather than into it.

The refutation's entry-side conclusion is unaffected — it was measured under reference
exits that were wrong in a way that hurts every region equally.

## The rule this produced

**Hand-check every exit shape on a handful of positions before quoting a number derived
from it,** and treat an implausible exit-mix cell (`trail = 0.0%` on a trailing stop) as a
defect until proven otherwise. The instrument gate already existed —
[`@plans/strategies/island-search.md`](../plans/strategies/island-search.md) — and was
skipped because the derived column looked authoritative.
