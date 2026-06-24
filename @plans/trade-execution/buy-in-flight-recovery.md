# Buy-in-flight recovery

How the buy path survives a crash without ever buying twice.
Code maps: [@docs/strategies.md](../../@docs/strategies.md),
[@docs/trade-execution.md](../../@docs/trade-execution.md).

## The problem

A buy isn't instant. There's a 2–5 s gap between **sending** the buy and **recording**
the fill (we wait for it on the trade feed). If the process dies in that gap:

- the in-memory buy task is gone — and the signature dies with it,
- the buy **still lands on-chain** — the wallet now holds tokens,
- on reboot the bot sees an unentered row, assumes "never bought," and deletes it.

→ Real tokens sit in the wallet, tracked by nothing, never sold.

## The one rule that constrains everything

**Never re-send a buy to "recover" it.** The buy is signed against a *durable nonce*, so
it can still land minutes later — even after a restart. Re-sending would buy twice. So
recovery may only **adopt** (the buy landed → record it), **wait** (might still land), or
**drop** (it provably reverted → bought nothing). Re-sending only ever happens on a
*proven on-chain revert*.

## The fix, in three layers

**1. A durable "buy in flight" marker.**
Split the old `PendingEntry` into:

- `Arming` — matched a rule, no buy sent yet → safe to reap if it goes stale.
- `BuySubmitted` — buy sent, tokens may exist → never reaped, owned by a recovery reaper.

Each submitted signature is saved on the row (`submitted_buy_signatures`). On boot and on
a timer, `redrive_orphaned_buy_submitted` checks those signatures and adopts / waits /
drops accordingly. An `EntryGuard` (like the sell side's `ExitGuard`) stops the reaper and
a live buy from both acting on the same position.

**2. Write down the signature *before* sending.**
Because the durable-nonce signature is known the instant we sign — before the network — we
persist the marker first, then submit:

```text
sign → save signature (BuySubmitted) → submit → record fill
```

So a crash anywhere after signing is recoverable, and a crash before submit means nothing
went out. Implemented with `buy_token_snipe_write_ahead` + a `BuySignedHook` in
`pump-trader` that fires with the signature right before submit. Even if the submit call
*reports* an error, the signed tx is treated as in-flight (it can still land) — never
re-sent.

**3. A boot wallet sweep (backstop).**
For anything the marker can't see — a manual transfer, a failed write, a future bug —
`wallet_reconcile` lists the wallet's token accounts once at boot and logs any balance that
no open position (across both strategy clones) accounts for. Read-only: it flags for manual
review, never auto-sells.

## Why this shape

It reuses two patterns the codebase already trusts — `ExitPending` recovery and
per-signature attribution — so it's the smallest change that fully closes the gap. The
sign-before-submit ordering makes it airtight, and the durable-nonce rule keeps it from
ever double-buying. Both clones (`tpsl_sniper_1/2`) carry identical changes.
