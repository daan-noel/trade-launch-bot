# fp-bv2 — BuyV2 playbook (metric + fingerprint track)

**Playbook** = exact creation `ix_labels` (one dev recipe), not one mint:

```json
[
  "Compute Budget: SetComputeUnitLimit",
  "Compute Budget: SetComputeUnitPrice",
  "Pump.Fun: Create_v2",
  "Associated Token: CreateIdempotent",
  "Pump.Fun: BuyV2"
]
```

Authority path: [metrics-path-profitable-rules.md](metrics-path-profitable-rules.md). This doc
is the **playbook-specific** instance of that plan.

---

## Cohort bias (local PG, 2026-07-22 .. 2026-07-28)

Source: `_local/rule-research/scripts/analyze-buyv2-playbook.sql`.

| Fact | Value | Implication |
| --- | --- | --- |
| Mints in window | **18,920** | Very common template (mostly low-activity launches) |
| Median trades / mint | **4** | Most rows are noise for dip-hot sim |
| Mints with ≥200 trades | **1,471** (~7.8%) | Where fold cost and edge estimates are meaningful |
| `% dead` (small init buy) | **~77%** in `_local/rule-research/scripts/seed-fp-buyv2-playbook.sql`.
Rules prefix **`fp-bv2-*`** (paper, inactive).

**Promotion rule:** prefer a fingerprint that **raises median activity** (e.g. `_local/rule-research/scripts/buyv2-playbook-ladder.ps1`.
3. **Sim bar:** `cost_model=pumpfun_impact`, `fill_model=worst`, profit factor &gt; 1,
   validate slice not `Failed` / `Degraded`.

---

## Operator defaults (unless you say otherwise)

| Knob | Default | Why |
| --- | --- | --- |
| Date window | 2026-07-22 .. 2026-07-28 | Matches existing lake + prior mx work |
| Buy size | 0.10 SOL | 2–3 SOL bankroll / conc 4 |
| Re-entry | **Off** (one-shot) | Neutral proxy + playbook spam |
| Entry family | Dip-hot (age, liq band, trail, gross60) | Starting grid from mx-00 |

---

## Open choices (tell me if you want these changed)

1. **Corpus cap:** discovery/sim on **all 18.9k** matches vs **`token_cap` 5000** (newest) vs
   **custom min trade_count** (needs a PG pre-filter export — not in engine today).
2. **Date window:** extend through today (`lake-export --include-today` + sync)?
3. **Live bankroll / buy SOL** if not 0.10.
4. **Acceptable min closed episodes** for promotion (e.g. n≥80 on validate slice).

---

## Artifacts

| File | Role |
| --- | --- |
| `analyze-buyv2-playbook.sql` | Bias / exemplars |
| `seed-fp-buyv2-playbook.sql` | Fingerprints + fp-bv2-00/01 seeds |
| `buyv2-playbook-ladder.ps1` | Authority simulate grid (impact + worst) |

Results table (authority sim, `pumpfun_impact` + **worst** fill, window 07-22..07-29):

| Label | Fingerprint | n_closed | profit_factor | total_pnl_sol | Notes |
| --- | --- | --- | --- | --- | --- |
| F0 scale ix-only | ix only | 396 | **0.27** | -7.67 | mx-00 geometry |
| F1 scale `_local/rule-research/docs/plans/strategies/data/fp-bv2-ladder.csv`. **Conclusion so far:** the neutral-proxy /
mx dip-hot + scale_out template is **not** profitable on this playbook under honest sim. Next step is
**metric discovery** (different entry metrics / baseline) or a **grouped sweep** seeded from discovery —
not more tuning of the mx grid.

Discovery run started (lab): fingerprint `fp-bv2 BuyV2 [0-6.4)`, `token_cap=5000`, baseline TP17/SL28 —
poll `GET /api/strategies/metric-discovery/last` when complete.
