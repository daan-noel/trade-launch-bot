# Fresh-wallet pool — phases & tasks

Plan for managing many single-use "fresh" wallets (dev/creator + bundler) used by
the token launch flow. Feeds into [`roadmap-plan.md`](roadmap-plan.md) Phase 5+
("wallet obfuscation"); ADRs go in [`decisions.md`](decisions.md).

**Scope decided:** single-use wallets (burned after one launch), manual SOL
funding (no automated hop-graph yet), hundreds of wallets / several concurrent
launches per day, local encrypted keystore (single machine, no remote signer yet).

---

## Phase map

| Phase | Scope | Status |
| --- | --- | --- |
| **1 — Wallet lifecycle & storage** | Status states, atomic claim, balance-driven funding detection | Done |
| **2 — Wallet Management page** | Frontend: generate, view pool, low-pool alert | Done |
| **3 — Token Launch integration** | Metadata + creator/bundler wallet selection wired into launch flow | Done |
| **4 — Cleanup & backup infra** | Dust sweep, encrypted-store backup, restore runbook | Done |
| **5+ — Deferred** | Automated multi-hop funding, instruction/CU/slippage fingerprint picker | Explicitly deferred by user, not now |

---

## Phase 1 — Wallet lifecycle & storage ✅

**Goal:** wallets move through explicit, race-safe states instead of a bare
active/inactive flag.

- [x] Migration: wallet `status` — `generated` → `funded` → `reserved` → `used` →
  `retired` (`migrations/0004_wallet_pool.sql`; replaces `managed_wallets.is_active`)
- [x] Migration: `funding_source` (text, where the manual SOL transfer came from)
- [x] Migration: `reserved_by_launch_id` + `reserved_at` (for TTL release)
- [x] Batch key generation (server-side): `launcher::generate_wallets` — creates N
  ed25519 keypairs, envelope-encrypts each via the existing keystore
  (`launcher::keystore`), inserts as `generated`
- [x] Balance poller: `launcher::spawn_balance_poller` — background task batches
  `getMultipleAccounts` for `generated` wallets, auto-promotes to `funded` once
  balance clears a floor (no manual "mark funded" toggle — avoids bookkeeping
  drift); also records `balance_lamports`/`balance_checked_at`
- [x] Atomic claim query: `ManagedWalletRepo::claim_funded` —
  `SELECT ... FOR UPDATE SKIP LOCKED` picks N random `funded` wallets, stamps them
  `reserved` + launch id in one statement
