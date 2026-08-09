# Venue/quote portability + cross-product SSOT drift (deferred goals)

Goals that are genuinely still open and not tracked anywhere else. Everything an
earlier audit raised that has since shipped or gone moot is listed under
"Dropped, not carried forward" below, so it is not re-raised.

## 1. Hunter is pump/SOL-only; forge already generalized this (deferred future goal)

Forge is the multi-venue product; hunter stays pump/SOL for now and would adopt forge's
model **later, as a port, not a fresh design** — forge already implements the target
shape end-to-end. Verified still true 2026-07-26:

- **Quote currency fused into units/names.** Pool PDA still hardcodes WSOL
  (`shared/ingest/pumpfun/src/pool.rs` — seeds `[0u16, authority, mint, WSOL]`);
  amounts are still `f64` SOL via `/LAMPORTS_PER_SOL`
  (`shared/executor/core/src/engine.rs`); the neutral `IngestEvent` still carries
  `sol: f64` (`shared/ingest/core/src/event.rs`); schema columns are still
  `_lamports`/`_sol` throughout (by design — see hunter's SOL-vs-lamports naming rule
  in [../../CLAUDE.md](../../CLAUDE.md) — but that rule assumes SOL is the only quote
  asset).
- **Venue recognition is a pump-only string match.** `shared/ingest/pumpfun/src/venue.rs`
  classifies by `log_messages.iter().any(|l| l.contains(pump_fun_id))` /
  `pump_swap_id`; `enum TxRelevance { Curve, Amm }` and the `venue CHECK IN
  ('curve','amm')` constraint both assume exactly pump.fun's two venues.
- **`PriceUnit` is a hard SOL/USD binary.** `shared/types/index.ts`, `usePriceDisplay.ts`
  (hardcodes `◎`), and `TokenRecord` have no quote-asset field. Pairs with the above —
  can't display a non-SOL quote until the ingest/schema side carries one.
- **Forge's reference implementation:** `forge/migrations/0001_init.sql` has
  `quote_assets` + `launchpads` dimension tables, `launchpad_id`/`quote_asset_id` FKs,
  integer `amount_quote`/`amount_base` columns, and an open venue registry (`AnyVenue`
  dispatch, not a single-arm `VenueId::PumpFun`).

**When pursued:** port forge's model, don't re-derive it. Needed: a `venue-core`-style
shared unit-type layer (`QuoteUnit`/`QuoteAmount`/`BaseAmount`/`CurveModel`), a
program-id→decoder registry replacing the single `VenueId::PumpFun` arm, un-hardcoding
the WSOL pool seed, and flipping workspace `resolver = "1" → "2"` for per-crate feature
unification. Structural prerequisites already met: the executor/ingest split, real
`Venue`/`IngestVenue` traits (`Protocol` is already a constants module, not a false venue
seam). **Acceptance test:** a USDC-paired pump token must be addable without touching
anything outside venue config + data.

## 2. Token chart is a forking SSOT risk (M18 + cross-product drift)

`hunter/frontend/src/shared/components/token-price-chart/` (3,169 lines across its
files as of 2026-07-26, still growing — was 1,862 → 1,931 → now larger) and
`forge/frontend/src/shared/components/tokenChart/` are **separate forks** of what was
originally one component, already diverging (forge's is swing-overlay-stripped; hunter's
kept growing swing/strategy overlays). Curve-math constants and pump program IDs are
similarly duplicated across both products now that the monorepo restructure split them.

**Plan, when picked up:** inventory the hunter/forge duplicated facts; for each, either
extract to a `shared/` module or add a guard test asserting the copies stay equal (the
monorepo's SSOT rule — see [../../CLAUDE.md](../../CLAUDE.md)). Any chart refactor must
decide fork-vs-share **up front**, before touching line count. Note: the "extract
`TokenPriceChart`" half of the original M18 finding (splitting the 3× ~1200-line lab
strategy pages) is now moot — those pages (`Tpsl1Page`/`Tpsl2Page`/`Swing1Page`) no
longer exist, replaced wholesale by the generic sweep/Console UI (see
[../arch/strategies.md](../arch/strategies.md)).

## Dropped, not carried forward

- **Part 3 (merge `tpsl_sniper_1/2`, `sweep_dispatch<S>`/`StrategyDescriptor`, collapse
  the 12 sweep tables, parametrize `StrategyLabPage`)** — moot, not just declined. All of
  it was about the tpsl1/tpsl2/swing1 triplicated strategy stack, which the fingerprint
  redesign deleted wholesale rather than merged (`lab/src/sweep/registry.rs` itself notes
  "the tpsl/swing retirement (Phase 7) — exactly one arm, `generic`"). There is no code
  left for this rationale to apply to.
- **"Doc hygiene: rename `trading_core`/`pump-trader`/`ingest-laserstream`"** — based on a
  false premise. These are the **current, intentional Cargo dep-key names** (the package
  rename only changed the underlying pkg/lib names — `pump-trader` is deliberately kept
  as the dep-key alias for pkg `executor-pumpfun`, etc.), not stale pre-restructure
  vocabulary — see the crate table in [../../CLAUDE.md](../../CLAUDE.md). No rename
  needed; verify against that table before ever re-raising this.
- Everything else in the original audit (real-money bugs C1/C2/H1-H6/H9/M1-M4/M8-M10,
  frontend M14-M16) shipped and is covered by `docs/arch/position-lifecycle.md`,
  `docs/arch/trade-execution.md`, `docs/plans/trade-execution/sell-close-smoke.md`,
  `docs/arch/frontend.md`.

## No unresolved audit IDs remain

The audit's bare `M*`/`L*` identifiers are not carried here: the doc that defined them is
gone, so an ID alone points nowhere a reader can act on. The one item whose subject was
still nameable — a `runtime_cache` module sitting in shared core instead of `live` — is
moot: run state is the in-memory `TokenCache` plus the engine's own state, and there is no
such module to relocate. The pump-specific ingest contract that a couple of those IDs
circled is the live item, and it is **H8** above.
