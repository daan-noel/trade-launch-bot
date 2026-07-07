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
| **3 — Token Launch integration** | Metadata + creator/bundler wallet selection wired into launch flow | Not started |
| **4 — Cleanup & backup infra** | Dust sweep, encrypted-store backup, restore runbook | Not started |
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

## Phase 3 — Token Launch page integration

**Goal:** launch flow consumes the pool instead of a free-form wallet picker.

- [ ] Metadata editing panel (name/symbol/image/description/etc.)
- [ ] Creator wallet select: dropdown over `funded` `role=dev` wallets
- [ ] Bundler wallet allocation: "use N bundlers" → server-side random claim from
  `funded` `role=bundler` pool (reuses Phase 1 claim query, not a client-side pick)
- [ ] Wire claimed wallet ids into existing create/bundle execute flow
- [ ] On launch completion (landed/dropped/partial), transition claimed wallets
  `reserved` → `used`; on launch failure before submit, let the TTL sweep release
  them back to `funded`

## Phase 4 — Cleanup & backup infra

**Goal:** don't leak capital into dead wallets, don't lose the pool to a machine
failure.

- [ ] Dust sweep job: scan `used` wallets, sweep balance above a threshold back to
  treasury, transition to `retired`
- [ ] Backup automation: after each generation batch, copy the encrypted keystore
  + export current wallet status table
- [ ] Master password/KEK stored in a **separate** location from the encrypted
  keystore backups (never bundled together)
- [ ] Restore runbook: restore keystore + DB, set KEK, decrypt one known wallet
  and confirm the derived address matches before trusting the pool
- [ ] Backup retention: prune `retired` wallets from active backups periodically
  (no live value once swept, smaller exposure surface)

## Phase 5+ — Deferred (explicitly not now)

- [ ] Automated multi-hop funding fan-out (treasury → intermediate hops → pool
  wallets, randomized timing/amounts) — manual funding chosen for now
- [ ] Frontend picker for per-launch instruction fingerprint params (CU
  limit/price, slippage, instruction variant) — the randomization engine
  (`leg_structures` / `materialize_leg`) already exists server-side; this phase
  is just exposing it as an editable UI, deferred per user
- [ ] Remote/HSM-backed signer swap (KEK trait is already pluggable; env passphrase
  → KMS is a config change, not a redesign, when this becomes needed)
