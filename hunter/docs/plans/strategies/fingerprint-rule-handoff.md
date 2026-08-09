# Fingerprint + rule handoff (session continuity)

Permanent reference for continuing **metric + partial-exit rule** work in a new chat.
Written 2026-07-29 after DB mining, neutral dip analysis, and alignment on how the
user finds rules manually.

Related deep dives:

- Engine shape: [../../arch/strategies.md](../../arch/strategies.md)
- **BuyV2 playbook track:** [fp-buyv2-playbook.md](fp-buyv2-playbook.md)
- Metrics path runbook: [metrics-path-profitable-rules.md](metrics-path-profitable-rules.md)
- Metrics: [metrics-reference.md](metrics-reference.md)
- Costs (believe backtests only after reading): [execution-costs.md](execution-costs.md)
- Partial exits / `scale_out`: [../../roadmap/partial-exits-plan.md](partial-exits.md), `hunter/engine/src/rule_params.rs`
- Sweep / simulate authority: [../../arch/sweep.md](../../arch/sweep.md), [../sweep/sim-parity.md](../sweep/sim-parity.md)
- mx neutral proxy (not proof): [mx-metric-rules.md](mx-metric-rules.md)

---

## 1. What a fingerprint is (who / which playbook)

A **fingerprint** answers: *which tokens should this rule even consider?*

It is **not** an entry signal. It is a filter on **creation-time facts** (and optionally
first-slot settlement facts once `Full` match applies). Live and lab both use the same
matcher: `hunter_engine::fingerprint` + bucket rules in `grouping.rs`.

Typical axes the user clusters on manually:

| Axis | Meaning | Match style |
| --- | --- | --- |
| **`ix_labels`** | Ordered instruction label sequence on the **create** tx | Exact sequence (dev "recipe" / playbook) |
| **`init_buy` / `initial_buy_sol`** | Dev's initial buy size on creation | Often binned (e.g. [0, 6.4) SOL) in saved fingerprints |
| **`cu_limit`, `cu_price`** | Compute budget on create tx | Exact value |
| **`first_slot_buy` / `first_slot_sell`** | Buy/sell SOL in creation slot | Binned when present (`tokens_info` / lake cols) |

**User mental model (confirmed in chat):** each dev runs a repeatable **playbook**.
Tokens from the same playbook (especially same **`ix_labels`**) tend to share **similar
chart shapes** — e.g. one clean first swing, or swing then breakage then a dev-only
second pump. The fingerprint identifies the playbook; the **rule** is how you trade
that shape.

**Implications:**

- Reliable edge is expected from **several small rules**, each tied to a **validated
  creation fingerprint**, not one broad "all pump.fun dips" rule.
- **`ix_labels` is high signal but brittle**: one extra/missing ix in the create tx
  splits the cluster. Prefer starting from an exemplar mint's exact sequence, then
  widen only if fire rate is too low.
- **Graph shape is an outcome**, not a fingerprint field. You infer typical shapes *after*
  grouping by creation facts, then encode reactions via **metrics + exits** on the rule.

**Where fingerprints live:** DB `fingerprints` row + criteria JSON; rules reference
`fingerprint_id`. Grouped sweep can **scope corpus** by `fingerprint_id` (engine match
SSOT — see sweep doc).

Code touchpoints:

- Observed axes from PG/token row: `hunter/core/src/strategies/fingerprint_axes.rs`
- Engine match: `hunter/engine/src/fingerprint.rs`, `grouping.rs`

---

## 2. What a rule is (how / when to trade)

A **rule** = **`fingerprint_id`** + **`RuleParams`** (JSON): sizing, caps, optional
re-entry, **entry** metric conditions, **exit** conditions (TP/SL and/or metric exits),
and optional **`scale_out`** partial-exit ladder.

It answers: *given this playbook's tokens, when do I buy, how much, how do I leave?*

### Entry

- **`entry`**: AND groups of metric conditions (`m_snapshot`, `m_price_window`,
  `m_flow_window`, `m_price_lifetime`, etc.). Token-scoped metrics only on entry.
- Engine refuses entry if **exit conditions already hold** at the candidate row
  (`can_enter` / sweep Stage B) — avoids buying into an immediately-exitable trap state.

### Exit

- **`take_profit` / `stop_loss`** desugar into position PnL checks.
- **Metric exits**: e.g. `m_position.retrace`, `m_position.held`, armed trailing
  (`arm_above_pct` + retrace) — see [armed-trailing-stop.md](armed-trailing-stop.md).
