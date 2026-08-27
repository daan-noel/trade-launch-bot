# 2026-08-27 — fingerprints duplicated by an inert bucket width

125 fingerprints, many of them the same match stored several times under several
names. Root cause was one uncanonicalised field; a second finding fell out of
checking the premise for the merge.

## `init=0 · bkt=1000` is NOT the wildcard

The working assumption was that `init_buy_lamports = 0` at a 1000 SOL bucket width
matches every token, since creation amounts are far below 1000 — and so the 33
rules on it could be merged into a `wildcard` row.

The bucket half held: max `initial_buy_lamports` across 927,708 tokens was 85.005
SOL, so every token lands in bucket 0 and the axis never discriminates. The other
half did not. `sol_axis` requires the token to *have* a value:

```rust
tf_lamports.is_some_and(|v| match precision { ... })
```

**170,550 of 927,708 tokens (18.4%) carried a NULL `initial_buy_lamports`**, running
at 15–18% per day at the time (08-26: 3,343 of 22,014) — not a historical gap. A
NULL fails a configured axis and passes a wildcard, so the row means *every token
whose dev-buy we parsed*, and the merge would have widened 33 rules by about one
token in six, two of them live (`isl-ab-confirmed`, `isl-b-quiet-pause`).

The merge was made anyway, deliberately: the gap is our own parser coverage, not a
creation shape, so the rows mean "every token". `0007` flips all 11 of them to
`wildcard` (33 rules + the 10 `8dtx · <router>` carriers), widening them by that
18.4%.

Scope is the **spanning** bucket only. The other `init=0` rows (`bkt=1.6`, `5`,
`6.4`) bound a real band well inside the 85 SOL range. So does
`fs_buy=500 · bkt=1000`, for a second reason: a first-slot axis is judged at
`MatchPhase::Full`, so making it a wildcard would also move it from arming after
the creation slot closes to arming at `TokenCreated` — a change in *when*, not
just what.

**Read a "matches everything" axis as "everything the axis has a value for", and
price that difference before treating it as a wildcard.** The NULL rate is
invisible from the fingerprint row itself.

## The duplicates: an inert width forked identity and the name

`bucket_size_amount` reaches a match only through the five bucket-matched SOL
axes. 14 rows configured none of them, so their width changed nothing — but both
readers of the column spent it anyway: `IDENTITY_WHERE` keyed on it, so
`find_or_create` minted a new row instead of reusing one the engine matches
identically, and `auto_name` printed it, so one match carried several names.
`3ix:Buy` and `3ix:Buy · bkt=1000` were one fingerprint stored twice.

Fixed by `Fingerprint::effective_bucket_size_amount` (the width, or `None` with no
SOL axis) at every write edge, plus the `0006` CHECK. Migration `0006`
canonicalised the rows and merged 15 into 5 — **125 → 115 rows, all 197 rules
preserved, 0 orphans**.

Two related defects went with it: a width below 1e-4 rendered as `bkt=0` under the
old fixed-4-decimal name (the one width `validate` rejects, on a legal row), and
the chip row / search text / `fingerprintIdentityKey` all spent the inert width too,
so identical rows sorted apart.

## What `metric_config` still blocks

Even as wildcards the eleven rows stay eleven: none is deleted and no rule moves,
because every pair that became match-identical carries a different
`metric_config`.



Ten `8dtx · <router>` rows are match-identical and carry ten
different `metric_config` payloads (804–27,641 bytes of
`m_flow_split.volume_ix_patterns`). `find_or_create` ignores `metric_config` for
identity but the engine does not, so merging them would have erased the router
split. The migration merges only within groups whose `metric_config` is identical.

Those rows use the fingerprint as a carrier for per-rule metric config, wearing an
inert axis to get a distinct id. Still open: `metric_config` on a shared,
non-identity row is per-rule state stored per-fingerprint.

## Names

`auto_name` is pure in the axes but its output is stored, so changing it strands
the copies already written — which is how names drifted apart in the first place.
`is_generated_auto_name` now decides "generated, never typed" by grammar, so a
change to `auto_name` heals itself on the next `list`. 88 of 115 rows now carry the
generated name; the 27 that differ are nicknames holding derivation evidence the
axes cannot state (`probe group mc0.0108 (held +17.13pc 9of9)`) and are never
rewritten.

## Both boxes, and why the wildcard id is pinned

The live box was at core migration `4` — it had never run `0005`, so it had no
`wildcard` column at all. It also holds a different row set: 18 fingerprints / 30
rules against the lab's 115 / 197, because the lab accumulates research rows the
server never sees.

That matters because `0006` and `0007` each pick a winner from what a given
database happens to hold, and the two boxes landed on different ids for the same
match — the lab on `793c5b87`, the live box on its own `isl-ALL broad`.
`db-incremental-sync.ps1` mirrors `fingerprints` and `strategy_rules` server-wins
**by primary key**, so two ids for one match means the next sync re-creates the
duplicate the merge just removed and moves rules back onto it. `0008` pins the id
instead of deriving it: every axis-free, `{}`-config wildcard collapses onto
`793c5b87` whichever box runs it.

Applied to live 08-27 over the direct DB port: **18 → 16 fingerprints, 30 rules
preserved, 0 orphans, 1 wildcard.** All six active *real* rules sit on untouched
`max=…` fingerprints; the only widened rules are the two paper island ones. The
ledger was deliberately left untouched — every statement in `0005`-`0008` re-runs as
a no-op (verified), so the redeploy applies them properly and records the rows with
sqlx's own checksums.

The sync script itself had never learned about `wildcard`: its fingerprint upsert
predates `0005`, so a server wildcard row would have landed on the lab as an
axis-free row with `wildcard` defaulted `FALSE` — which the matcher reads as
*matches nothing*, the opposite of the row it copied. Fixed in the same pass.
