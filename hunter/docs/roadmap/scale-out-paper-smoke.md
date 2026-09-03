# Scale-out paper smoke (post-deploy verification)

Scale-out ships end to end — engine → exec/mig → kernel → sweep → FE → manual partial,
contract in [../plans/strategies/partial-exits.md](../plans/strategies/partial-exits.md) —
and the golden tests cover the fold. What no test covers is the **wiring**:
sink → SSE → Console chip → dialog ledger, on a real position moving through a real ladder.
Run this once on the live box after the next deploy.

It is a wiring check, not a measurement: the PnL question is already answered (a banked
tranche loses money on `fs3-00` — see the deep dive). Nothing here depends on the rule being
profitable, only on it firing.

## Arm

Any paper rule with a 2-stage ladder works. Pick a fingerprint that arms often so the wait
is short, and keep the stages *easy to hit* — this is about legs landing, not about edge:

```sql
UPDATE strategy_rules
   SET params = jsonb_set(params, '{scale_out}', '[{"sell_bps": 5000, "take_profit": 8}]'),
       is_active = true
 WHERE rule_name = 'fs3-00 dev13 base'
   AND trade_mode = 'paper';
```

The ladder above is one explicit stage plus the global exit as the remainder. To exercise a
**remainder stage** as well, use
`[{"sell_bps": 5000, "take_profit": 8}, {"conditions": {"m_position": {"held": [{"operator": ">=", "value": 120}]}}}]`.

Disarm afterwards: `UPDATE strategy_rules SET is_active = false WHERE …`.

## What must hold

Run against the box's Postgres while one position walks the ladder.

**1. A partial keeps the position open and advances the stage.** After the first leg fills,
the row is still `Holding` (never `End`), with `scale_stage` = 1 and a nonzero
`sold_token_amount`:

```sql
SELECT status, scale_stage, sold_token_amount, entry_token_amount,
       round(10000.0 * sold_token_amount / entry_token_amount) AS sold_bps,
       exit_price, exit_time
  FROM strategy_positions
 WHERE id = :position_id;
```

`exit_price` / `exit_time` stay NULL until the final leg — a stamp on a still-open position
is the bug this catches.

**2. One ledger row per leg, entry included.**

```sql
SELECT seq, side, stage, reason, token_amount, sol_lamports, at
  FROM position_fills
 WHERE position_id = :position_id
 ORDER BY seq;
```

Expect `seq 0` = the buy, then one `sell` row per partial, then the closing sell.

**3. The aggregates match the ledger** — they are a cache, and two writers of one fact is
how they drift:

```sql
SELECT p.sold_token_amount = COALESCE(SUM(f.token_amount)  FILTER (WHERE f.side = 'sell'), 0) AS tokens_ok,
       p.exit_sol_lamports_total = COALESCE(SUM(f.sol_lamports) FILTER (WHERE f.side = 'sell'), 0) AS sol_ok
  FROM strategy_positions p
  LEFT JOIN position_fills f ON f.position_id = p.id
 WHERE p.id = :position_id
 GROUP BY p.sold_token_amount, p.exit_sol_lamports_total;
```

Both `true`.

**4. The final close stamps a weighted average, and only then re-arms.** On `End`,
`exit_price` equals the SOL-weighted mean of the sell legs, not the last leg's price:

```sql
SELECT p.exit_price,
       SUM(f.sol_lamports) / NULLIF(SUM(f.token_amount), 0)::float8 / 1e9 AS weighted_avg
  FROM strategy_positions p JOIN position_fills f ON f.position_id = p.id AND f.side = 'sell'
 WHERE p.id = :position_id GROUP BY p.exit_price;
```

A new episode for the same (token, rule) may appear only *after* that row reaches `End` —
a partial must never bump the episode counter or re-arm.

**5. The surfaces agree.** In the Console: the row shows a partial chip while mid-ladder,
the dialog's ledger lists every leg, and the chart draws one marker **per leg** rather than
one arrow at the average. Frames arrive over SSE without a refresh.

## If it wedges

A ladder whose stages cannot fire leaves the position open and holds its concurrency slot.
That is the same failure the simulate ladder hit — see the `n_open == max_concurrent_tokens`
rule in `flow-scalper-findings.md`.
Close it manually rather than waiting it out.