- [x] Reservation TTL sweep: `launcher::spawn_reservation_sweep` — background task
  releases `reserved` → `funded` after a 15-minute TTL if a launch never confirmed
  (aborted launch shouldn't strand wallets forever)
- [x] Mark-used transition (`ManagedWalletRepo::mark_used`, `reserved` → `used`,
  terminal — never re-selectable) wired into `launcher::service::execute_launch`
  (dev wallet, on create success) and `launcher::bundle_execute::execute_bundle`
  (bundler leg wallets, on Jito submit success). Currently a safe no-op in
  practice — nothing claims wallets via `claim_funded` yet, so no wallet reaches
  `reserved` until Phase 3 wires pool claiming into wallet *selection*; Phase 3
  also moves the bundle-wallet transition to the confirm watcher's
  landed/dropped/partial outcomes per its own checklist below.

## Phase 2 — Wallet Management page ✅

**Goal:** operate the pool without touching the DB directly.

- [x] List view: address, label, role (dev/bundler), status, balance, age
  (`frontend-launch/src/WalletPool.tsx`, backed by `GET /api/wallet_pool` →
  `ManagedWalletRepo::list_all`, a role filter, no separate pagination yet —
  fine at "hundreds of wallets" scale)
- [x] "Generate N wallets" action (calls Phase 1 batch generation) —
  `POST /api/wallet_pool/generate` → `launcher::generate_wallets`
- [x] Status counts summary (generated / funded / reserved / used / retired) —
  computed client-side from the one pool fetch, not a separate aggregate
  endpoint (avoids a second query that could drift from the list)
- [x] Low-pool banner: warns when `funded` count for a role drops below
  `LOW_POOL_THRESHOLD` (3, in `WalletPool.tsx`) — manual funding won't refill
  itself
- [x] Never expose private keys or `key_ref` contents to the frontend at any
  point — enforced at the model level (`ManagedWallet.key_ref` is
  `#[serde(skip_serializing)]`), and the frontend's `ManagedWalletPool`
  TS interface has no `key_ref` field at all

No router added — the launch console (`frontend-launch/src/App.tsx`) gained a
simple `view` tab-state switcher between "Launch Console" and "Wallet Pool"
rather than pulling in a routing dependency for a two-view app.

## Phase 3 — Token Launch page integration ✅

**Goal:** launch flow consumes the pool instead of a free-form wallet picker.

- [x] Metadata editing panel (name/symbol/uri) — `App.tsx`'s Launch card gained
  editable name/symbol/metadata-URI fields, pre-filled from the selected
  template but sent as per-launch overrides (`LaunchRequest.name/symbol/uri` in
  `service.rs`; the template row itself is never mutated). **Scope note:** no
  image upload/hosting was built — pump.fun's on-chain create only takes
  `name`/`symbol`/`uri` (the URI already points at an off-chain JSON with
  description/image), and this repo has no existing pinning/upload
  infrastructure to build that on; wiring an image host is a separate,
  unscoped decision left for later.
- [x] Creator wallet select: dropdown over `funded` `role=dev` wallets —
  `App.tsx` now fetches `api.walletPool('dev')` and filters to
  `status === 'funded'` client-side (reuses the Phase 2 endpoint instead of a
  new query param on `/api/managed_wallets`)
- [x] Bundler wallet allocation: "use N bundlers" input on the Launch card →
  `LaunchRequest.bundler_count` → `ManagedWalletRepo::claim_funded(pool,
  "bundler", n, launch_id)` in `service.rs` — a server-side atomic
  `FOR UPDATE SKIP LOCKED` claim, never a client-side pick. `bundle_wallet_ids`
  removed from `PumpfunTemplateParams`/the seed script (templates no longer
  list bundler wallets at all); `bundle_leg_count` stays as the template's
  *default* leg count when a launch doesn't override it. A short pool plans a
  smaller bundle (exactly as many legs as wallets claimed) rather than erroring
  or reusing one wallet across legs.
- [x] Wire claimed wallet ids into existing create/bundle execute flow — claimed
  ids feed `compose_bundle_legs`/`bundles.legs` exactly as the old free-form
  `bundle_wallet_ids` did, so `bundle_execute.rs`/Jito submission are unchanged.
- [x] On launch completion (landed/dropped/partial), transition claimed wallets
  `reserved` → `used`; on launch failure before submit, let the TTL sweep
  release them back to `funded`. Moved the dev-wallet's own `mark_used` call
  unchanged from Phase 1 (still correct — the dev wallet's job ends at
  create-success, independent of any bundle); moved the *bundler* legs'
  `mark_used` out of `bundle_execute.rs`'s submit-success path (too early —
  submit isn't completion) into `launcher::confirm`'s three terminal branches
  (landed/dropped/partial all transition to `used`, matching this checklist's
  own wording — a dropped/partial bundle still consumed a real submit attempt).
  Also fixed a pre-existing bug while restructuring this exact code: bundle
  *planning* (not just execute) now runs after `finish` resolves, not inside
  it, so a planning error (missing `leg_structures`, empty bundler pool) can no
  longer retroactively flip an already-succeeded launch's status back to
  `failed` — matches how bundle auto-submit already treated post-create bundle
  problems as non-fatal to the launch row.

## Phase 4 — Cleanup & backup infra ✅

**Goal:** don't leak capital into dead wallets, don't lose the pool to a machine
failure.

- [x] Dust sweep job: `launcher::spawn_dust_sweep` (hourly) scans `used`
  wallets; below a 0.0001 SOL floor it retires directly (not worth a signed tx +
  fee), otherwise it transfers `balance - fee_reserve` lamports to the first
  `role=treasury` wallet via a plain `solana-client` transfer (deliberately not
  routed through pump-trader's Jito/multi-sender machinery — no landing
  urgency for a dust sweep) and retires the wallet. Skips cleanly (logs a
  warning) if no treasury wallet is configured yet.
- [x] Backup automation: `launcher::run_backup`, called after every
  `POST /api/wallet_pool/generate` batch (fire-and-forget — a backup problem
  never fails the generate response). Writes a timestamped dir under
  `WALLET_BACKUP_DIR` (opt-in — unset disables it) containing
  `managed_wallets.json` (full-fidelity export via the new
  `ManagedWallet::to_backup_json`, which — unlike the normal `Serialize` impl —
  includes `key_ref`, since a backup without it can't be restored) and a
  `keystore/` copy of every non-retired wallet's encrypted blob.
- [x] Master password/KEK stored in a **separate** location from the encrypted
  keystore backups (never bundled together) — enforced by what `run_backup`
  simply never touches: it never reads `LAUNCHER_KEK_PASSPHRASE` or `.env`, only
  the keystore dir + DB. Each backup also drops a `README.txt` restating this
  rule for whoever finds the backup later.
- [x] Restore runbook: restore keystore + DB, set KEK, decrypt one known wallet
  and confirm the derived address matches before trusting the pool. New CLI:
  `cargo run -p live -- wallet-verify <key_ref> <expected_address>` — resolves
  the keystore blob and PASS/FAILs against an expected pubkey (non-zero exit on
  mismatch, so it's a real gate, not just an eyeball check). Concrete steps:
  1. Restore the `keystore/` backup dir to the path `WALLET_KEYSTORE` will point at.
  2. Restore/replay the `managed_wallets` table (from `managed_wallets.json`, or
     the normal DB backup/sync path).
  3. Set `LAUNCHER_KEK_PASSPHRASE` from its **separately stored** location — never
     from anything that shipped alongside the keystore backup.
  4. Pick one wallet you know the address of and run
     `cargo run -p live -- wallet-verify <its key_ref> <its address>`.
  5. Only trust the restored pool once that prints `PASS`. A `FAIL` means the
     KEK, the keystore blob, or the DB row are mismatched — stop and
     investigate before pointing `live` at this restore.
- [x] Backup retention: prune `retired` wallets from active backups
  periodically — done as an ongoing property of `run_backup` itself rather
  than a separate prune job: every new backup's `keystore/` copy already
  **excludes** `retired` wallets (the JSON export still includes them, all
  statuses, for audit/history).

## Phase 5+ — Deferred (explicitly not now)

- [ ] Automated multi-hop funding fan-out (treasury → intermediate hops → pool
  wallets, randomized timing/amounts) — manual funding chosen for now
- [ ] Frontend picker for per-launch instruction fingerprint params (CU
  limit/price, slippage, instruction variant) — the randomization engine
  (`leg_structures` / `materialize_leg`) already exists server-side; this phase
  is just exposing it as an editable UI, deferred per user
- [ ] Remote/HSM-backed signer swap (KEK trait is already pluggable; env passphrase
  → KMS is a config change, not a redesign, when this becomes needed)
