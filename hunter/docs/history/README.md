# hunter - history

**Not a reading path.** Nothing in `CLAUDE.md` or `docs/arch/` links here, and nothing should:
this tier costs zero context until someone greps it deliberately.

What lives here: incidents and their RCAs, superseded approaches, and research journals - the
*how we got here*. The **rules** those produced live in `CLAUDE.md` (hot-path landmines),
`docs/arch/` (current behavior) and `docs/plans/` (deep-dive references). If a past fact still
changes what you do today - "runs stored before date X are priced wrong" - it belongs in the
present-tense tier, not here.

Entry shape for an incident: `# <what broke> (YYYY-MM-DD)` -> **Symptom** (with the numbers) ->
**Cause** (the mechanism) -> **Fix** -> **The rule this produced** (one line + where that rule
now lives). Entry shape for a research round: the question, the measurement, and the verdict,
including the refuted branches.

## Signal search - the numbered rounds

The standing brief, the gates and the settled list are in
[`@plans/strategies/signal-search-mandate.md`](../plans/strategies/signal-search-mandate.md).
These are the per-round records behind it.

| Round | One-line |
| --- | --- |
| [2 - stock and fresh wallets](2026-08-19-signal-round-2-stock-and-fresh-wallets.md) | First positive net in the program, and it turned out to be a bet on the tip |
| [3 - participation breadth](2026-08-19-signal-round-3-participation-breadth.md) | Breadth was a tie-break, not a signal; the fresh-wallet screen alone is the rule |
| [4 - the D->L->I chain](2026-08-19-signal-round-4-lull-impulse-chain.md) | A real timing signal (+7.70pp against a matched control) that does not clear the cost bar |
| [5 - the operator's two ideas](2026-08-19-signal-round-5-operator-ideas.md) | A large exit gain (silence-exit reduces to "hold 3-6 s"), and entry selection stays shut |
| [6 - the ix-pattern channel](2026-08-19-signal-round-6-ix-pattern-campaign.md) | Campaign mechanism confirmed, latency eats the confirmation, and the exit question closes |
| [7 - forward test and venue state](2026-08-20-round-7-fresh-wallet-forward-and-venue-state.md) | The rule fails on the fresh day; `P(arm)` is invariant to every venue-state variable |
| [8 - cost and entry depth](2026-08-20-round-8-cost-and-entry-depth.md) | 125 bps is immovable; sizing minimises cost percentage, not money; entry depth is a signal |
| [9 - entry depth forward](2026-08-20-round-9-entry-depth-forward.md) | The depth cut holds on a second later day, and both halves of the rule are required |
| [11 - the impulse-inception island](2026-08-21-island-search-refuted.md) | The island hypothesis pays after the exit bug is fixed: a one-slot buy impulse entered before the move |
| [10 - exhaustive combination search](2026-08-20-round-10-combination-search.md) | 17,744 cells and OR-portfolios, refuted by walk-forward: searching loses to not searching |

## Wallet studies and copy-trading

Surviving conclusions live in
[`@plans/strategies/wallet-analysis.md`](../plans/strategies/wallet-analysis.md).

| Entry | One-line |
| --- | --- |
| [Wallet research journal (07-21 -> 07-31)](wallet-research-2026-07.md) | The run-by-run reverse-engineering of the first four scalpers |
| [Wallet books were gross, not net](2026-08-18-wallet-books-were-gross-not-net.md) | 4 of 6 wallets do not clear the fee once 125 bps/leg is charged; two verdicts inverted |
| [`FBvx` intra-slot absorption](2026-08-20-wallet-fbvx-intra-slot-absorption.md) | Net positive 22/22 days, and the whole edge is a +6.15% gap that opens and closes inside one slot |
| [`3Xk2` momentum breakout](2026-08-18-wallet-3xk2-momentum-breakout.md) | Real and unreachable: +1 slot of latency costs 9.87% on his signal |
| [`8dtx` dip-turn clone](2026-08-17-wallet-8dtx-clone-refuted.md) | Mechanism confirmed under pessimistic fills; his edge is token selection and it is unreproducible |
| [`64hP` full study](2026-08-18-wallet-64hp-full-study.md) | Entry fully characterised and none of it transfers; bag-holding destroys 73% of gross |
| [Profitable-wallet mine](2026-08-03-wallet-copying-closed.md) | A 7-day mine of profitable wallets, negative even at zero latency - the class is closed |
| [The copy edge is the wallet's own price impact](2026-08-18-copy-edge-is-own-price-impact.md) | Their +18%/leg *is* `1+buy/pool`, priced before a copier can act |
| [The lull signature and the choice set](2026-08-18-choice-set-lull-signature.md) | What four scalpers watch: a slot-level buy impulse at S-1 that never pays universe-wide |
| [Intra-slot turn refuted](2026-08-18-intra-slot-turn-refuted.md) | The signal buys a top; execution is not the issue, and exits must be priced at +1 slot |
| [Dump-scalp execution gap](2026-08-16-dump-scalp-execution-gap.md) | The ~6pp loss is fill dispersion, not thresholds and not latency |

## Token, crew and fingerprint searches

| Entry | One-line |
| --- | --- |
| [Inverted token search](2026-08-18-inverted-token-search.md) | Starting from money rather than wallets: the whole observable universe is 1pp short of the fee |
| [The winning population, inverted search](2026-08-18-winner-population-inverted-search.md) | 599 daily-profitable wallets whose edge is entry price - they pick tokens worse than random |
| [Real traders after removing the dev crew](2026-08-18-real-traders-after-removing-dev-crew.md) | The removal is right and the +10% was a weighting artifact; per token the OOS blind hold is -8.23% |
| [Price-action and token-filter space refuted](2026-08-18-price-action-space-refuted.md) | All 256 cells negative in and out of sample; activity *is* the extraction |
| [Graduation runs and the identity layer](2026-08-18-graduation-and-identity-space.md) | The finish line is real but priced; identity predicts death, not success |
| [Crew-share filter, forward window](2026-08-18-crew-filter-forward-validation.md) | Selection confirmed, profit CI spans zero, and the per-entry trap bites a second time |
| [Creator-reputation + launch-crew screen](2026-08-03-creator-crew-screen-refuted.md) | The `p=0.0002` signal did not replicate; backer reputation came out inverted |
| [Launch-crew follower analysis](2026-07-29-launch-crew-refuted-oos.md) | Registry-copy + fixed-TP looked strong in sample and failed out of sample |
| [fp `5ix:Transfer 600K/160K`](2026-08-19-fp-5ix-transfer-600k-160k-refuted.md) | 42 SOL bundle, 26% graduate against a 35% break-even - priced against us |
| [fp `5ix cu_price=75210` exit sweep](2026-08-19-fp-75210-exit-sweep.md) | Entry selection is empty, one exit level pays 3.5pp and fails placebo, and the cohort is dying |
| [FP108-VET-1 refuted](2026-08-18-fp108-vet-1-refuted.md) | The +5.95%/trade was a veteran-roster leak plus unpriced impact |

## Incidents and RCAs

| Entry | One-line |
| --- | --- |
| [Trailing exits were hard stops](2026-08-21-trailing-exit-peak-bug.md) | A fancy-index `out=` no-op pinned the running peak at entry, so every trail in the island extract silently became a fixed stop |
| [`m_bundle` removed](2026-08-22-m-bundle-removed.md) | The launch-bundle group produced one refuted rule and kept charging an hourly launch-history sweep on the live box |
| [The island is a same-slot artifact](2026-08-22-island-is-a-same-slot-artifact.md) | 95% of the impulse island's money needs a fill inside a 10 ms same-slot gap; nothing in the space is positive at +100 ms |
| [ix-structure cuts](2026-08-22-ix-structure-cuts.md) | Blacklisting launcher / trigger / impulse-driver ix structures is worth +46% expectancy LODO — the one part of the island that holds up |


| Entry | One-line |
| --- | --- |
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
