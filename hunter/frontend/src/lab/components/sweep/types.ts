// Shared sweep-result row shape, reused by the grouped-sweep page and
// `buildSweepColumns`. Mirrors the backend per-combo metrics serde struct.

/** One ranked combo row: the combo's aggregated outcome across all swept tokens.
 *  `params` is the combo's rule `params` document (format 2; a run stored before it
 *  may carry format 1, which `ruleParamsCell` shows as such). */
export interface SweepResultRecord {
  combo_id: number;
  params: Record<string, unknown>;
  n_fired: number;
  n_open: number;
  n_closed: number;
  win_rate: number;
  total_pnl_sol: number;
  /** Unrealized mark-to-last-price sum over the still-`Open` positions. Excluded
   *  from `total_pnl_sol` (realized-only) — add the two for mark-to-market. */
  open_pnl_sol: number;
  mean_pnl_pct: number;
  median_pnl_pct: number;
  p90_pnl_pct: number;
  best_pnl_pct: number;
  worst_pnl_pct: number;
  /** Stddev of realized per-trade pnl% (display). */
  std_pnl_pct: number;
  /** null = no losing trades (infinite profit factor). */
  profit_factor: number | null;
  /** Checklist rank: MTM% scaled by fire-rate × open-drag × win-rate (a gain shrinks, a loss deepens). null = never fired. */
  score: number | null;
  expectancy_sol: number;
  avg_holding_secs: number;
  median_holding_secs: number;
  /** Per-exit-reason trade counts — how many of this combo's closed trades
   *  terminated on each reason. Counts, **not** params: distinct from the
   *  `exit_take_profit`/`exit_stop_loss` *threshold* knobs inside `params`. */
  n_exit_take_profit: number;
  n_exit_stop_loss: number;
  n_exit_trailing: number;
  n_exit_stall: number;
  n_exit_time: number;
  n_exit_liquidity: number;
  /** Exits on a rule's own sell line. Optional on the wire (absent = 0). */
  n_exit_metrics?: number;
  /** `n_exit_metrics` broken down by WHICH sell line fired (slot index into the
   *  page's `X-Exit-Metric-Legend` header, see `useStreamedSweepResults`). Absent on
   *  rows written before this column existed; `sum() === n_exit_metrics` otherwise. */
  n_exit_metrics_by_slot?: number[];
  /** Analysis-only death-closes: positions closed at the last meaningful trade
   *  because the token died silent. Counted as closed; 0 on the live path. */
  n_exit_dead: number;
  n_exit_open: number;
}
