//! Wire types for the rule-search job result — the TS mirror of
//! `hunter-lab`'s `rule_search::report::Report`.

export type RuleSearchVerdict = 'refuse' | 'ungated' | 'candidate';

export interface RuleSearchScored {
  params: Record<string, unknown>;
  n_fired: number;
  n_closed: number;
  n_tokens_entered: number;
  enter_pct: number;
  enter_pct_unguarded: number | null;
  total_pnl_sol: number;
  total_pnl_sol_optimistic: number | null;
  profit_factor: number | null;
  win_rate: number;
  /** Mean realized SOL per closed trade (null when nothing closed). */
  expectancy_sol?: number | null;
  /** Realized SOL per time quartile of the search range. */
  block_pnl_sol?: number[];
}

/** Champion replayed with both legs' fill windows delayed by `delay_ms`. */
export interface RuleSearchLadderRung {
  delay_ms: number;
  total_pnl_sol: number;
  n_closed: number;
}

/** Champion minus one clause, authority replay. */
export interface RuleSearchAblationRow {
  side: 'entry' | 'exit';
  removed: string;
  total_pnl_sol: number;
  n_tokens_entered: number;
  enter_pct: number;
}

export interface RuleSearchReport {
  verdict: RuleSearchVerdict;
  n_matched: number;
  n_combos: number;
  champion: RuleSearchScored | null;
  empty_entry: RuleSearchScored | null;
  incumbent: RuleSearchScored | null;
  archive: RuleSearchScored[];
  archive_replay_disagree: boolean;
  diagnostics: string[];
  /** Champion authority SOL at 0 / 250 / 500 / 1000 ms fill delay. */
  champion_ladder?: RuleSearchLadderRung[];
  /** Every ladder-raced candidate flips sign at 1 s fill delay. */
  latency_fragile?: boolean;
  /** Champion fast-archive SOL vs same-exit-bag siblings, in std deviations. */
  sibling_z?: number | null;
  champion_ablation?: RuleSearchAblationRow[];
  /** Champion realized SOL over gross attainable (entry → post-entry ATH). */
  exit_efficiency?: number | null;
}

/** `GET …/rule-search/{run_id}` and `/last` both return this envelope. */
export interface RuleSearchResult {
  run_id: string;
  result: RuleSearchReport;
}

/** Body for `POST /api/strategies/rule-search`. */
export interface RuleSearchStartArgs {
  fingerprint_id: string;
  created_after?: string;
  created_before?: string;
  buy_amount_sol?: number;
  fill_model?: string;
  cost_model?: string;
  skip_duplicate_identity?: boolean;
  incumbent_rule_id?: string;
  token_cap?: number;
}
