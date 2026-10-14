# Owner split builder: build the volume tag once, in the repo

The `volume` tag is built once, offline, and a running rule only reads it
([_!___owner-split.md](../plans/strategies/_!___owner-split.md), "At a glance"). The stored
5ix, 7ix and 3ix tags come from a script that is not tracked, so a tag cannot be rebuilt,
audited or tested. This is the lab command that replaces it.

## What it does

One lab command, workstation only (DuckDB over the lake; never on the EC2 box):

1. Read the launch group's coins and every market trade of the build window from the lake.
2. Run Mark, Grow and Check over them, as `_!___owner-split.md` sections 1-3 state them. The
   rows under "Not in the build" are not run.
3. Write `tags.volume` on one fingerprint, replacing it whole:
   - `match.ix_shape`: ix structure rows (`level` exact) and ix core rows (`level: "core"`),
     each passing its match level at 90 % or more owner SOL on the group with 10 or more
     trades, reduced to the fewest rows that cover every passing trade (biggest first).
   - `match.wallet`: wallets that come back only - same slot after 20 coins, below zero on
     2 coins, never sells (10 coins, 20 buys, no sell), stays in group (3 coins, 80 %).
     An address is kept when its latest trade on the group is inside the recent window
     (the gap that covers 90 % of returns, counted up to the build's last coin).
   - `match.creator: true`, `sticky: true`.
   - Nothing else: no `program`, `ix_template`, `ix_contains` or `ix_lacks` on `volume`.
   - A hand edit made since the last build (a chart click, the Flow Discovery cart) is
     dropped with the rest of the old tag.

## Constraints the build must keep

| constraint | why |
| --- | --- |
| Exclude `wallet_dict.is_proxy` from `match.wallet` | a proxy is the venue's routing account; storing it marks every customer who shares the address ([arch/database.md](../arch/database.md)) |
| No birth-bundle wallet in `match.wallet` | it is new on the next coin, so it only adds rows to a static list; the creation-slot rows cover it while the build labels |
| No this-coin wallet in `match.wallet` | only wallets that come back are stored |
| Assert `hunter/engine/fixtures/ix_core_parity.json` | the builder's core and the engine's `core_labels` must reduce every list the same way, or a core row matches nothing live |
| Write rows the engine parses | every row round-trips through `IxShapeRows` (`hunter/engine/src/metrics/fee.rs`) before the write |

## In-sample stamp

A backtest on coins inside the build window is in-sample. The build stamps the window on
the tag, and simulate flags fired coins born inside it. `parse_tag` rejects unknown keys
today, so the stamp needs one accepted, non-matching key (`built: {from, to}`) in
`hunter/engine/src/metrics/tags/config.rs` and its TS twin in `lib/strategy/tagsDoc.ts`.

## Done when

- The command rebuilds `5ix owner split`, `7ix` and `3ix 0.54 owner split` and each new tag
  agrees with its stored one within the Appendix B numbers of `_!___owner-split.md`
  (rows carry the same owner SOL share, outsider SOL caught within half a point).
- Simulate marks in-sample coins on a stamped fingerprint.
