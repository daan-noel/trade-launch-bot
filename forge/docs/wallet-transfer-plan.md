# Wallet-to-Wallet Transfer Plan

> **Status: SHIPPED.** Backend `launcher::wallet_transfer` + `POST /api/wallet_pool/transfer`,
> frontend per-row Transfer modal, all built (`cargo check -p forge-live` clean,
> `npm run build` green). `plan_exec::execute_transfer` now returns the exact lamports
> moved so the report is exact (dust-sweep / funding / consolidate callers updated).

Move SOL between managed wallets from the operator dashboard — no Phantom, no
handling raw private keys. Primary driver: a managed wallet sitting at 0 SOL
can't pay the fee for its own buy/sell; top it up in two clicks. Generalizes to
any operator-chosen move.

## Decisions (locked)

- **Scope:** any managed wallet → any managed wallet (general).
- **Amount:** exact SOL, or a **Max** toggle that sweeps the source to ~0.
- **Auth:** existing fail-closed bearer gate only (no second factor).

## The reuse story — one primitive already does the on-chain work

There is a single, typed, audited SOL-transfer SSOT that funding, dust-sweep, and
consolidate all route through. **We add no new signing / tx-assembly code.**

`launcher::plan_exec::execute_transfer(rpc, signer, from, to, mode, confirm)`
— [plan_exec.rs:117](../launcher/src/plan_exec.rs)

- `TransferMode::Exact(lamports)` — move exactly N; source pays the fee on top.
- `TransferMode::SweepAll { min_lamports }` — probe the fee, send `balance − fee`
  so the source lands at exactly 0; returns `Ok(None)` if balance ≤ min/fee.

The [dust sweep](../launcher/src/dust_sweep.rs) is already a managed-wallet →
treasury transfer; this feature is the same shape with an operator-chosen source,
destination, and amount, and **without retiring the source**.

> SSOT rule (CLAUDE.md): do NOT add a 4th raw `system_instruction::transfer`
> site. Route through `execute_transfer`.

## Design — 3 thin layers

### 1. Backend orchestration — new `launcher/src/wallet_transfer.rs`

Exported from [lib.rs](../launcher/src/lib.rs).

```
pub async fn transfer_between_wallets(
    pool, settings,
    from_id: Uuid, to_id: Uuid,
    amount: TransferAmount,          // Exact(sol) | Max
) -> Result<TransferReport>
```

Flow:
1. `from_id != to_id` else 400.
2. `from = ManagedWalletRepo::get(from_id)`, `to = ManagedWalletRepo::get(to_id)`.
3. **Status guard on the source:** reject if `from.status` ∈ {`funding`,
   `reserved`, `retired`}. `funding`/`reserved` = a launch is mid-flight on that
   wallet; `retired` = shredded/backed out. `funded` / `used` / `treasury` /
   `generated` are allowed sources (a `generated`/0-balance source just no-ops on
   `SweepAll` or errors on `Exact` — insufficient funds).
4. `Pubkey::from_str` both addresses.
5. `let signer = keystore::resolve_signer(&settings.keystore_dir, &from.key_ref, &kek)?`
   (`kek = EnvKek::from_passphrase(&settings.kek_passphrase)`) — the **source**
   signs and pays the fee.
6. `execute_transfer(rpc, signer.as_ref(), from_addr, to_addr, mode, /*confirm=*/true)`.
   `TransferAmount::Exact(sol)` → `Exact(to_lamports(sol))`;
   `Max` → `SweepAll { min_lamports: SWEEP_MIN }` (reuse the sweep floor).
7. On success, `record_balance` both wallets so the pool table reflects the new
   balances immediately (funding does the same via its treasury write-back).
   Note: `record_balance` auto-promotes a `generated`/`funding` wallet to
   `funded` once it crosses `MIN_FUNDED_LAMPORTS` — so topping up an empty
   destination naturally promotes it. Acceptable / arguably correct; called out
   so it isn't a surprise.

