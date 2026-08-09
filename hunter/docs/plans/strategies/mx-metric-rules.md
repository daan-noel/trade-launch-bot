# mx metric rules — DB-driven decisions (2026-07-29)

Neutral analysis on local PG (**not** wallet copy-trading). Method:
`_local/rule-research/scripts/analyze_dip_hot_neutral.py` — 220
hot tokens (250–2500 curve trades each), dip-hot entry proxy on spot from reserves,
costs **2.53% fee + 2× buy/vsol impact**, 07-22..07-28.

**Honest limit:** this is a **path proxy**, not `hunter-engine` simulate. Tail episodes
(p10 ≈ **−32%**) dominate means; **promote only after** `lake-export` + lab simulate
(`pumpfun_impact`, `worst` fill). The proxy is still useful for **relative** choices.

## Headline: what is *not* reliably profitable

- **Market-wide dip-hot with re-entry** (up to 8 eps/token): every entry×exit combo in
  the grid had **negative mean** %/episode at 0.10 SOL buy (best mean about **−1.9%**).
- **Deep dip-only gates** (25% trail, gross60 35) did **worse** than shallower
  **dip 12% / gross60 15** on this sample.
- **`m_flow_window(2).net >= 0`** was not tested here; wallet work says it often
  **fights** knife-catch entries — leave it out of mx until measured neutrally.

## Decisions (data → rule shape)

| Knob | Decision | Evidence |
| --- | --- | --- |
| **Fingerprint** | **`init_buy` bucket `_local/rule-research/scripts/seed-mx-metric-rules.sql` → prefix
**`mx-*`**, all `paper` / `is_active=false`.

| Rule | Fingerprint | Intent |
| --- | --- | --- |
| **mx-00 dip scale one-shot** | mx-dev small [0-6.4) | **Primary candidate** |
| mx-01 dip full tp one-shot | same | A/B vs scale_out |
| mx-02 dip scale reentry | same | Negative control (re-entry) |
| mx-03 dip scale micro | mx-dev micro [0-1.6) | Tighter dev-buy screen |
| mx-04 dip scale broad | mx-ALL broad | Metrics-only universe |

## Next validation step (required for “reliable”)

Follow [metrics-path-profitable-rules.md](metrics-path-profitable-rules.md) phases 1–3:

```powershell
cargo run -p hunter-lab -- lake-export
psql "$env:DATABASE_URL" -f hunter/scripts/seed-mx-metric-rules.sql
# then lab simulate mx-00 vs mx-01 vs mx-04, pumpfun_impact, worst fill, same window
```

If mx-00 is still negative on **worst** fill, shrink tail (smaller buy, tighter SL, or
cold-flow exit on remainder) before arming paper.

Scratch CSV from the analyzer: `scripts/_neutral_dip_hot_trades.csv` (gitignored in
practice — delete when done).
