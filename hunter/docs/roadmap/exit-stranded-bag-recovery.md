# Stranded-bag RCA — what landed, what's left

Session handoff, 2026-07-28. Branch `strategy-redesign`. **Everything below is
uncommitted.** Companion refs: [../arch/position-lifecycle.md](../arch/position-lifecycle.md)
(section 2.1 is the permanent record of the three invariants),
[../arch/trade-execution.md](../arch/trade-execution.md).

Trigger: 94 real positions over an 8 h window on the deployed box
(`35.158.128.131:5555`) left **5 stranded bags** — bought, still held, sell gave up.
Root cause was not one bug but four independent ones.

## 1. The four causes and their fixes (all landed, none deployed)

| # | Cause | Fix | Files |
| --- | --- | --- | --- |
| 1 | **Token-account attribution.** `buy.rs` cached the funded account in a per-MINT `user_token_accounts` map. Two rules entering one mint in the same slot each drew their own seeded account, both wrote that one key, last writer won — the loser's row recorded its sibling's account and its exit sold from an account holding none of its tokens. | `SnipeBuy { signature, user_token_account }` returned from the buy; the position records the account **its own** buy funded. A retry passes that account back as the buy override. | `shared/executor/pumpfun/src/trader/buy.rs`, `hunter/live/src/strategies/engine/exec_real.rs`, `decision_loop.rs` |
| 2 | **6005 reroute gated on RPC.** `RerouteMigrated` re-confirmed migration via `refresh_curve_facts`; its `Err(_)` fell through to `Fatal`, so an RPC blip stranded a token that was merely migrated and perfectly sellable. | 6005 `BondingCurveComplete` is treated as proof. Loop-local `route_migrated` latch (NOT the token cache — `get_mut` is `None` on an aged-out entry, exactly the long-hold case), durable `is_migrated` written in the background. | `exec_real.rs` |
| 3 | **Durable-nonce sells never expire.** `ExitUnconfirmed` was never auto-re-sold (correct — a blind resend can land twice) but had no path back, so the bag stranded forever. | `Engine::{note_nonce_tx, nonce_tx_state, burn_nonce_tx}` + `NonceTxState` make deadness decidable; `redrive_exit_unconfirmed` re-sells **only** once every recorded sell sig is provably `Dead`. `Unknown` is never treated as dead. | `shared/executor/core/src/nonce.rs`, `engine.rs`, `hunter/live/src/strategies/engine/reapers.rs` |
| 4 | **PG-net is the only bag oracle.** Blind to a feed gap and blind to a bag sitting in an unreferenced account. | Opt-in `EXIT_BAG_ONCHAIN_CHECK` (default **off**, Helius spend): one `getTokenAccountsByOwner` before parking either books a genuinely-cleared row or re-points it at the account really holding the bag and resets the redrive budget. | `reapers.rs`, `strategy_repo.rs`, `hunter/.env{,.example}` |

Supporting changes: `fail_exit()` stashes submitted sell sigs before **every** exit
failure emission (`Position::add_submitted_exit_sigs`, union semantics);
`find_exit_stuck_bags` generalised to `find_bags_by_status(status, threshold_raw)` —
the ONE stranded-bag query; `set_token_account(id, account)`.

### Follow-up that landed after the above

- **`find_reusable_token_account` wired** into `run_entry`. A buy with nothing recorded
  falls back to the account a sibling position on the mint already holds, so the
  template pool stops minting one account per entry. Gated behind a pure in-memory
  `cached_token_account` hit, so a fresh-mint snipe pays **zero** DB round trips; only
  a re-buy into an already-traded mint pays the indexed local query. Best-effort by
  design — a cold cache after restart just draws a fresh account, still correctly
  attributed.
- **`consolidate_token_accounts` made callable.** It was dead for a reason worth
  keeping in mind: it hardcoded the **canonical ATA** as the destination, and *hunter
  never buys into the ATA* — bot and manual buys both go through
  `buy_token_snipe_write_ahead` into a seeded template account. (`buy_token_inner`'s
  ATA branch has exactly one caller in the repo: forge,
  `forge/launcher/src/manage/execute.rs:455`.) Wiring it "as documented" (pre-buy)
  would have moved a live bag somewhere no position references and stranded it.
  Destination is now an explicit `into: Option<Pubkey>`; the create-ATA prefix is
  emitted only when the destination *is* the ATA. Reached via
  `probe consolidate-dryrun <mint> [--into <acct>] [--execute]`; `--execute` refuses
  unless every orphan tx simulated clean.

## 2. Verification state — read this before trusting anything above

| Check | Result |
| --- | --- |
| `cargo check -p hunter-live -p hunter-lab -p forge-live` | clean |
| Tests (hunter-live, hunter-core, executor-core, executor-pumpfun) | 305+ pass, incl. new `curve_sell_6005_reroutes_to_amm_without_an_rpc_reconfirm`, `submitted_exit_sig_tests`, `caller_named_dest_emits_no_create_ata` |
| clippy on touched crates | no new warnings in touched files |
| `cargo build --release -p hunter-live` (Windows) | **FAILED** — inside `ring 0.16.20`'s C build (`lib.exe` exit 1136). Zero errors from any workspace crate. The deploy artifact builds on `rust:1.95-bookworm` in Docker (`deploy/hunter-live/api.Dockerfile`), so this does not gate shipping — **but no release build was ever confirmed locally.** |
| Live-DB SQL smoke | `find_bags_by_status('ExitUnconfirmed', …)` returned exactly `a1a627ef-971b-4922-ada4-172e8291037b` / `57aJfPxk…` / `exit_parked=f` / 97 754 457 434 tokens. `set_token_account` + the exit-sig write executed and rolled back cleanly. |
| **Real-money smoke** | **NOT DONE.** Needs the box, a deploy, and real SOL. |

