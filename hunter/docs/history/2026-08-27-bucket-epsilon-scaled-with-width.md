# 2026-08-27 — the retired bucket epsilon scaled with the width

Found while diffing the matched-token set across the fingerprint range migration
(`core/migrations/0009_fingerprint_criteria_ranges.sql`). It is recorded because it
changes how to read every result measured through a **wide** bucket before this date.

## What was wrong

The retired `grouping::bucket_index` was:

```rust
((v / width) + BUCKET_EPS).floor() as i64   // BUCKET_EPS = 1e-9
```

The epsilon absorbed float noise so an on-edge value landed in the upper bucket. But
it was added in **ratio** units, so what it was worth in lamports scaled with the
width:

| width (SOL) | epsilon worth |
| --- | --- |
| 0.1 | 0.1 lamport |
| 5 | **5 lamports** |
| 1000 | **1000 lamports** |

Its own doc argued the nudge was "far below the 1-lamport quantum of any real SOL
amount" — true at 0.1, and the widths in the library reached 1000.

So any value within `width × 1e-9` lamports **below** a bucket's top edge was filed
into the **next** bucket. A `first_slot_buy` of 4.999999999 SOL, at width 5, read as
bucket 1 rather than bucket 0 — outside the `[0, 5)` window it belongs to.

## Blast radius

Small, and one-directional. Over a 3-day window, 339,700 fingerprint×token match rows:
**3 rows** differed, on 2 of 115 fingerprints — both `8dtx-clone: creation bundle`
rows, at widths 5 and 10. Nothing was lost; the three are tokens the retired matcher
wrongly excluded.

The error only bites within a few lamports of an edge, so it is invisible except where
a fingerprint's window edge happens to sit on a value the tape actually produces. It
did not affect the 0.1-width rows, which are most of the library.

## Why it cannot recur

Identity is integer now. A fingerprint axis is an inclusive `[min, max]` over `u128`,
so a bound either contains a value or does not — there is no division, no ratio, and
no epsilon. The concept the defect lived in is gone, not guarded.

Read against this: any measurement taken through a bucket width of **1 SOL or more**
before 2026-08-27 could include or exclude tokens within `width × 1e-9` lamports of an
edge. <!-- pt-ok: cutoff for re-reading stored results -->
