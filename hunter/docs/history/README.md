# hunter - history

**Not a reading path.** Nothing in `CLAUDE.md` or `docs/arch/` links here, and nothing should:
this tier costs zero context until someone greps it deliberately.

What lives here: incidents and their RCAs, and one ledger of superseded research. The
**rules** those produced live in `CLAUDE.md` (hot-path landmines), `docs/arch/` (current
behavior) and `docs/plans/` (deep-dive references). If a past fact still changes what you
do today - "runs stored before date X are priced wrong" - it belongs in the present-tense
tier, not here.

Entry shape for an incident: `# <what broke> (YYYY-MM-DD)` -> **Symptom** (with the numbers)
-> **Cause** (the mechanism) -> **Fix** -> **The rule this produced** (one line + where that
rule now lives).

## Superseded research

| Entry | One-line |
| --- | --- |
| [Refuted search lines, 07 -> 09](2026-09-03-refuted-lines-ledger.md) | The whole pre-2026-09-03 search program in one table: signal rounds 2-11, every wallet clone, the token/crew/fingerprint screens, the island and 6ix lines - what each concluded, and the nine laws that survive |

The methodology that replaced it:
[market-model-and-workflow.md](../plans/strategies/market-model-and-workflow.md).

## Incidents and RCAs

| Entry | One-line |
| --- | --- |
| [2026-08-31 backtest price basis + impact denominator](2026-08-31-backtest-price-basis-and-impact-denominator.md) | Two pricing defects of opposite sign hid each other; every `sim_results` run stored before this date is incomparable |
| [2026-08-28 TokenCache eviction spinlock](2026-08-28-token-cache-eviction-spinlock.md) | A DashMap guard held across `.await` wedged both workers at 100% CPU; watchdog kills were the symptom |
| [2026-08-27 fingerprints duplicated by an inert width](2026-08-27-fingerprint-inert-width-duplicates.md) | A bucket width that matched nothing still forked identity and the auto-name |
| [2026-08-27 bucket epsilon scaled with width](2026-08-27-bucket-epsilon-scaled-with-width.md) | The retired epsilon was in ratio units, so near-edge values misfiled into the next bucket |
| [2026-08-22 `m_bundle` removed](2026-08-22-m-bundle-removed.md) | The launch-bundle group produced one refuted rule and kept charging an hourly sweep on the live box |
| [2026-08-21 trailing exits were hard stops](2026-08-21-trailing-exit-peak-bug.md) | A fancy-index `out=` no-op pinned the running peak at entry, so every offline trail silently became a fixed stop |
| [2026-08-13 unexplainable `untagged_buy` exit](2026-08-13-nonvol-buy-exit-unexplainable.md) | A pattern list missing one tip-transfer variant booked bot buys as organic demand; three UI surfaces then hid it |
| [2026-08-11 scoped sweeps truncated](2026-08-11-scoped-sweep-token-cap-truncation.md) | Fingerprint-scoped grouped sweeps silently cut short by `token_cap` |
| [2026-08-05 seven watchdog kills](2026-08-05-watchdog-kills-are-real-outages.md) | Real ingest outages, not a watchdog bug - the transport was mute |
| [2026-08-05 paper `ExitStuck` backlog](2026-08-05-paper-exitstuck-backlog.md) | 45% of paper positions stranded open; the bias was one-directional, so paper PnL read high |
| [2026-08-04 group-key unit drift](2026-08-04-group-key-unit-drift.md) | One wire field, three readers, three units - plus a `to_char` mask that overflowed into a *valid* wrong group |
| [2026-08-04 token-scale 1e6 PnL](2026-08-04-token-scale-1e6-pnl.md) | A factor that cancelled out of SOL PnL but not out of the stored token count |
| [2026-08-02 unemitted fill leaks a slot](2026-08-02-unemitted-fill-leaks-slot.md) | Leaked concurrency slots filled a live rule's cap and silenced it ~17 h |
| [2026-07-30 boot-recovery killstorm](2026-07-30-boot-recovery-killstorm.md) | 70 consecutive kills, 14 h with no rule evaluated - unbounded boot scan + a watchdog policing a boot |
| [2026-07-27 replay-anchor blackout](2026-07-27-replay-anchor-blackout.md) | A recovery anchor derived from the failing attempt's own state, so it never survived the failure |
| [2026-07-26 sweep entry-cache poisoning](2026-07-26-sweep-entry-cache-poisoning.md) | Cached a resolved entry under a key that didn't determine it; every sibling combo inherited it |
| [2026-07-22 heartbeat green through a wedge](2026-07-22-heartbeat-green-through-wedge.md) | Liveness measured the loop iterating, not work landing - 7 h outage, watchdog silent |
| [2026-07 chart swing + chain overlays removed](2026-07-chart-swing-and-chain-overlays-removed.md) | Two chart overlays deleted with the `swing_1` stack; geometry recorded in case they return |
