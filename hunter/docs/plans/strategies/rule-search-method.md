# Find the best rule for a fingerprint

**Input:** a fingerprint and a datetime range (one habit).
**Output:** one champion `RuleParams` for that slice.

Lab job that runs this method: [rule-search.md](rule-search.md).

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
       label paths (ran vs never-ran; dumpers)
       entry: contrast + winner floor (mixed peak if the split is thin)
       exit:  dump-lead + runner after-dump + dumper p90
       windows: cohort clocks, plus winner burst
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
  6. top 3 archive rules × each unused exit cut
       (one extra OR; any exit-legal metric)
              │
              ▼
     champion = archive max, then run_replay
              │
              ▼
     champion  vs  empty-entry  vs  incumbent
                   (buy everything)   (compare only)
```

**Score:** realized PnL, worst fill + `pumpfun_impact`, copycat guard ON. Same
buy size as the incumbent when one exists, else a fixed size for the run. The
candidate is always a complete rule, never a metric scored alone.

The archive inner loop is a `CompiledRule` series walk (shared entry across exit
bags, copycat merge after). Report columns (champion, empty-entry, incumbent)
are `run_replay` — the same kernel as Simulate. How that job is wired:
[rule-search.md](rule-search.md).

No metric is privileged. A new registry row joins by flags (`scope`, `monotonic`,
`kind`, `family`, unit). Unknown / `Standalone` is its own exclusive alternative
and may OR with other families.

## 1. Enter only when exit is not already true

`can_enter` = entry AND holds, and **no token-scoped exit already holds**.
Otherwise the next event after fill would sell. This is the rule, not a side
effect.

```
  want to buy now?

  entry AND true?  ──no──► wait
         │ yes
         ▼
  any token-scoped exit already true?  ──yes──► wait (do not buy)
         │ no
         ▼
  BUY
         │
         ▼
  after fill: any exit OR true?  ──► SELL
```

So `exit: buy(3s) > 10` is two clauses at once:

| when | meaning |
| --- | --- |
| before fill | enter only while `buy(3s) <= 10` (or NaN) |
| after fill | sell when `buy(3s) > 10` |

Several token-scoped exits OR together: **any** one that is already true blocks
entry. Adding an exit can change who gets in, not only when you sell.

`m_position` (`retrace` / `bounce` / `pnl` / `held`) reads `NaN` before fill, so
it does **not** veto. `trail` as an exit can; `retrace` as an exit cannot. That
is one reason they are different fillings, not two names for the same stop.

Entry and exit are not independent searches. Score the full `RuleParams`.

Do not rank an exit bag on empty-entry before it meets an entry. A dump floor
that is already true at t=0 looks like a bad empty-entry rule because it vetoes
the buy; the same floor can pay once a selector times the fill.

## 2. Cut table

From this range's metric paths, before any combo is scored. Every registry
metric that is legal on that side gets a row. Windows are shared.

Labels are path facts, not a rule. **Ran** = ATH multiple at or above this
cohort's p67, floored at 1.5×. **Never-ran** = the rest. **Dumper** = price
leaves ATH by 15%+. A token can be ran and a dumper (the usual meme). Another
rule's fills are not a label.

A mixed quantile of everyone sits with the never-ran majority. The knife that
changes who gets in or when you sell sits in the **gap** between ran and
never-ran, or in the **seconds before ATH breaks** on dumpers.

| Cut | Source |
| --- | --- |
| Windows | trade spacing; time-to-peak of everyone; winner burst (median TTP of ran tokens). Three to four sizes. |
| Entry contrast | per metric, median at peak of ran vs never-ran. If they differ, one threshold in the gap; operator toward the ran side (`>=` if ran is higher, `<=` if ran is lower). |
| Entry winner floor | p10 of ran tokens at peak, `>=`. A floor winners fail is not on the menu. |
| Entry fallback | early / peak p50 of everyone, only when the ran/never-ran split is thinner than 8+8. |
| Exit dump-lead | last 3 s **before** ATH on ran dumpers (all dumpers if that set is thin). p50 and p90. This is a lead, not after-dump. |
| Exit after-dump | rows after price has left ATH, **ran dumpers only** (everyone's after-dump if that set is thin). |
| Exit dumper p90 | p90 of that after-dump set, not p90 of every row. |
| Position exits | declared menu (`retrace` / `bounce` / `held` / `pnl`). |

A clause is `(metric, side, operator, threshold, window?)`. Threshold and window
come from this table. A curve fact (real reserve tops near 85) appears only if
this cohort's liquidity distribution reaches it — one candidate, not a required
cell.

Retune after a combo wins uses neighbors in this table only (nearby quantile,
nearby window). Retune does not add a metric.

## 3. Roles

A metric is a quantity. Side + operator + cut make the clause. Token-scoped
metrics are legal on entry and on exit. `m_position` is exit-only and optional.

Entry ANDs; exit ORs.

```
  ENTRY (AND, sparse)                 EXIT (OR, sparse)
  0–2 can-fail                        0–2 clauses, different metrics
  0–1 trigger family                  empty exit is a combo
  empty entry is a combo
