# Ineffectiveness / SSOT Audit + Fixes — 2026-07-07

Session handoff. A three-agent audit of `solana-launch-platform` (frontend, backend/SSOT,
FE↔BE seam) followed by fixes in three tiers. **All backend Rust edits below were made but
NOT compiled** in the session's environment — you must `cargo check` + apply migrations.
The **frontend `npm run build` is green** throughout (verified after every frontend change).

---

## TL;DR — what you must run

```powershell
# From repo root
cargo check -p platform-core
cargo check -p launcher
cargo check -p live
cargo test  -p platform-core        # 4 CHECK-vocabulary roundtrip tests

# Apply the two new migrations (review 0007 against real data first — it alters
# schema AND backfills/strips data):
#   migrations/0006_status_check.sql
#   migrations/0007_launch_metadata_fk.sql
# (sqlx::migrate! runs them on live boot; or apply manually via psql)

cd frontend-launch; npm run build   # already verified green
```

---

## The original problem that kicked this off

The Launch Console had **two** metadata dropdowns (a launch-template picker + a
"Load from metadata template" picker), because token metadata (name/symbol/uri) was
**triplicated**: inlined in `launch_templates.params`, stored as `metadata_templates`
rows, AND overridable per-launch — with `metadata_templates` never actually read at
launch time. That was the tip of a broader SSOT/duplication pattern the audit mapped out.

---

## TIER 1 — bug + high-value fixes (DONE)

### 1. Real bug: Launch Console edit-wiping
`frontend-launch/src/App.tsx` — the metadata-reset `useEffect` re-fired on every
`templates` array-reference change, so saving in the Templates tab silently wiped
in-progress launch edits. **Fix:** gated the reset with a `${id}@${updated_at}` ref so it
only reseeds when the *selected* template actually changes.

### 2. Dead-code sweep (safe subset only)
- Removed FE `api.wallets` + `ManagedWallet` interface (`api.ts`).
- Removed backend `GET /api/managed_wallets` route + handler + orphaned
  `ManagedWalletRepo::list` (superseded by `/api/wallet_pool`).
- **KEPT `token_sync_state`** — the audit flagged it as dead, but it's wired into
  `scripts/db-incremental-sync.ps1` + roadmap (forward scaffolding). Deleting it would
  break the sync script.
- **Left for your call** (plausible ops value, not deleted): `bundle_execute` (manual
  retry), `launch_get`, `bundle_get`, `token_overview`, `token_trades`.

### 3. Status enums + CHECK constraints
- New `platform_core::models::status` module: `LaunchStatus` (pending/created/failed),
  `BundleStatus` (planned/submitting/submitted/landed/dropped/partial/failed). Mirrors
  `MarketKind` (as_str + FromStr + ALL + roundtrip test).
- Swapped every loose status literal in `service.rs`, `bundle_execute.rs`, `confirm.rs`.
- **Migration `0006_status_check.sql`** — adds CHECK constraints on both columns; also
  fixed a latent bug where `bundles` defaulted to `'pending'` but code always inserts
  `'planned'` (realigned the default).

### 4. Metadata SSOT — FULL CUTOVER (the original complaint)
Chosen approach: full cutover (not incremental). Metadata now lives in ONE place.
- **`launch_templates.metadata_template_id`** FK (`ON DELETE SET NULL`) references a
  `metadata_templates` row.
- `launcher::service::execute_launch` + `probe` resolve name/symbol/uri from that row at
  create time (per-launch override = a different `metadata_template_id`, not free text).
