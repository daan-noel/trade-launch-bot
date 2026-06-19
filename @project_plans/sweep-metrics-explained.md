# Sweep Metrics Explained

Reference for the **Grouped Sweep** result table and combo table columns.

---

## BEST SCORE

**Formula:** `mean − 1.64 × (std / √n)`

A **lower confidence bound** on the combo's true average return. It answers:
> *"What's the worst I can credibly believe this combo's real mean is, given the data?"*

| Variable | Meaning |
|---|---|
| `mean` | Average % return across closed trades |
| `std` | Sample standard deviation of closed returns |
| `n` | Number of closed trades |
| `1.64` | ~95% one-sided confidence multiplier (Z-score) |

**Example:** Mean = +50%, Std = 80%, n = 10
→ Score = 50 − 1.64 × (80 / √10) = **+8.5**

### Why it matters
- High mean with few trades → low score (no repeatable edge proven)
- High mean with volatile returns → low score (punishes inconsistency)
- High mean + many trades + tight returns → high score (reliable edge)
- Score is `None` (shown as `—`) when fewer than 2 trades have closed

**Source:** `backend/src/sweep/aggregate.rs` → `robust_score()`

---

## Combo Table Columns

### Profit Factor
`gross_wins_sol / gross_loss_sol`

For every 1 SOL lost, how many SOL were won. A value of 3.48 means 3.48 SOL gained per 1 SOL lost.
- Higher is better
- Shown as `∞` when there are zero losing trades

### Mean %
`Σ(pnl%) / n_fired`

Simple average % return over **all fired tokens**, including open positions marked to their current price.

### Median %
50th percentile of all trade returns, estimated via a log-bucketed quantile sketch.

- Half of trades did better, half did worse
- More robust to outlier wins/losses than Mean
- ~15% relative error (approximate, not exact)

### P90 %
90th percentile of all trade returns (same quantile sketch).

- 90% of trades performed *worse* than this value
- Represents what a "good day" looks like — the upside tail
- Useful for understanding ceiling, not floor

### Std %
Sample standard deviation of **closed trade** returns.

`√[(Σx² − n·μ²) / (n−1)]`

- Low Std = consistent, predictable returns
- High Std = wild swings, even if mean is positive
- Fed directly into the Score formula to penalize inconsistency
- Returns `0` when fewer than 2 closed trades

---

## Closed vs. All Trades

Some metrics use **all fired tokens** (including open positions at mark-to-market), others use only **closed trades**:

| Metric | Scope |
|---|---|
| Mean %, Median %, P90 % | All fired (open included) |
| Std %, Score | Closed trades only |
| Holding time (avg/median) | Closed trades only |
| Win rate, Total PnL, Expectancy | All fired |

---

## Memory Efficiency Note

Median and P90 are computed via a **DDSketch-style log-bucketed quantile sketch** (64 buckets per sign, ~0.6 KB per combo) instead of storing every trade's return. This keeps memory at `O(1)` per combo regardless of how many tokens are swept.

Best % and Worst % are exact running min/max — no approximation needed.
