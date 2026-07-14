# Trade-history read paths: Parquet lake vs Postgres

Deep-dive for the data-scale rule in [CLAUDE.md](../../../CLAUDE.md) *"bound every query;
single-rule simulate reads the lake, not PG."* Explains which trade-history reads go to
the sealed Parquet lake, which stay on Postgres, and why. Related: `@arch/database.md`,
`@arch/sweep.md`, `@plans/token-analysis/swing-detection-logic.md`.

## The one deliberate full-history carve-out on PG

`GET /api/tokens/:mint/trades` (`get_trades` → `TradeRepo::find_by_mint_paged`,
`limit <= 0` ⇒ no `LIMIT`) returns a token's **full** trade history **on purpose**. The
inspect charts (Positions / Sim / grouped-sweep) resolve their entry/exit markers + swing
legs against this exact trade set, so a first-N cap mis-snapped the exit / later swing legs
off a high-volume token. It is still mint-scoped (never the whole table) and a cold,
deliberately-opened path — **don't re-add a row cap**. A positive `limit` still pages.

## Single-rule simulate reads the lake, not PG

tpsl1/tpsl2/swing1 `.../simulate` → `strategies::sim_fetch::fetch_sim_histories` →
`LakeSource::load` with `Selection::with_signatures = true` — the **same corpus + same
`SweepTrade`** the grouped sweep uses. There is no separate `SimTrade`; the flag only
populates `tx_signature` for Solscan links.

The grouped sweep loads it `false` and instead resolves its token-results table's
entry/exit signatures via a narrow indexed `(mint, slot, side)` PG lookup
(`grouped_sweep.rs`'s `resolve_fill_signatures` → `TradeRepo`) rather than carrying the
extra bytes through every sweep row. **That lookup plus the candidate-token scan are the
only remaining PG reads of the `trades` table anywhere in `lab`** — every other
trade-history path (grouped sweep, simulate, swing1-detect, the generic `swing.rs`
analyzer, backtests) is lake-only.

## Sealed-days-only + the stale-lake warning

The lake is **sealed-days-only**, so keep
`cargo run -p lab -- lake-export --include-today` on a cadence or simulate on recent
(today's) tokens returns truncated histories (the loader logs a stale-lake warn). Parity
with the sweep is guarded by `lake::duck::parity_tests` (no longer `--ignored`: auto-runs
when `$SWEEP_LAKE_DIR` points at a populated lake, self-skips otherwise), and the
writer/reader lake column names are single-sourced in `lab/src/lake/schema.rs`.

## swing1-detect reads lake ∪ PG fresh tail

The per-token **`swing1-detect`** endpoint reads the token's **full history = sealed lake
∪ PG fresh tail** (`fetch_full_history_one`: lake for the deep past, plus every PG row on a
slot the lake never reached) via the shared `swing_1::funnel::build_swing1_funnel`.
Lake-only blanked the overlay for a token created after the last `lake-export` (entry/exit
markers still showed — they come from the PG fills, not the lake); the PG-tail union closes
that, and the lake still covers tokens older than PG's 30-day `trades` retention.

The swing1 backtest still **carries its legs** in the result row — so the *sim/sweep*
inspect chart's legs are the sim's own (no re-detect); only the *position* overlay
re-detects (and now sees the fresh tail).

## Bounds are analysis-agnostic

`MAX_TRADES_RETAINED` is the **live in-RAM cache trim, never an analysis read bound** —
analysis reads full history. The **generic** `swing.rs` endpoints
(`detect_token_swings`/`detect_tokens_swings_batch` — a separate analyzer from the swing1
strategy) also read the same uncapped lake now; the batch path resolves its whole mint list
in **one** `fetch_sim_histories` call instead of per-mint PG round trips.