- **`scale_out`**: ordered stages (sell bps at TP or metric trigger); remainder follows
  later exit reqs. Banks first-swing profit; tail managed separately.

### Re-entry

- Optional `reentry { cooldown_sec, max_episodes_per_token }`. Default profitable shape
  in recent analysis: **one-shot per token** unless the playbook clearly supports
  multiple swings. Re-entry often compounds tail loss on broad samples.

### Modes

- Rules can be `paper`, `is_active=false` for lab/sim first. Promote only after
  simulate + holdout validation.

**Not the goal of this track:** flow-scalper seeds that mimic a specific external wallet
(`fs*`, `63ot`, etc.). Those are a different calibration source
([wallet-analysis.md](wallet-analysis.md)). User wants **generic metrics + partial exit**
on **their** fingerprints.

---

## 3. Fingerprint vs rule (one table)

| Question | Fingerprint | Rule |
| --- | --- | --- |
| What varies dev to dev? | Create tx recipe, init buy, CU, first slot | N/A |
| What varies chart episode to episode? | N/A | Age, liq, trail, flow, PnL, held |
| Binds to mint at | TokenCreated (+ Full for first-slot axes) | Every trade/tick while armed |
| Typical mistake | Putting dip/trail thresholds in fingerprint | Using overly broad fingerprint so breakage playbooks fire |
| Live arming | Match once per token (two-phase if first-slot axes) | Evaluated on metric series until Done/Disarmed |

---

## 4. Adversarial dev behavior (must affect rule design)

User-described pattern (example mint:
`Fb6shLknTdApxiTmT4muVubHSxMM1HsWke1mQwVypump`):

1. First swing / volume as usual.
2. Price/liquidity collapses toward **creation floor** (rug / breakage).
3. Other traders exit at a loss or hold bags at the bottom.
4. Dev **only then** resumes volume-making toward migration floor — **not** while
   outsiders still have a cheap exit on the first breakdown.

**Trading consequence:** entries in the **breakage / near-floor** regime often see
**no dev pump**; dip-buy and "wait for second swing" remainder tranches get trapped.
Rules tuned on average dip-hot paths without playbook separation **overstate** edge.

**Design responses (try existing metrics first):**

- Tighter **liquidity band** on entry (stay out of near-creation-price zone).
- **Shallower trail** + **shorter hold** on remainder; prefer **scale_out** on first
  swing rather than betting on second pump.
- **No re-entry** on playbooks that show breakage traps.
- If simulate still fires in floor regime: candidate **new metric** — e.g.
  **liquidity drawdown** (current vs rolling max virtual SOL since creation) as entry
  veto. Spec only after failure mode is visible on per-token simulate rows.

---

## 5. User's manual workflow (target automation)

1. Cluster tokens by **create tx** (`ix_labels`, init buy, CU, …).
2. Inspect **typical charts** inside each cluster.
3. For clusters with a tradable **first swing** (or a known two-phase playbook), write
   entry metrics + exit/`scale_out` matched to that shape.
4. Paper/sim, adjust.

**Automated pipeline should mirror this order:** mine/scoped fingerprint → scoped corpus
→ search entry/exit → simulate → time-split validate → seed paper rules.

---

## 6. Plan to find actually profitable rules (metrics path)

**Full runbook (phases 0–5, commands, artifact map):**
[metrics-path-profitable-rules.md](metrics-path-profitable-rules.md).

**Definition of "profitable" for promotion:**

- Lab **`simulate_one_combo`** (authority), not Python proxy alone.
- Pricing: **`pumpfun_impact`**, **`worst`** fill, **`buy_amount_sol`** = intended live size.
- **Train/validate split by token `created_at`** (discovery Layer 3 in
  `hunter/lab/src/discovery/validate.rs`) — large train→validate drop = reject.
- Enough **fires** and acceptable tail (inspect worst mints per combo).

### Phase 0 — Playbook list

- SQL: `_local/rule-research/scripts/mine-playbook-clusters.sql`
  — group by `ix_labels` + init_buy bucket (same widths as neutral analyzer).
- Filter: min mint count, min median trades (tunable in script).
- Tag **breakage-trap** vs **first-swing-only** families; save winner via
  `_local/rule-research/scripts/seed-fp-playbook-from-mint.sql`.

