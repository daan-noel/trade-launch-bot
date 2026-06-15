// Records returned by the TPSL2 param-sweep endpoints
// (`/api/strategies/tpsl2/sweeps[...]`). Mirror the backend `Tpsl2SweepRun` /
// `Tpsl2SweepResult` serde structs.

export interface SweepRunRecord {
  id: string;
  rule_id: string | null;
  source: string;
  method: string;
  token_count: number;
  combo_count: number;
  corpus_hash: string | null;
  created_at: string;
}

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
  exit_take_profit: number;
  exit_stop_loss: number;
  exit_trailing: number;
  exit_stall: number;
  exit_time: number;
  exit_liquidity: number;
  exit_cohort: number;
  exit_open: number;
}
