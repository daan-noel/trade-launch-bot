# Rule search — habit portrait, then a draft

Open replacement of the cut-table × role product in
[rule-search-method.md](../plans/strategies/rule-search-method.md) §1–4.
Job wiring stays [rule-search.md](../plans/strategies/rule-search.md). Scorer,
fill, cost, copycat, and `run_replay` stay.

**Input:** one fingerprint + one datetime range. Scope the cohort through
`hunter_engine::fingerprint::matches` — the same SSOT simulate uses — never a
hand-rolled predicate. A fingerprint matches on **every** non-null axis, not just
`ix_labels`: `cu_limit`, `cu_price`, `init_buy_lamports`, `max_cost_lamports`,
`spendable_lamports_in`, the two first-slot axes, and `bucket_size_amount` — where
a NULL bucket means an **exact** compare, not an unset one. An ix-labels-only
approximation of `3ix:BuyExactSolIn · spend=5 · bkt=exact` took 3,440 tokens where
the engine took 264 (that label cohort spreads over 12 distinct
`spendable_lamports_in` values; the fingerprint's is 7.7% of it), and the ranking of
two rules **inverted** between the two populations: a search on an approximate
cohort ranks rules for a creator set that does not exist. Cheapest guard —
`n_matched` against a hand count on every run.
**Output:** a habit portrait and one sparse `RuleParams` **draft** the operator
can add/drop/nudge. Empty-entry and incumbent stay comparison columns. The draft
is never empty-entry.

A fingerprint is a launch style. A habit is how that style unfolds in time. A
rule is a bet on that unfolding. A metric belongs in the draft only if it names
a **phase event** on this cohort's paths.

Fold this file into `rule-search-method.md` when the job runs it; then delete
this roadmap entry.

---

## Contract

| | Rule |
| --- | --- |
| Clock | Age from `created_at`. Entry contrast is cross-section at the same age. Exit contrast is relative to that token's ATH (dump is per-token). |
| Time | The index of the habit, not a peak-sampled cut. A draft carries `time in (t_lo, t_hi)` with `t_lo >= 2 s` (p25 of `t15_ran` is sub-second on real cohorts, so an unfloored band swallows the create slot) — **unless a state clause already acts as the clock**: the promoted g4 enters on `liquidity > 20` with no time band, because reaching that liquidity *is* the timing. Mandating the band on every draft is not supported. |
| Regime | A draft is valid only for the range it is fitted on. The board carries a **prior-period column**: the frozen draft replayed on the preceding equal-length window. Paying there too = durable; not = regime-scoped, expect decay and re-search per range. |
| Metrics | Registry is the catalog. A row earns a clause only via a phase signature (below). Unique-wallets is not an entry gate ([flow-scalper-findings.md](../plans/strategies/flow-scalper-findings.md)). Lifetime monotonic floors are wait-only, not selectors. `stall` is not an entry clause and not a time stop (`held` is). |
| Draft shape | Time band AND 0–1 state band AND 0–1 start event; exit OR of 1–2 end events. No third entry AND. |
| Empty-entry | Scored for the Ungated / Candidate verdict only. Never the draft. |
| Incumbent | Compare only. Never a seed. |
| Create-slot | If the move is already gone in the first 2 s, refuse — no draft. Template: [maxbuy-launcher-fingerprint.md](../plans/strategies/maxbuy-launcher-fingerprint.md). The create-slot move itself is a first-fill race: positive at next-print fill, negative one print later — never a rule-search product. |
| Buy snapshots | Launch (first 2 s) and fill-moment (first 1.5×) are portrait diagnostics, not draft clocks **and not entry clauses** — first-2 s gates select nothing profitable at one-print fill delay. |

Labels (path facts, not a rule): **ran** = ATH multiple ≥ max(cohort p67, 1.5×);
**never-ran** = the rest; **dumper** = price leaves ATH by 15%+. A token can be
ran and a dumper.

---

## Pipeline

```
  fingerprint + datetime range
              │
              ▼
  1. Timeline          clock of this habit
  2. Age-aligned paths future-ran vs future-never-ran at the same t
  3. Signatures        which metrics turn on in which phase
  4. Theses            2–4 full rules from those signatures
  5. Score             same simulate as now; retune on the same dials
              │
              ▼
     portrait + draft (+ unused signatures as add/drop)
     vs empty-entry vs incumbent
```

### 1. Timeline

From this range's matched tokens (after the token cap):

| Stat | Source |
| --- | --- |
| `ttp_ran` | seconds, create → ATH, ran only |
| `t15_ran` | seconds, create → first 1.5× first-print, ran that hit 1.5× |
| `dump_lag_ran` | seconds, ATH → 15% giveback, ran dumpers |
| `spacing` | median inter-trade seconds |
| `create_markup` | max(price in first 2 s) / first price, per token |

Trade window:

- `t_lo` = p25 of `t15_ran` (if that set < 8: 0.2 × p25 of `ttp_ran`)
- `t_hi` = p75 of `ttp_ran`
- if `t_lo >= t_hi`: `t_lo` = p10 of `ttp_ran`, `t_hi` = p50 of `ttp_ran`
- round both to 1 s; require `t_hi - t_lo >= 2`

Windows for flow/price groups (at most four, from this clock):

- short = median `spacing`, floored at 2 s
- burst = 0.25 × median `ttp_ran`
- mid = 0.6 × median `ttp_ran`
- grind = 0.25 × median TTP of never-ran, only if it differs from burst and mid by ≥ 1 s

**Create-slot refuse** (no draft):

```
share = median_ran(create_markup − 1) / median_ran(ath_multiple − 1)
refuse if share ≥ 0.60
```

The first 2 s already hold most of the typical run; there is no tradable climb.

Phases on the **cohort clock** (age from create):

| Phase | Age |
| --- | --- |
| setup | `[0, t_lo)` |
| climb | `[t_lo, t_hi)` |
| peak-desc | `[0.7 × p50 ttp_ran, p75 ttp_ran]` — description only, not a buy clock |
| dump-lead | last 3 s before **that token's** ATH, ran dumpers |
| giveback-lead | first 3 s after that ATH, before the 15% dump |

### 2. Age-aligned contrast

For each token, `age = trade.block_time − created_at`. The ran / never-ran
label is known only at the end of the path. Features at age `t` must not use
prices after `t`.

**Entry (cross-section).** At each 1 s bucket `t` in `[t_lo, t_hi)`, among
tokens that have a print in that bucket, contrast on every legal registry metric
(the windows from §1). Need ≥ 8 tokens per side (`MIN_SPLIT`). The entry-side
label is **remaining-move at `t`**: `max(spot after t) / spot(t) >= 1.5` — not
the path-level ran label. Path-ran tokens hold most of their multiple *before*
`t` (at age 10 s, ~58% of print-active tokens are path-ran but only ~22% still
have 1.5× ahead), so path-labelled signatures select moves already spent. The
path label stays for the §1 clock and the create-slot share only.

**Exit (event-aligned).** Dump-lead / giveback-lead / after-dump stay relative
to each token's ATH. Held duration = (dump − first 1.5×) on ran dumpers; if that
set is thin, declared 60 / 120 / 300 s.

Do not take entry thresholds from values at each token's own ATH. That compares
two finished moves of different size.

### 3. Signatures

A metric **earns** a clause only if both hold (15% relative gap, same scale as
today's contrast; admission: the clause is false on ≥ 10% of sampled rows).

**Turns on (longitudinal, ran only).** Median in climb differs from median in
setup by ≥ 15% (entry), or dump-lead / giveback-lead differs from climb by ≥ 15%
(exit).

**Splits (cross-section).** At some climb bucket, future-ran vs future-never-ran
differs by ≥ 15%, operator toward the ran side.

| Role | Who may earn | Clause |
| --- | --- | --- |
| Clock (always) | `time` | band `(t_lo, t_hi)` — not from a contrast test |
| State | `liquidity` | band: p10 ran at climb ≥, p90 ran at climb ≤ |
| Start event (one family) | windowed `gross_flow` / `buy`; organic `untagged_buy` / `untagged_net`; dip `trail`; rise `rise` | floor at a rung of ran in climb, window from §1 — **or a band**, when the ran distribution is non-monotone in outcome (its top decile scores worse than its middle). A floor cannot express "enough crowd, but not bot-flood"; without the band that habit is invisible rather than absent. Prefer organic or flow over rise. A rise-only thesis carries a latency-risk flag. |
| End event | dump-lead or giveback of flow / organic / `trail`; `held` | p25 / p50 / p75 of that event set (`>=`) |

One start family per thesis (dip XOR rise XOR accumulation XOR organic). Giveback
is `trail` XOR `retrace`. Clock exit is `held` (not `stall`).

Metrics with no signature do not join the menu. Peak-desc medians may print on
the portrait as "what winners look like at the top"; they do not author clauses.

Threshold grid for earned metrics: **p25, p50, p75, p90** of the earning sample
(plus the liq p10/p90 band). The paying region sits at the selective end — a
grid stopping at p75 misses it. Neighbor retune stays inside that grid and the
same phase. A value that sits between two rungs can exist.

### 4. Theses (not a kitchen product)

Each thesis is a full `RuleParams`. Enumerate only:

**Entry** (AND), always including the time band:

1. `time in (t_lo, t_hi)`
2. time + liq band (if liq earned)
3. time + one start event (each earned start family × each of its p25/p50/p75)
4. time + liq + one start event (same)

**Exit** (OR) is the **primary search axis, not the tail of the entry search.**
The promoted incumbents carry a trivial entry (g4 is `liquidity > 20`, no time
band) and a four-term exit; on that cohort every exit term *alone* loses 30–50%
per trade while their OR pays, because an OR fires at the earliest alarm and the
edge is getting out fast on any of several independent signals. So: 2–4 terms,
drawn from flow / organic / `stall` / liquidity-ceiling. A price trail is the
weakest term available — it loses on entries the OR makes profitable — and never
stands alone. Empty exit is a diagnostic, not a draft.

Cap: a few dozen full rules, not the registry product. Extra-OR after scoring
may add one unused **earned** end event to the top 5 theses if the full rule
beats the archive. Retune: top 3 × same-metric same-phase neighbors in the
p25/p50/p75 grid.

The draft is the best-scoring thesis that has the time band (every thesis does).
If none pay: verdict Refuse, still show the time-band thesis as the draft to
adjust. If empty-entry beats every thesis: verdict Ungated, draft stays the
best selector thesis (not empty-entry).

### 5. Score

Unchanged authority: realized PnL, worst fill + `pumpfun_impact`, copycat ON,
same buy as the incumbent when one exists. Report columns are `run_replay`.
Selection is **expectancy plus day-block sign agreement** — train-half
expectancy margins between neighbor rungs are noise-thin, and day-sign is what
separates a real rung from a lucky one. Expectancy floor, token-n floor,
latency ladder, sibling z, ablation, quartile PnL, exit efficiency stay as
**board diagnostics**. They do not replace a paying thesis with a
different-metric combo. The drift check (early-half vs late-half clock) needs
≥ 30 ran per half before it may claim drift.

Three gates decide whether a thesis is reportable at all, and they run **before**
the expensive scorer:

1. **Fill-timing sensitivity.** Re-price the thesis at entry+exit fill delays of
   0 / 0.2 / 0.5 / 1.0 s. A thesis whose sign flips inside that band is not a
   draft — it is priced on a microstructure artifact, and live latency is not
   controllable to 200 ms. Only a thesis positive across the whole band is
   reportable.
2. **Avoidance vs profit, scored apart.** Two questions, two verdicts: does the
   gate beat the *ungated* same-band control (avoidance), and does it beat zero
   (profit)? A gate can be a stable, valuable filter — reliably turning a −9%
   band into a flat one — while never paying on its own. Report both; never let
   a strong avoidance number read as an edge.
3. **Walk-forward, not split-half.** A regime boundary can land anywhere, so a
   median split can hand one half the entire pocket. Fit on trailing K days,
   trade day K+1 frozen, roll. The equity of that process is the tool's real
   score.

---

## Portrait (what the board must show)

The portrait is the product; the draft is its executable form. So the portrait is
**prose, not a dashboard** — five plain-language answers, each one sentence, in
creator terms. A metric name may appear only after the sentence that explains it.
Numbers back the sentences up and stay collapsed until asked for.

| Question | The answer says |
| --- | --- |
| What does this creator do? | how many of his tokens run, how fast they peak, how fast they dump |
| What separates his winners from his duds? | the one earned signature, in words ("winners draw 25+ buyers in 3 s; duds never do") |
| What is the rule? | wait / buy / sell, in seconds and plain thresholds |
| Can I trust it? | separately: as a filter (vs the ungated band) and as a money-maker (vs zero), each naming the windows it holds in |
| Is he changing? | how the portrait's **own numbers** moved across the range — ran share, time-to-peak, the earned threshold. A habit is a sequence of portraits; one portrait is a photograph of it. |
| What can I change? | the unused signatures, as add/drop, each with its own one-line reason |

A refusal is the same shape and shorter: the verdict, then one sentence of why.

Backing detail, collapsed: clock (`t_lo`/`t_hi`, `ttp_ran`, `create_markup`
share), earned + unused signatures with phase/direction/threshold/window, the
per-window rows (fit / holdout / prior-period, per-day PnL sign — never one
pooled number), the fill ladder (0 / 0.2 / 0.5 / 1.0 s), and draft /
empty-entry / incumbent columns.

Actions: Open in editor (RuleEditor, no save required) · Simulate · Promote.

Round thresholds with the existing `round_for_unit`.

---

## Constructed tests (`cargo test -p hunter-lab --lib rule_search`)

Pass without the lake. Same bar as method §5: a new source lands in the gap /
lead, the mixed-everyone quantile does not.

- [ ] Time band equals `(max(2, p25 t15_ran), p75 ttp_ran)`, not `time < p75 at ATH`.
- [ ] Entry samples labelled by remaining move at `t`: a token at 1.6× of its
      eventual 1.7× ATH at age `t` is a negative sample, however the path ends.
- [ ] Prior-period column: the frozen draft replays on the preceding
      equal-length window and the verdict names durable vs regime-scoped.
- [ ] A thesis positive at one fill delay and negative at another inside
      0–1 s is withheld, not drafted.
- [ ] Walk-forward equity (fit trailing K, trade day K+1) is the reported
      score; a median split that puts one regime in one half does not pass.
- [ ] Age-aligned climb split on liq (ran ~80 vs never-ran ~15 at the same age)
      earns a liq band; sampling those same tokens at each token's ATH is not
      the entry knife.
- [ ] Create-slot share ≥ 0.60 refuses; no draft.
- [ ] A metric that only differs at ATH (crowd / rise leftover) does not earn
      an entry clause.
- [ ] Organic that turns on in climb and at dump-lead can be a start event on
      entry and an end event on exit (same quantity, different side).
- [ ] Draft always contains the time band; empty-entry is never `champion`.
- [ ] `run_replay` of the draft equals HTTP Simulate on the same fingerprint,
      range, fill, cost, guard, buy.

Same-form ablation on the page (method §5) still grades a change: freeze
fingerprint, range, buy, fill, cost, copycat, incumbent; promoted g4 / g8 / g12
are the incumbents to beat. Coverage ("the clause exists") is not a pass.

---

## Robustness gates — still unproven on real cohorts

The mechanisms (latency ladder, spread gate, trimmed block ranking, sibling z,
admission gate, entry bands, expectancy floor, token-n floor, ablation / quartile /
exit-efficiency diagnostics) are implemented and documented in
[rule-search-method.md](../plans/strategies/rule-search-method.md) §2-§4. What is open
is proving them:

- Grade each with the method's §5 same-form ablation: freeze fingerprint, range, buy,
  fill, cost, copycat, incumbent; one cut source / gate per run.
- Re-run the fingerprints the pilots killed for latency or spread (g2, g6, g8 v1, g12)
  and confirm the gates refuse or downgrade them with no operator reading numbers.
- Watch whether the 10% admission share and the 0.25 spread discount need tuning -
  both are constants in `hunter-lab`'s `rule_search` (`cuts.rs`, `report.rs`), chosen
  from pilot history, not fitted.

## Land slices

- [ ] **Portrait** — timeline + age-aligned samples + create-slot refuse; print
      on the report even if theses still use the old generator.
- [ ] **Signatures** — earn/drop metrics from §3; cut table becomes the earned
      menu (time band always on it).
- [ ] **Theses** — replace entry × exit cartesian product with §4; empty-entry
      never the draft.
- [ ] **Board** — portrait, draft checkboxes, unused signatures, Open in editor
      without Promote.
- [ ] Fold into [rule-search-method.md](../plans/strategies/rule-search-method.md)
      and [rule-search.md](../plans/strategies/rule-search.md); delete this file.

Do not add more ranking gates or more AND slots to "find" a habit. The portrait
has to name it first.
