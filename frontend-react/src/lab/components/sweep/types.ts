// Shared sweep-result row shape, reused by the grouped-sweep page and
// `buildSweepColumns`. Mirrors the backend per-combo metrics serde struct.

/** One ranked param-pair row: the combo's aggregated outcome across all swept
 *  tokens. `params` carries the strategy's swept knob values (keys vary by
 *  strategy), so the table derives its param columns from them. */
export interface SweepResultRecord {
  combo_id: number;
  params: Record<string, number | null>;
  n_fired: number;
  n_open: number;
  n_closed: number;
  win_rate: number;
  total_pnl_sol: number;
  mean_pnl_pct: number;
  median_pnl_pct: number;
  p90_pnl_pct: number;
  best_pnl_pct: number;
  worst_pnl_pct: number;
  /** Stddev of realized per-trade pnl% — the dispersion term in `score`. */
  std_pnl_pct: number;
  /** null = no losing trades (infinite profit factor). */
  profit_factor: number | null;
  /** Robust rank μ − z·σ/√n over closed trades; null = fewer than 2 closed
   *  trades. The table sorts on this by default and blanks nulls. */
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
  n_exit_cohort: number;
  /** swing1's symmetric next-kill flee count; 0 for tpsl1/tpsl2. */
  n_exit_next_kill: number;
  n_exit_open: number;
}
