# TPSL Unification Plan (Phase 5.1)

Goal: eliminate the near-byte-identical `tpsl_sniper_1` / `tpsl_sniper_2` clones
so every fix is applied **once**, not twice, while keeping **table-level DB
isolation** (each strategy keeps its own `tpslN_*` tables and rows).

Branch: `refactor/tpsl-unify`. Money-path code — every step must compile and be
behavior-preserving; migrate incrementally, never big-bang.

---

## Measured divergence (normalized for `Tpsl1↔Tpsl2` naming, CRLF-stripped)

| file | true diff lines | nature of divergence |
|------|-----------------|----------------------|
| util.rs | 0 | identical |
| paper_run.rs | 0 | identical |
| lifecycle.rs | 0 | identical |
| runtime_cache.rs | 3 | one comment only |
| handler.rs | 4 | struct name only |
| mod.rs | 13 | wiring/names |
| backtest.rs | 19 | names + scalp |
| execution/paper.rs | 20 | names + scalp |
| execution/mod.rs | 35 | tpsl2 scalp exports |
| service.rs | 57 | **scalp entry-arm hook** |
| execution/real.rs | 63 | **scalp entry-arm wait** |
| entry/mod.rs | 94 | **scalp entry policy** |
| exit/mod.rs | 137 | **scalp exit gate** |
| **tpsl2-only** cohort.rs (167), entry/scalp.rs (416) | — | scalp engine |

Everything outside the **scalp** logic is identical modulo three things:
1. naming (`Tpsl1`/`Tpsl2`/`TPSL1`/`TPSL2`/`tpsl1`/`tpsl2`),
2. the **Rule** type (`Tpsl1StrategyRule` vs `Tpsl2StrategyRule`),
3. the three **repos** (`tpslN_position_repo`, `tpslN_paper_trading_repo`,
   `tpslN_strategy_rule_repo`).

Key facts that make this tractable:
- **Positions already share** `crate::models::Position` (and the paper model).
  Only the *repos* differ (tables).
- `Tpsl2StrategyRule` == `Tpsl1StrategyRule` **+ optional scalp gate fields**
  (`p_entry_min_age_secs`, `p_entry_pullback_pct`, … `p_exit_cohort_ratio`), all
  defaulting to `None`. A single superset struct serves both.
- Rule repos map rows through a **per-strategy `…DbRow` (FromRow) + explicit
  column SELECT**, separate from the model. So the model can unify while each
  repo keeps its own table/columns (tpsl1's repo fills scalp fields `None`).

---

## Target architecture

```
strategies/
  tpsl_core/                 ← the single engine (was duplicated)
    rule.rs                  ← unified TpslRule (superset struct)
    repos.rs                 ← traits: PositionRepo, PaperRepo, RuleRepo
    variant.rs               ← trait TpslVariant + scalp hooks
    runtime_cache.rs         ← generic TpslRuntimeCache<V>
    service.rs, lifecycle.rs, exit/, execution/, entry/, backtest.rs, handler.rs
  tpsl_sniper_1/             ← thin: `struct V1; impl TpslVariant for V1`
  tpsl_sniper_2/             ← thin: `struct V2; impl TpslVariant for V2` (+ scalp)
```

- **`TpslVariant`** carries the per-strategy bits as associated types + methods:
  ```rust
  trait TpslVariant: Clone + Send + Sync + 'static {
      type PositionRepo: PositionRepo;
      type PaperRepo:    PaperRepo;
      type RuleRepo:     RuleRepo;
      const NAME: &'static str;            // "TPSL1" / "TPSL2"
      fn position_repo(pool: PgPool) -> Self::PositionRepo;
      fn paper_repo(pool: PgPool)     -> Self::PaperRepo;
      fn rule_repo(pool: PgPool)      -> Self::RuleRepo;
      // Scalp hooks — default no-op (tpsl1); tpsl2 overrides:
      async fn await_entry_arm(ctx: &EntryArmCtx<'_>) -> bool { true }
      fn scalp_entry_gate(rule: &TpslRule, trades: &[Trade], ...) -> bool { true }
      fn scalp_exit_gate(...) -> Option<ExitReason> { None }
  }
  ```
- The engine modules become generic over `V: TpslVariant` (or hold the repos as
  `Arc<dyn …>`). Positions/rule are shared concrete types, so only the repos and
  scalp hooks are abstracted.
- **DB isolation is unchanged**: the repos are still the concrete per-table
  `tpslN_*` repos behind the traits.

---

## Migration order (each step compiles + is behavior-preserving)

0. **Done:** reconnaissance + this plan (branch `refactor/tpsl-unify`).
1. **Unify the Rule model.** Add `models/tpsl_strategy_rule.rs` with the superset
   `TpslRule`; point both rule repos' conversions at it (tpsl1 → scalp `None`);
   replace `Tpsl1StrategyRule`/`Tpsl2StrategyRule` references (incl. API handlers)
   with `TpslRule`. Behavior-preserving: tpsl1 never reads the scalp fields.
2. **Repo traits.** Define `PositionRepo` / `PaperRepo` / `RuleRepo` in
   `tpsl_core::repos` from the exact method surface the engine calls; `impl` them
   for the six concrete repos. Additive; no consumer change yet.
3. **`TpslVariant` trait + no-op scalp hooks**, plus `V1`/`V2` unit structs.
4. **Move the identical modules** into `tpsl_core`, generic over `V`: `util`,
   `paper_run`, `lifecycle`, `runtime_cache`, `handler` (true diff ≤ 4). Re-point
   `tpsl_sniper_1/2` at them. Compile + (ideally) run.
5. **Move `service` / `execution` / `entry` / `exit` / `backtest`**, routing the
   scalp divergences through the `TpslVariant` hooks. tpsl2's `cohort.rs` +
   `entry/scalp.rs` become the `V2` hook implementations; `V1`'s hooks stay the
   defaults (provably identical to today's tpsl1).
6. **Delete** the now-empty `tpsl_sniper_2` duplicate modules; `tpsl_sniper_1/2`
   are just the `V1`/`V2` instantiations + routes.

## Risk controls
- `cargo check` after every step; keep steps small and reviewable.
- The `V1` hooks are literal no-ops, so tpsl1 behavior is unchanged by
  construction; tpsl2's scalp logic is moved verbatim into `V2` hooks.
- Tables/rows are never touched — only the in-memory engine is deduplicated.
- Land step-by-step (own commits); validate paper mode before real mode.