```

Can-fail is selector and extra pooled: two independent facts (`time` AND `liq`,
or `liq` AND `wallets`). Trigger stays one family. Three can-fail ANDs are not
a filling — that is kitchen-sink.

| Role | How it joins | Who may fill it |
| --- | --- | --- |
| Can-fail | 0–2, different metrics | selector (`time` / `liquidity` / other bounded token metrics) and extra (windowed flow / wallets / split floors) |
| Trigger | times the buy | one family: dip **or** rise **or** accumulation **or** organic **or** a new exclusive family — never two |
| Giveback | exit OR, compete | `trail` XOR `retrace` (token ATH vs since-entry peak) |
| Clock | exit OR, compete | `stall` XOR `held` (token quiet vs our fill) |
| Progress | exit OR, stack | `pnl` / `bounce` / `rise` — different metrics may OR |
| Flow | exit OR, stack | any distinct flow or split metric (lifetime or windowed) — different metrics may OR |
| Wait-only | monotonic lifetime floor, no cap | not a selector |

**Compete** (at most one per combo): same metric (one threshold); giveback pair;
clock pair; trigger family on entry.

**Stack** (may OR, subject to the 0–2 cap): two different metrics that are not a
compete pair. Flow is not one slot. `buy(window)` and `nonvol_net` are two
metrics; they may share an exit bag. A new metric with an unknown family stacks
with the others.

Same quantity, different thesis by side: `trail` as a dip trigger is not `trail`
as a dump sell. Generate both from the cut table; do not copy one onto the other.

Same-window clauses on one dynamic group merge (`window_size_sec` unique per
group array).

There is no kitchen-sink entry and no greedy add/drop on entry. Entry fillings
are the role product. Champion is the archive max, not the end of a path that
added or dropped one clause at a time.

After every entry × bag is scored, take the top 3 full rules and try each
**unused** exit cut as one extra OR. Any exit-legal metric is a candidate,
including `m_position`. Re-simulate the whole rule. The extra clause stays only
if that full rule beats what is already in the archive.

Each `scale_out` stage is the same exit bag plus a size. This search scores
entry + the global exit; staged scale-out is the same generator on a third bag.

## 4. Report

| Check | Meaning |
| --- | --- |
| Champion vs empty-entry (same exit bag) | If it does not beat buy-everything, the juice is ungated |
| n floor | Too few closed trades → no rule |
| PF > 1 under authority | |
| Fill spread (optimistic / authority) | Quote next to every SOL number |
| Selective claim | enter% of matched, guard OFF, ≲ 60% — necessary, not sufficient |
| Empty entry / no selector | latency ladder; a 1 s entry floor must still pay |
| Exit bag | a useful champion may carry two different-metric exits; a one-clause bag is not a required shape |

| empty-entry | other combos | report |
| --- | --- | --- |
| loses | all lose | refuse |
| pays | all lose | juice is ungated |
| either | one pays | that filling is the candidate |

Refuse is a valid result. Paper the next launch burst; if the habit moved, pick
a new range and run again.

## 5. Prove an update

A cut-table change is helpful only if it puts a **better full rule** on the
board. Coverage ("the clause exists") is not enough. Grade in this order:

**Constructed corpus** (`cargo test -p hunter-lab rule_search`, no lake). Build
tokens whose ran vs never-ran (or dump-lead vs after-dump) distributions are
known. Pass if the new cut lands in the gap / lead, and the mixed-everyone
quantile does not. This is how a source earns a row in §2.

**Same-form ablation** on the Rule search page. Freeze fingerprint, datetime
range, buy, fill, cost, copycat, incumbent. Run, save the three columns. Change
one cut source (or the can-fail cap). Re-run the same form.

| Grade | Pass |
| --- | --- |
| Beats incumbent | champion authority SOL > incumbent, n ≥ floor, PF > 1, beats empty-entry |
| Known miss | dump-lead (or stacked exit) appears in the champion or top archive, and that row beats the incumbent |
| Harmless | champion not worse than the previous champion on this same form |
| Fail | n starves, ungated, or champion loses to both the previous champion and the incumbent |

Incumbent is the baseline to beat, not a shape to clone. g4 / g8 / g12 promoted
rules are the ablation incumbents; a new fingerprint is green on the same
verdict table with no incumbent.

Do not hide a tail of the search range as holdout. Next week's tokens after
promote are the live test. One update per ablation — five sources at once
cannot say which one paid.

The board prints the champion's cut phases in diagnostics (`contrast`,
`dump-lead`, `winner-floor`, …) so a page run shows which source fired.