### Phase 1 — Corpus per fingerprint

```powershell
cargo run -p hunter-lab -- lake-export
```

- Grouped sweep / simulate with **`fingerprint_id`** scope (engine `matches`).

### Phase 2 — Search

- **Discovery:** lab `POST /api/strategies/metric-discovery` (Layer 1–3) scoped by
  `fingerprint_id` → **Open as sweep** seed ([sweep arch § discovery](../../arch/sweep.md)).
- **Grouped sweep:** entry grids (age, liq, trail, gross_flow, …) with `off` per axis.
- **Exit:** hand **`scale_out`** ladder (discovery does not grid partial exits yet) vs
  full TP/SL; default **no re-entry**.
- Sweep/discovery **ranks**; top combos go to Phase 3 (**simulate** is PnL authority).

### Phase 3 — Validate

- Re-simulate winners on full scope + validate slice.
- Per-token drill-in on worst losses (breakage traps).

### Phase 4 — Guards

- Existing metric vetoes first; then consider **liquidity drawdown** if needed.

### Phase 5 — Live

- Seed `paper` / inactive rules; small size; compare live slippage to worst-case sim.

---

## 7. Work already done (repo)

| Artifact | Purpose | Trust level |
| --- | --- | --- |
| [metrics-path-profitable-rules.md](metrics-path-profitable-rules.md) | Phases 0–5 runbook (authority sim, discovery, seeds) | Operator guide |
| `_local/rule-research/scripts/mine-playbook-clusters.sql` | Phase 0 ix_labels + init_buy clusters | PG mining |
| `_local/rule-research/scripts/seed-fp-playbook-from-mint.sql` | Save fingerprint from exemplar mint | Idempotent insert |
| `_local/rule-research/scripts/analyze_dip_hot_neutral.py` | Neutral dip-hot proxy on local PG | Relative ranking only |
| [mx-metric-rules.md](mx-metric-rules.md) | Decisions from proxy (init_buy `_local/rule-research/scripts/seed-mx-metric-rules.sql` | Seed `mx-*` paper rules | **Not applied** unless user runs SQL |
| [fs5-practice-rules.md](fs5-practice-rules.md) | 63ot-local-DB practice ladder | Separate from mx track |
| Scratch SQL under `scripts/_scratch-*` | Ephemeral 63ot analysis | Disposable |

**Neutral proxy headline (07-22..07-28, 220 tokens):** market-wide dip-hot with re-entry
was negative mean; **init_buy [0,6.4)** + **scale_out** + **one-shot** looked best *in
proxy* — still requires lake + lab simulate.

---

## 8. Open inputs for next session

Collect from user if not already set:

- Study **date window** (extend beyond 07-28?).
- **One target playbook**: exemplar mint or exact `ix_labels` from `tokens` for a
  "good" first-swing family.
- **Adversarial exemplar**: confirm `Fb6shLknTdApxiTmT4muVubHSxMM1HsWke1mQwVypump`
  creation row in local PG (`instruction_labels` / fingerprint axes).
- **Live buy size** and **max_concurrent** for sim parity.
- **`scale_out` shape** preferences if deviating from mx draft (8500 bps @ 17% TP,
  armed trail on remainder).
- Whether to **apply** `seed-mx-metric-rules.sql` or only fingerprint-scoped `fp-*` seeds.

---

## 9. Suggested first commands (next session)

```powershell
# 1) Phase 0 clusters + exemplar mints
psql "$env:DATABASE_URL" -f hunter/scripts/mine-playbook-clusters.sql

# 2) Save fingerprint (edit mint + name inside SQL first)
psql "$env:DATABASE_URL" -f hunter/scripts/seed-fp-playbook-from-mint.sql

# 3) Export lake for sim window
cargo run -p hunter-lab -- lake-export

# 4) Lab: metric-discovery or grouped sweep scoped by fingerprint_id
#    then simulate — pumpfun_impact, worst fill, buy_amount_sol = live notional

# 5) Optional hypothesis seeds: seed-mx-metric-rules.sql
```

---

## 10. Glossary (short)

- **Playbook:** dev's repeated create-tx recipe; approximated by fingerprint axes.
- **One-shot:** no `reentry` key — at most one episode per (rule, mint) unless manual.
- **Proxy backtest:** Python reserve-walk — fast, not engine-parity.
- **Authority backtest:** `hunter-engine` fold via lab simulate / `simulate_one_combo`.
