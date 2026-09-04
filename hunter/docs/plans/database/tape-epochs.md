# Tape epochs — what the stored rows carry, and from when

Every forward-only migration splits [`trades`](trades-storage.md) /
[`tokens`](token-storage.md) into a before and an after. This is the register of
those splits: where each boundary sits, how it is pinned, and which reads have to
respect it.

The boundaries live in code as
[`storage::tape_epochs`](../../../core/src/storage/tape_epochs.rs) — the one
definition, with the guard that enforces the only silent one. This page is the
survey that constant cannot carry: the shape of the local store around it.

## The boundaries

Instants are UTC, on `trades.block_time` unless noted.

| From | The tape gains | Pinned by |
| --- | --- | --- |
| `2026-08-18 10:47:26.684563` | `tokens.meta->>'uri'` (on `tokens.created_at`) | first non-empty `meta` |
| `2026-08-23 00:00:00.017453` | `trades.fee_lamports` | first non-null |
| `2026-08-30 17:48:13.387180` | **`ix_labels` vocabulary v2** + `cu_limit`, `cu_price`, `tip_lamports` | last v1 label `17:48:10.201701`, first `ix#` key `17:48:13.387180` |
| `2026-09-01 16:24:13.860129` | `payer_id`, `is_proxied`, and the jsonParsed rebuild arms | first non-null |

**A boundary is an ingest RESTART, not a commit.** The labelling rewrite is
committed 08-28 and reaches the tape 08-30, because that is when the binary
carrying it comes up. Read the boundary off the data; a commit date is a guess.

**Two of these are one deploy.** The 08-30 restart delivers the vocabulary change
and the fee-budget columns together, so a row with a v2 label and a NULL
`cu_limit` means the transaction set no compute budget — never that the row is old.

## Which splits announce themselves, and which does not

A new column is NULL on the old side, so a reader that wants it gets nothing and
knows it. `ix_labels` is the exception: its spelling changes without its type
changing, so an old label is a well-formed string that means something else. The
mechanism, the spellings, and the guard
([`ix_vocabulary_for_window`](../../../core/src/storage/tape_epochs.rs)) are
documented at the module; call it before grouping, hashing, or exact-matching
labels over a range. Label *construction* is
[instruction-decoding.md](../ingest/instruction-decoding.md).

## Nothing here backfills

[`raw_txs`](raw-txs-storage.md) is opt-in with 3-day retention and holds no
payload for any of these windows, so an old row can never be re-decoded. Every
boundary is one-way and permanent, and a study that needs a field simply starts
at that field's instant.

The one exception is `wallet_dict.is_proxy`, which is derived from the address
bytes rather than from an observation (core migration `0015`) and therefore
applies to rows whose transactions are long gone. Router attribution is the only
correction that reaches history; use it, not `is_proxied`, when the window opens
before `2026-09-01`.

## What the lake carries

The Parquet lake ([read paths](lake-pg-read-paths.md)) has its own split, on
partition rather than on instant:

| Partitions | Columns |
| --- | --- |
| `dt=2026-08-02` .. `dt=2026-08-22` | 15 — no `cu_limit` / `cu_price` / `tip_lamports` |
| `dt=2026-08-23` onward | 18 |

A scan crossing that line needs `union_by_name`, or it reads the narrow schema and
drops three columns without saying so. The exporter emits neither `fee_lamports`
nor `payer_id` / `is_proxied` on trades, and no `uri` on tokens, so those four
fields are PG-only regardless of partition.