- Dropped inlined `params.{name,symbol,uri}` from `PumpfunTemplateParams` + `LaunchRequest`.
- `metadata_templates.image_uri` made **nullable** (a preset backfilled from a legacy
  launch template has no separately-pinned image; it's embedded in the JSON at `uri`).
- **Migration `0007_launch_metadata_fk.sql`** — adds the FK, backfills every legacy
  launch template into its own `metadata_templates` row + links it, then strips
  `name/symbol/uri` from `params`.
- Frontend: Launch Console + LaunchTemplates form collapsed to **one** metadata dropdown;
  LaunchTemplates list gained a "Metadata" column.
- Updated `scripts/seed-dev-launch.sql` (new pattern), `CLAUDE.md` ("Metadata SSOT" bullet),
  and doc comments in `metadata_upload.rs` / `models/metadata.rs`.

---

## TIER 2 — quick SSOT wins (DONE)

### 5. SOL quote-asset id/mint de-hardcoded
Added `QuoteAssetRepo::native()` (resolves the `is_native` row → id + mint, fails loudly
if the seed is missing, matching `PumpFunAdapter::resolve`). `sol_price.rs` + `main.rs` now
use it; dropped the hardcoded `SOL_QUOTE_ASSET_ID = 1` and `WSOL_MINT` consts.

### 6. Pump decimals `6`
Named the bare literal in `service.rs` as a local `PUMP_TOKEN_DECIMALS` const with a
cross-ref comment to `ingest_host::map::PUMP_TOKEN_DECIMALS`. **Could not truly unify** —
`ingest-host` and `launcher` share only the venue-neutral `platform-core`, so a shared home
would violate that rule or couple the crates (deliberate-decoupling case).

### 7. Duplicate structs collapsed
- `StoredBundleLeg` deleted (byte-identical to `BundledLegPlan`); `legs_from_json` now
  returns `BundledLegPlan`; `legs_to_json` simplified to `json!(legs)` (derived Serialize
  IS the persisted shape). `bundle.rs`, `lib.rs`.
- `UpdateLaunchTemplate` → `type` alias of `NewLaunchTemplate` (`models/own_launch.rs`).

---

## TIER 3 — enums + frontend cleanup (DONE)

### 8. Wallet role/status enums
Added `WalletRole` (dev/bundler/treasury/trading) + `WalletStatus`
(generated/funded/reserved/used/retired) to `platform_core::models::status` (roundtrip
tests). Swapped every literal in `service.rs`, `bundle_execute.rs`, `dust_sweep.rs`,
`probe.rs`, `wallet_pool.rs` (incl. the `matches!` validation → `WalletRole::from_str`),
`backup.rs`. Repos keep `&str` signatures.

### 9. Frontend shared components (`frontend-launch/src/components/`)
- **`StatusPill.tsx`** — replaced two drifted copies (App normalized the CSS class,
  WalletPool didn't — latent bug).
- **`useResource.ts`** — the `loading`/`error`/`load()` triplet each tab reimplemented.
  Applied to WalletPool + MetadataTemplates.
- **`DataTable.tsx`** — the list-table shell; empty-row `colSpan` derives from
  `columns.length` (fixes the manual-colSpan drift). Applied to all 3 list tables.
- **`Field.tsx`** — the `<div className="field"><label/>…</div>` wrapper. Applied to
  WalletPool + MetadataTemplates forms.

---

## Files touched (by area)

**Migrations (NEW):** `0006_status_check.sql`, `0007_launch_metadata_fk.sql`

**platform-core:**
- `models/status.rs` (NEW — LaunchStatus, BundleStatus, WalletRole, WalletStatus)
- `models/mod.rs` (exports), `models/metadata.rs` (image_uri Option + doc),
  `models/own_launch.rs` (metadata_template_id field; UpdateLaunchTemplate alias)
- `storage/repositories/dimensions.rs` (QuoteAssetRepo::native)
- `storage/repositories/own_launch.rs` (removed ManagedWalletRepo::list; FK in
  insert/update SQL), `storage/repositories/metadata.rs` (unchanged logic, image_uri Option)

**launcher:** `service.rs`, `probe.rs`, `bundle_execute.rs`, `confirm.rs`, `bundle.rs`,
`wallet_pool.rs`, `dust_sweep.rs`, `backup.rs`, `metadata_upload.rs`, `lib.rs`

**live:** `http.rs` (route/body/handlers), `main.rs` (native quote resolve), `sol_price.rs`

**frontend-launch/src:** `App.tsx`, `LaunchTemplates.tsx`, `MetadataTemplates.tsx`,
`WalletPool.tsx`, `api.ts`, `components/{StatusPill,useResource,DataTable,Field}.tsx`

**docs/scripts:** `CLAUDE.md`, `scripts/seed-dev-launch.sql`, this file

---

## STILL OPEN (from the audit — not done)

- **Wire-type codegen** — every TS shape in `api.ts` is a hand-mirrored copy of a Rust
  struct with no codegen (drift risk; several are already partial subsets). Consider
  `ts-rs` / `schemars`. Larger effort.
- **Chatty bootstrap / no cache** — shared lists (`templates`, `metadataTemplates`,
  `launchpads`, `quoteAssets`) refetch on every tab switch; static dimensions have no
  cache. Could add a `GET /api/launch_console/bootstrap` composed response and/or lift
  shared lists to App-level state.
- **Dead ops-value routes** — `bundle_execute`, `launch_get`, `bundle_get`,
  `token_overview`, `token_trades` have no FE caller; keep behind an explicit `/admin`
  scope or delete (your call).
- **Small tails:** `LaunchTemplates.tsx` still has a bespoke multi-resource `load` (not on
  `useResource`, because it also sets form defaults), and its form fields aren't on `Field`
  yet (left to keep the diff reviewable).
- **i64 lamports over JSON** — large amount fields cross the wire as JS `number`; fine now,
  but no guard above 2^53. Serialize as strings if larger denominations ever appear.

---

## Key architectural facts learned this session (for the next session)

- **Crate deps:** `launcher` → {platform-core, pump-trader}; `ingest-host` →
  {platform-core, ingest-laserstream}. They share **only** `platform-core`, which is
  deliberately **venue-neutral** (no pump-specific constants). This is why pump decimals
  can't be truly single-sourced across them.
- **CHECK-vocabulary SSOT pattern:** `platform_core::models::status` is the home for all
  enum⇄DB-CHECK pairs (mirror `venue::MarketKind`): `as_str()` must equal the SQL CHECK,
  guarded by a no-DB roundtrip test. New value = code + CHECK edit.
- **Metadata SSOT:** token identity lives in `metadata_templates`; `launch_templates`
  references it via `metadata_template_id`. Never inline name/symbol/uri anywhere else.
- **Migrations** run via `sqlx::migrate!("../../migrations")` (embedded, checksummed,
  run-once) on `live` boot.
