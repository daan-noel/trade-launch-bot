# Find the best rule for a fingerprint

**Input:** a fingerprint and a datetime range (one habit).
**Output:** one champion `RuleParams` for that slice.

Lab job that runs this method: [rule-search.md](rule-search.md).
Open replacement of §1–4 (habit portrait → signatures → sparse draft):
[rule-search-habit.md](../../roadmap/rule-search-habit.md).

Inputs are the registry, this range's tokens, and the scorer. Another rule's
params are not an input. Search the range you picked; do not hide a tail of it.

```
  fingerprint + datetime range
              │
              ▼
  1. metric paths for this cohort
              │
              ▼
  2. cut table
       labels: ran vs never-ran; dumpers
       entry: peak contrast + winner floor (primary)
              run-lead / launch / fill-moment (extra fillings)
       exit:  dump-lead + giveback-lead + after-dump + dumper p90
              + outcome held
       windows: winner burst + never-ran grind
              │
              ▼
  3. REGISTRY → entry roles and exit bags
       entry AND (0–2 can-fail, 0–1 trigger)
       exit OR (0–2 different metrics)
              │
              ▼
  4. every entry filling × every exit bag = one RuleParams
              │
              ▼
  5. score every combo; archive every full rule
              │
              ▼
  6. top 5 × unused exit cut (extra OR)
     top 3 × same-metric, same-phase neighbors (retune)
              │
              ▼
     champion = first ladder-robust rule by spread-discounted authority
                (paying, sign-agreeing replays; 1 s fill-delay gate)
              │
              ▼
     champion  vs  empty-entry  vs  incumbent
                   (buy everything)   (compare only)
```

**Score:** realized PnL, worst fill + `pumpfun_impact`, copycat guard ON. Same
buy size as the incumbent when one exists, else a fixed size for the run. The
candidate is always a complete rule, never a metric scored alone.

Report columns are `run_replay` (same kernel as Simulate). Fast archive is a
`CompiledRule` series walk. Job wiring: [rule-search.md](rule-search.md).

No metric is privileged. A new registry row joins by flags (`scope`, `monotonic`,
`kind`, `family`, unit). Unknown / `Standalone` is its own exclusive alternative
and may OR with other families.

## 1. Enter only when exit is not already true

`can_enter` = entry AND holds, and **no token-scoped exit already holds**.
Otherwise the next event after fill would sell.

```
  entry AND true?  ──no──► wait
         │ yes
         ▼
  any token-scoped exit already true?  ──yes──► wait
         │ no
         ▼
  BUY → after fill, any exit OR true?  ──► SELL
```

So `exit: buy(3s) > 10` is two clauses at once: before fill it is a wait-gate
(`buy(3s) <= 10` or NaN); after fill it is a sell. Several token-scoped exits
OR together: **any** one that already holds blocks entry. Adding an exit can
change who gets in, not only when you sell.

`m_position` (`retrace` / `bounce` / `pnl` / `held`) reads `NaN` before fill, so
it does not veto. `trail` as an exit can; `retrace` as an exit cannot.

Do not rank an exit bag on empty-entry before it meets an entry. A dump floor
that is already true at t=0 looks like a bad empty-entry rule because it vetoes
the buy; the same floor can pay once a selector times the fill.

## 2. Cut table

From this range's metric paths, before any combo is scored. Every registry
metric that is legal on that side gets a row. Windows are shared.

Labels are path facts, not a rule. **Ran** = ATH multiple at or above this
cohort's p67, floored at 1.5×. **Never-ran** = the rest. **Dumper** = price
leaves ATH by 15%+. A token can be ran and a dumper. Another rule's fills are
not a label.

A mixed quantile of everyone sits with the never-ran majority. The knife sits
in the **gap** between ran and never-ran, or in the **seconds before / after**
a labeled event (ATH break, first 1.5×).

