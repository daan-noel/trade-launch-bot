# hunter — history

**Not a reading path.** Nothing in `CLAUDE.md` or `docs/arch/` links here, and nothing
should: this tier costs zero context until someone greps it deliberately.

What lives here: incidents and their RCAs, superseded approaches, and research journals —
the *how we got here*. The **rules** those produced live in `CLAUDE.md` (hot-path
landmines) and `docs/arch/` (current behavior). If a past fact still changes what you do
today — "runs stored before date X are priced wrong" — it belongs in the present-tense
tier, not here.

Entry shape: `# <what broke> (YYYY-MM-DD)` → **Symptom** (with the numbers) → **Cause**
(the mechanism) → **Fix** → **The rule this produced** (one line + where that rule now
lives).

| Entry | One-line |
| --- | --- |
| [2026-08-05 paper `ExitStuck` backlog](2026-08-05-paper-exitstuck-backlog.md) | 45% of paper positions stranded open; the bias was one-directional, so paper PnL read high |
| [2026-08-04 group-key unit drift](2026-08-04-group-key-unit-drift.md) | One wire field, three readers, three units — plus a `to_char` mask that overflowed into a *valid* wrong group |
| [2026-08-04 token-scale 1e6 PnL](2026-08-04-token-scale-1e6-pnl.md) | A factor that cancelled out of SOL PnL but not out of the stored token count |
| [2026-08-02 unemitted fill leaks a slot](2026-08-02-unemitted-fill-leaks-slot.md) | Leaked concurrency slots filled a live rule's cap and silenced it ~17 h |
| [2026-07-30 boot-recovery killstorm](2026-07-30-boot-recovery-killstorm.md) | 70 consecutive kills, 14 h with no rule evaluated — unbounded boot scan + a watchdog policing a boot |
| [2026-07-27 replay-anchor blackout](2026-07-27-replay-anchor-blackout.md) | A recovery anchor derived from the failing attempt's own state, so it never survived the failure |
| [2026-07-26 sweep entry-cache poisoning](2026-07-26-sweep-entry-cache-poisoning.md) | Cached a resolved entry under a key that didn't determine it; every sibling combo inherited it |
| [2026-07-22 heartbeat green through a wedge](2026-07-22-heartbeat-green-through-wedge.md) | Liveness measured the loop iterating, not work landing — 7 h outage, watchdog silent |
| [2026-08-03 creator-crew screen refuted](2026-08-03-creator-crew-screen-refuted.md) | The `p=0.0002` creator-reputation signal did not replicate; backer reputation came out inverted |
| [2026-08-03 wallet-copying closed](2026-08-03-wallet-copying-closed.md) | A 7-day mine of profitable wallets — negative even at zero latency, so the whole class is closed |
| [2026-07-29 launch-crew refuted OOS](2026-07-29-launch-crew-refuted-oos.md) | Registry-copy + fixed-TP looked strong in-sample and failed out-of-sample |
| [2026-07 chart swing + chain overlays removed](2026-07-chart-swing-and-chain-overlays-removed.md) | Two chart overlays deleted with the `swing_1` stack; geometry recorded in case they return |
| [Wallet research journal (07-21 → 07-31)](wallet-research-2026-07.md) | The run-by-run scalper reverse-engineering; conclusions live in `@plans/strategies/wallet-analysis.md` |
