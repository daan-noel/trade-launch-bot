# 2026-08-25 — four hours of `ix_labels` with no structural markers

## What happened

Between roughly 18:00 and 22:00 UTC on 2026-08-25 the labeler emitted
`System Program: Unknown` for essentially every System Program instruction, and
the machinery markers went to zero. Hourly, from `trades.ix_labels`:

| hour UTC | `System: Unknown` | `System: Transfer` | `CreateAccountWithSeed` | `AdvanceNonceAccount` | `ATA: CreateIdempotent` |
| --- | --- | --- | --- | --- | --- |
| 17:00 | 0 | 92,813 | 4,295 | 18,296 | 50,707 |
| 18:00 | 125,234 | 13,409 | 499 | 2,417 | 7,710 |
| 19:00 | 157,453 | 0 | 0 | 0 | 7 |
| 20:00 | 132,178 | 5 | 0 | 0 | 3 |
| 21:00 | 75,552 | 4 | 0 | 0 | 3 |
| 22:00 | 0 | 7,477 | 288 | 1,151 | 4,952 |

490,590 System instructions and 61,621 Token instructions landed unnamed that
day; every other day in the window has zero. pump.fun and Compute Budget labels
were unaffected throughout.

## Why

The frames were jsonParsed. A `{program, parsed}` instruction carries no raw
`data` — the RPC node consumed the bytes — so `convert::data_from_parsed`
re-encodes them. For those four hours the rebuild returned `None` for
system / spl-token / ATA instructions, and `compiled_parts` fell back to empty
data. An instruction with empty data has no discriminator, so `label_instruction`
had nothing to name it with.

The split in what that produced matters:

- **System and Token degraded to `Unknown`** — visibly wrong, if anyone looked.
- **ATA degraded to a *wrong* label.** `Associated Token: Create` is the legal
  encoding for a zero-length payload (the pre-1.0.5 form), so 168,230 instructions
  booked as `Create` when the traffic is overwhelmingly `CreateIdempotent`. That
  branch is not a bug — it is right on the gRPC path — but on the jsonParsed path
  an empty payload only ever means the rebuild failed.

## Why it matters more than a decode gap

`m_flow_ix`'s machinery markers (`CreateAccountWithSeed`, `AdvanceNonceAccount`,
`System Program: Transfer`) are matched as substrings of these labels. With the
labels gone, **every build in that window reads as clean organic flow**. This is
not noise: it is a one-directional bias toward "human" on exactly the axis the
8dtx cleanliness gate scores. Anything measured across 2026-08-25 18:00–22:00 UTC
is contaminated on the marker axis and cannot be repaired — `raw_txs` is not
persisted, so the tape cannot be re-labelled after the fact.

## What changed

`convert::note_unrebuilt_parsed_ix` counts every parsed instruction that cannot be
rebuilt and warns (throttled to once per 30 s, with a running total). Silence was
the whole failure: the tape said nothing while it lost its markers for four hours.
`unrebuilt_parsed_ix_count` exposes the counter so the alarm is testable without
the log sink.

The labeler also stopped saying `Unknown` for anything except this — a named
program with an unrecognised instruction now renders `ix#<key>`. `Unknown` in an
`ix_labels` entry today means one specific thing: the feed delivered no
instruction data.

## What to check if it recurs

1. `jsonParsed instruction data could not be rebuilt` in the ingest logs — the
   `program` and `parsed_type` fields name what the rebuild is missing.
2. Whether the publisher changed encoding: `base64` frames carry raw data and
   never take this path.
3. Whether a new `parsed.type` has appeared that `system_data` / `token_data` /
   `ata_data` do not cover — a real, fixable gap rather than a feed fault. A live
   sample of 60 pump.fun transactions on 2026-08-28 showed zero uncovered types,
   so the four-hour window was a feed fault, not a missing arm.