Peak contrast is the primary entry clock. Run-lead, launch, and fill-moment
are extra fillings for the same metric — the generator keeps peak and at most
one extra, so an earlier clock can win without erasing the quiet one.

| Cut | Source |
| --- | --- |
| Windows | trade spacing; winner burst (0.25 × median TTP of ran); never-ran grind when it differs; near everyone's TTP. At most four sizes. |
| Entry contrast | median at peak (ATH neighborhood) of ran vs never-ran. Gap threshold; operator toward the ran side. |
| Entry winner floor | p10 of ran at peak, `>=`. |
| Entry winner ceil | p90 of ran at peak, `<=` — exists only to pair with the floor into a **band** (one can-fail filling), never a standalone single. |
| Entry run-lead | last 3 s **before** first 1.5× on ran (never-ran: median winner time-to-1.5×). |
| Entry launch | first print (within 2 s of create). Ran vs never-ran. |
| Entry fill-moment | snapshot at first 1.5× (losers: median winner time-to-1.5×). Time `Lt` is p75 of ran time-to-1.5×. |
| Entry fallback | early / peak p50 of everyone, only when the split is thinner than 8+8. |
| Exit dump-lead | last 3 s **before** ATH on ran dumpers (all dumpers if that set is thin). p50 and p90. |
| Exit giveback-lead | first 3 s **after** ATH, before the 15% dump, on ran dumpers (everyone's post-ATH lead if thin). p50 and p90. |
| Exit after-dump | rows after price has left ATH, ran dumpers only (everyone's after-dump if thin). |
| Exit dumper p90 | p90 of that after-dump set. |
| Exit held | p50 / p75 of (dump − fill-moment) on ran dumpers. Declared 60/120/300 only when that set is thin. |
| Position bounce / pnl / retrace | declared menu. |

A clause is `(metric, side, operator, threshold, window?)`. A curve fact (real
reserve tops near 85) appears only if this cohort's liquidity distribution
reaches it.

**Admission gate.** A cut joins the table only if it is false on at least 10%
of the cohort's sampled rows — a threshold at the metric's floor or ceiling
(`nonvol_buy >= 0`) selects nothing and is dropped before any combo is scored.
Cuts with no sampled column (position-scoped, declared menus) are kept; a
winner ceil rides its floor's gate (a ceiling above every row is rare-tail
insurance inside a band, not a selector).

Can-fail pairs that share an extra clock (run-lead / launch / fill-moment)
must co-occur on that clock on at least 8 ran tokens. Peak contrast pairs are
not gated on those snapshots.

Retune uses neighbors in this table only (nearby quantile, nearby window,
**same phase**). Retune does not add a metric or swap clocks.

## 3. Roles

Token-scoped metrics are legal on entry and on exit. `m_position` is exit-only
and optional. Entry ANDs; exit ORs.

```
  ENTRY (AND, sparse)                 EXIT (OR, sparse)
  0–2 can-fail                        0–2 clauses, different metrics
  0–1 trigger family                  empty exit is a combo
  empty entry is a combo
```

Can-fail is selector and extra pooled. Three can-fail ANDs are not a filling.

| Role | How it joins | Who may fill it |
| --- | --- | --- |
| Can-fail | 0–2, different metrics; a **band** (winner floor `>=` + winner ceil `<=` on ONE metric, one AND-arm) fills a single slot | selector (`time` / `liquidity` / other bounded token metrics) and extra (windowed flow / wallets / split floors) |
| Trigger | times the buy | one family: dip **or** rise **or** accumulation **or** organic **or** a new exclusive family — never two |
| Giveback | exit OR, compete | `trail` XOR `retrace` |
| Clock | exit OR, compete | `stall` XOR `held` |
| Progress | exit OR, stack | `pnl` / `bounce` / `rise` |
| Flow | exit OR, stack | any distinct flow or split metric |
| Wait-only | monotonic lifetime floor, no cap | not a selector |

**Compete** (at most one per combo): same metric; giveback pair; clock pair;
trigger family on entry.

**Stack** (may OR, subject to the 0–2 cap): two different metrics that are not a
compete pair. `buy(window)` and `nonvol_net` are two metrics.

Same quantity, different thesis by side: `trail` as a dip trigger is not `trail`
as a dump sell. Generate both from the cut table.

Same-window clauses on one dynamic group merge (`window_size_sec` unique per
group array).

There is no kitchen-sink entry and no greedy add/drop on entry. After the
entry × bag archive, extra-OR (dump-lead first) and same-phase retune may add
a candidate only if the full rule beats the archive.

Each `scale_out` stage is the same exit bag plus a size. This search scores
entry + the global exit.

## 4. Report

**Champion selection.** The fast archive ranks by trimmed SOL — the range
splits into four time blocks and the best block is subtracted, so an edge that
lives in one launch burst cannot buy the top slice. Among the replayed slice, a
candidate must pay AND agree in sign between authority and first-in-window;
those rank by spread-discounted authority (`authority − 0.25 × spread`). The
top four race a 1 s fill-delay replay, and the champion is the first that keeps
its sign — when none does, the run is flagged **latency-fragile**.

| Check | Meaning |
| --- | --- |
| Champion vs empty-entry (same exit bag) | If it does not beat buy-everything, the juice is ungated |
| n floor | Counted in distinct entered TOKENS (copycat-merged trades cluster); too few → no rule |
| PF > 1 under authority | |
| Expectancy floor | Mean realized SOL per closed trade ≥ 2× the round-trip cost at this buy size — total SOL alone can be trade count, not edge |
| Fill spread | Authority ranks after a 0.25 × spread discount; a sign disagreement between authority and first-in-window disqualifies the rule (score the next slice) |
| Latency ladder | Champion + top 3 replayed at 1 s fill delay; the champion keeps its sign there, and its 0/250/500/1000 ms decay curve prints on the board |
| Sibling z | Champion's fast SOL vs same-exit-bag siblings; below 1 standard deviation a Candidate downgrades to Ungated (selection noise) |
| Selective claim | enter% of matched, guard OFF, ≲ 60% — necessary, not sufficient |

| empty-entry | other combos | report |
| --- | --- | --- |
| loses | all lose | refuse |
| pays | all lose | juice is ungated |
| either | one pays | that filling is the candidate |

Refuse is a finished run. Paper the next launch burst; if the habit moved, pick
a new range and run again.

If the top archive slice has no paying, sign-agreeing replay, the board scores
the next slice. Diagnostics print the champion's cut phases (`contrast`,
`run-lead`, `launch`, `dump-lead`, `giveback-lead`, …); the board also carries
per-quartile PnL (a front-loaded row = the habit died mid-range), a per-clause
ablation table (champion minus each clause, authority replay — dead-weight and
harmful clauses become visible), and exit efficiency (realized over gross
attainable to each token's post-entry ATH — low says work the exit bag next).

## 5. Prove an update

A cut-table change is helpful only if it puts a **better full rule** on the
board. Coverage ("the clause exists") is not enough.

**Constructed corpus** (`cargo test -p hunter-lab --lib rule_search`, no lake).
Pass if the new cut lands in the gap / lead, and the mixed-everyone quantile
does not.

**Same-form ablation** on the Rule search page. Freeze fingerprint, range, buy,
fill, cost, copycat, incumbent. Grade one cut source per run.

| Grade | Pass |
| --- | --- |
| Beats incumbent | champion authority SOL > incumbent, n ≥ floor, PF > 1, beats empty-entry |
| Known miss | the new clock appears in the champion or top archive, and that row beats the incumbent |
| Harmless | champion not worse than the previous champion on this same form |
| Fail | n starves, ungated, or champion loses to both the previous champion and the incumbent |

Incumbent is the baseline to beat, not a shape to clone. Promoted g4 / g8 / g12
rules are the ablation incumbents. Do not hide a tail of the search range as
holdout; tokens after promote are the live test.
