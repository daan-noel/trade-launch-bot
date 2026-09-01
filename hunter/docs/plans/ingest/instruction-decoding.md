# Instruction decoding — how an `ix_labels` entry gets its name

Every entry in `trades.ix_labels` is `"<program>: <instruction>"`. The two halves
are resolved independently, because they have different evidence behind them and
different failure modes. Knowing a program ran `SellBondingCurvePercentage` is
useful whether or not anyone knows who owns the program.

Owning code: [`shared/ingest/pumpfun/src/decode/program_registry.rs`](../../../../shared/ingest/pumpfun/src/decode/program_registry.rs)
and [`instructions.rs`](../../../../shared/ingest/pumpfun/src/decode/instructions.rs).
Harvest commands: [`hunter/live/src/diagnostics.rs`](../../../live/src/diagnostics.rs).

## The three tiers, in descending order of proof

**1. Computed Anchor names (`ANCHOR_IX`).** The table stores the instruction
*name*; the 8-byte discriminator is computed as `sha256("global:<snake_name>")[..8]`.
A wrong name therefore matches nothing on chain and the label degrades to a key.
This tier cannot emit a wrong name, which is why it holds almost everything.

**2. Explicit keys (`EXPLICIT_IX`).** A few programs log an instruction name but
dispatch on something that is not that hash. Their key bytes are written down, so
this tier *can* carry a transcription error — it stays short and reviewed, and
`decode-harvest` re-derives every row from chain.

**3. Key only.** Everything else renders `ix#<key>`. This is an identity, not a
name. It is what separates a router's buy from its sell without claiming to know
what either is called.

`Unknown` survives in exactly one case: the feed delivered no instruction data.
That is a fact worth keeping — see the blackout below.

## Key width is a cardinality decision

`IxKey` picks how many leading bytes identify an instruction:

| width | when | why |
| --- | --- | --- |
| `Disc8` | 8-byte dispatch value carrying no arguments (Anchor and Anchor-shaped) | the whole value is identity |
| `Tag1` | one dispatch byte followed by arguments | reading further folds a `u64` amount into the key |

`Tag1` is the default for a program with no table, so an unknown program is
bounded at 256 labels. The cost of getting this wrong is not cosmetic: an
argument read into the key forks one instruction into thousands of labels, which
makes `ix_hash` unique per trade and dissolves every fingerprint grouping built
on it. Axiom is the worked example — `01` + `u64` + `u8`, two real instructions,
103 distinct 8-byte prefixes.

Memo is excluded for the same reason. A memo's data *is* its text, and the text
is per-transaction unique often enough to do that damage, so memos label as
`Memo Program: Memo` and `decode-harvest` reports the payloads in aggregate.

## Where names come from: the log-and-verify loop

An Anchor program logs `Program log: Instruction: <Name>` on every invoke. Pair
that line with the discriminator of the instruction that produced it and the pair
is **checkable**: recompute `sha256("global:<snake(Name)>")[..8]` and it must
equal what was observed. That is proof rather than a lookup, and it works on
programs that publish no IDL and appear nowhere on the web — which is most retail
routers.

```powershell
cargo run -p hunter-live -- unknown-programs                 # rank what has no name
cargo run -p hunter-live -- decode-harvest --top 20          # go find the names
cargo run -p hunter-live -- decode-harvest --program <ID>    # one program, no DB
```

`decode-harvest` costs one `getTransactionsForAddress` per program — not one call
per transaction. It prints paste-ready rows, splits verified pairs (tier 1) from
unverified ones (tier 2), reports the key cardinalities behind its width choice,
and lists the keys it could not name at all.

Only top-level invocations (`invoke [1]`) are paired. A CPI into the same program
logs a name too, and pairing that with a top-level instruction shifts every later
pair by one.

## When the feed delivers no data — and how loud that should be

A jsonParsed frame carries `parsed` instead of `data` for the programs the RPC
node knows (`system`, `spl-token`, `spl-token-2022`, ATA), so
[`convert::data_from_parsed`](../../../../shared/ingest/core/src/convert.rs)
re-encodes those bytes before anything labels them. An instruction it cannot cover
keeps empty data, has no discriminator, and renders `Unknown` — the one case that
word survives.

Rebuilds are **byte-exact or nothing**. An arm is added only where the parsed view
carries every argument the program serialises: `getAccountDataSize` rebuilds as the
bare tag `21`, but a Token-2022 frame listing `extensionTypes` does not, because
the parsed view spells those extensions by name and the tag mapping would have to
be invented. A short or guessed payload is worse than an empty one — downstream it
is indistinguishable from a real instruction.

**A label is only ever built from a top-level instruction.** `decode::protobuf`
walks `message.instructions`; inner (CPI) instructions are never labeled. The
rebuild runs on both, so the unrebuilt-instruction alarm reports two numbers and
only one of them means the tape is degrading:

| counter | meaning |
| --- | --- |
| `total` | every rebuild miss, inner included. Dominated by CPI: `spl-token: getAccountDataSize` alone fires ~800k/day as an ATA-creation CPI and reaches ~0 labels. |
| `top_level` | the misses that cost a label. **This is the blackout number.** |

A large `total` beside a `top_level` of 0 is an uncovered CPI type — worth an arm
in `data_from_parsed` to quiet the log, but no marker is lost and no window is
contaminated. A moving `top_level` is the 2026-08-25 failure recurring.

Naming an instruction the rebuild misses means **adding the arm in
`data_from_parsed`**, not passing a type into `label_instruction`: that function's
`parsed_type` fallback is `None` on every production path, because the
jsonParsed→protobuf conversion keeps `data` and drops `parsed`.

## What the loop cannot reach

A program that suppresses its instruction logs and publishes no IDL yields keys
and nothing else. Its *program* name may still be unknowable; that is acceptable
and the labeler is built for it. What is not acceptable is guessing: a vanity
prefix is a hint, the instruction set a program runs is evidence.

## Related

- Labeling in the ingest pipeline: [arch/ingest.md](../../arch/ingest.md)
- The instruction markers this vocabulary feeds: `hunter/engine/src/metrics/flow_ix.rs`
- The failure that made the `Unknown`-means-no-data rule load-bearing:
  [docs/history/2026-08-25-ix-label-blackout.md](../../../../docs/history/2026-08-25-ix-label-blackout.md)