**Concurrency:** if the source **or** destination is a `role=treasury` wallet,
take the process-wide `FUNDING_LOCK` (wallet_funding.rs) for the duration, so an
ad-hoc treasury move can't race a funding pass that snapshots treasury balance.
Non-treasury ↔ non-treasury moves skip the lock (independent).

`TransferReport { from_id, to_id, from_address, to_address, lamports_sent,
signature: Option<String> }` (`None` = a `Max` sweep found nothing worth sending).

### 2. HTTP — `POST /api/wallet_pool/transfer`

In [http.rs](../live/src/http.rs), copied from `wallet_pool_fund` ([:345]):

```rust
#[derive(serde::Deserialize)]
struct TransferBody { from_id: Uuid, to_id: Uuid, amount_sol: Option<f64>, max: Option<bool> }
```
- `web::Data<PgPool>` + `web::Data<Option<LauncherSettings>>` + `web::Json<TransferBody>`.
- `launcher_settings(&settings)?` → 503 if the launcher isn't configured.
- `max == Some(true)` → `TransferAmount::Max`, else require `amount_sol` (400 if
  both absent).
- Register in the `.route(...)` chain at [http.rs:77]. **Inherits the bearer gate
  automatically** (mutating route). Returns `TransferReport` as JSON.

### 3. Frontend — per-row action + modal

In [WalletPoolPage.tsx](../frontend/src/features/wallets/WalletPoolPage.tsx),
model on the existing **Export-key** modal (closest per-row-action template):

- New `DataTable` column button **"Transfer"** → opens a modal keyed by the
  source wallet (`transferTarget` state, like `exportTarget`).
- Modal fields: destination **`Select`** (the other managed wallets — exclude the
  source; show role + short address + balance), an amount **`Input`** (SOL), and a
  **"Max"** checkbox that disables the amount input.
- Submit → `transferSol({ from_id, to_id, amount_sol?, max? }).unwrap()`, show the
  resulting signature / lamports in a `Banner`, wipe modal state on close.
- `endpoints.ts`: add
  ```ts
  transferSol: build.mutation<TransferReport, TransferArgs>({
    query: (body) => ({ url: '/api/wallet_pool/transfer', method: 'POST', body }),
    invalidatesTags: ['Wallets'],   // pool table refetches
  })
  ```
  + a `TransferReport` type in `@shared/types`.

## Out of scope (deliberately)

- No funding-style safety rails (spend cap, treasury reserve floor, dry-run) on
  ad-hoc transfers — bearer auth only, per decision. `execute_transfer`'s own
  fee/rent math still prevents a sub-fee or below-rent send.
- No Jito / retry — a SOL move has no landing urgency (same rationale as sweep).
- No multi-hop / obfuscation timing — this is an operator utility, not a launch
  step.

## Definition of done

- `cargo check -p forge-live` clean; clippy on touched code.
- `npm run build` green in `forge/frontend`.
- Stayed in `launcher` + `forge-live` + `forge/frontend`; no new transfer-assembly
  site; no secrets in code.
- Docs: this file + a line in CLAUDE.md's wallet-pool status if shipped.

## Source map

| Concern | File |
| --- | --- |
| Transfer primitive (reuse) | [launcher/src/plan_exec.rs](../launcher/src/plan_exec.rs) |
| Nearest template (wallet→treasury) | [launcher/src/dust_sweep.rs](../launcher/src/dust_sweep.rs) |
| Signer load | [launcher/src/keystore.rs](../launcher/src/keystore.rs) `resolve_signer` |
| Wallet repo (`get`, `record_balance`) | [core/src/storage/repositories/own_launch.rs](../core/src/storage/repositories/own_launch.rs) |
| Funding lock / patterns | [launcher/src/wallet_funding.rs](../launcher/src/wallet_funding.rs) |
| New orchestration | `launcher/src/wallet_transfer.rs` (new) |
| HTTP handler template | [live/src/http.rs](../live/src/http.rs) `wallet_pool_fund` |
| Frontend page + modal template | [frontend/src/features/wallets/WalletPoolPage.tsx](../frontend/src/features/wallets/WalletPoolPage.tsx) |
| RTK endpoints | `forge/frontend/src/shared/store/endpoints.ts` |