Falsifiable prediction for the deploy: `57aJ` self-heals within one reaper tick
(~60 s). Its `exit_tx_signatures` is `[]` so the provably-dead gate passes vacuously,
and `tokens_info.is_migrated = t` is durable, so it routes to the AMM on attempt 0.
If it does *not* heal, the nonce-burn gate or the AMM route is wrong — start there.

## 3. Do this first: recover the 5 bags stranded right now

They have been sitting since 2026-07-27. **No deploy is required.**

`POST /api/trading/sell-all-by-mint` — the "Sell All by mint" action in MyWallet
(`hunter/live/src/api/handlers/trading/solana.rs:119-176`) — already enumerates
**every** token account for a mint, sells each using its own address as the override,
re-resolves curve-vs-AMM routing live per pass, and closes each cleared account for
rent. That covers both the orphaned-account case and the missed-migration case. The
reaper's `heal_cleared_by_status` books the rows to `End` once the sells hit the feed.

| Mint (prefix) | Status | Why it stranded |
| --- | --- | --- |
| `57aJfPxk…` | `ExitUnconfirmed`, `exit_parked=f` | cause 2 + 3 — durable `is_migrated` was written ~2.5 min AFTER the exit fired |
| `7cstqrt…` | `ExitStuck`, `exit_parked=t` | cause 1 — sibling positions created 61 us apart, 2 buys / 1 sell |
| `BW7nqMs…` | `ExitStuck`, `exit_parked=t` | cause 1 |
| `7jcj…` | `ExitStuck`, `exit_parked=t` | cause 4 — 2 min ingest blackout 20:11-20:12 |
| `BLJ4…` | `ExitStuck`, `exit_parked=t` | cause 4 |

Note the 4 `ExitStuck` rows are `exit_parked=t`, so `find_bags_by_status` excludes them
(`NOT p.exit_parked`) — the reaper will **not** pick them up on its own even after
deploy. They need a manual Retry (which calls `unpark_exit`) or the sweep above.

## 4. Remaining work, in priority order

1. **Ingest gap-replay (highest value).** 20:11-20:12 came back empty despite
   `ingest.gap_replay_on_reconnect = true`. Unexplained and untouched. Blast radius is
   far wider than the exit path: every strategy decision in that window ran on
   incomplete data, and two tokens were never flagged migrated or dead afterwards
   because their trades vanished. Exit recovery is downstream of this.
2. **Deploy + real-money smoke** of everything in section 1. Set
   `EXIT_BAG_ONCHAIN_CHECK=true` for at least a few days afterwards — it is a handful
   of calls per day and the only thing that can see a bag in an unreferenced account.
   Commit first; nothing here is committed.
3. **Pull `EVENT_LOG_DIR` (or container logs) off the box** to settle the 9
   `EntryFailed` rows. All 9 have zero on-chain buys, so no SOL is at risk and there
   was no silent buy — but the cause is unconfirmed. If they are 6002/6042 slippage
   reverts, that is ~27 landed reverts of burnt fees in 8 h and the buy slippage floor
   is too tight for the current market: a tuning fix, not a code one.
4. **Measure the CU delta of switching the snipe buy to the ATA**, then decide. The ATA
   is the better end-state (one mint = one derivable account, no orphan rent,
   indexer-visible) but it changes the latency-critical instruction mix, and nobody has
   measured it. Cause-1's fix makes attribution correct without it.

## 5. Deliberately NOT done — re-check before "fixing" these again

- **Emitting a re-queue event from the two silent `run_exit` guard returns.** The
  reaper's `redrive_orphaned_exit_pending` already owns that row at 60 s; emitting
  immediately would spin `MAX_EXIT_ATTEMPTS` out in milliseconds. Strictly worse.
- **Consolidation in the reaper's recovery path.** For the bags actually stranded, each
  position's whole bag sits in ONE account and the row merely points at the wrong one —
  `reconcile_bag_onchain` re-pointing recovers 100% of it. A token-transfer tx inside
  unattended recovery is new risk against a split-bag case with no evidence behind it.
- **`exclusive` on the 26 real rules.** Multiple rules per mint is a strategy choice and
  the executor is now safe under it. Turning it on would cut coverage to hide a bug that
  is fixed.
- **Any new Helius spend on a default path.** Cause-4's check and the consolidation
  sender are both opt-in / operator-invoked. Keep it that way (see the standing rule in
  `CLAUDE.md`).

## 6. Non-defects confirmed — do not re-investigate

- All 13 `End`/`Dead` rows carry real `exit_lamports`. `Dead` is a **rule name**, not a
  write-off.
- All 9 `EntryFailed` rows have zero on-chain buys. No silent buy, no SOL at risk.
  (Their *cause* is still open — item 3 above.)
