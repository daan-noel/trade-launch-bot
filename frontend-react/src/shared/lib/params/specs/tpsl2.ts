// TPSL2 spec — TPSL1 plus the scalp-continuation entry gates (p_entry_*).
// Standalone. Columns mirror `RuleRecord`; `comboKey`s mirror
// `lab/src/sweep/strategies/tpsl2.rs` `params_json`. Presentation copied from the
// old `tpsl2/RuleFormModal`.

import type { ParamField, SpecSection, StrategySpec } from '../types';

const FIELDS: ParamField[] = [
  // ── Token fingerprint ──
  { column: 'p_token_initial_buy_sol', group: 'fingerprint', section: 'fingerprint', kind: 'float', required: false, label: 'Initial Buy SOL', helpKey: 'initialBuy', unit: '◎', step: '0.001', nullable: true },
  { column: 'tolerance_pct', group: 'fingerprint', section: 'fingerprint', kind: 'float', required: false, label: 'Tolerance %', helpKey: 'tolerance', unit: '%', step: '0.1', min: 0, max: 100, nullable: true },
  { column: 'p_token_cu_limit', group: 'fingerprint', section: 'fingerprint', kind: 'int', required: false, label: 'CU Limit', helpKey: 'cuLimit', nullable: true },
  { column: 'p_token_cu_price', group: 'fingerprint', section: 'fingerprint', kind: 'int', required: false, label: 'CU Price', helpKey: 'cuPrice', nullable: true },
  { column: 'p_token_max_sol_cost', group: 'fingerprint', section: 'fingerprint', kind: 'float', required: false, label: 'Max SOL Cost', helpKey: 'maxSolCost', unit: '◎', step: '0.001', nullable: true },
  { column: 'p_token_spendable_sol_in', group: 'fingerprint', section: 'fingerprint', kind: 'float', required: false, label: 'Spendable SOL In', helpKey: 'spendableSolIn', unit: '◎', step: '0.001', nullable: true },
  { column: 'p_token_ix_labels', group: 'fingerprint', section: 'fingerprint', kind: 'array', required: false, label: 'Instruction Labels', helpKey: 'ixLabels' },

  // ── Sizing & limits ──
  { column: 'buy_amount', group: 'sizing', section: 'sizing', kind: 'float', required: true, label: 'Buy Amount (SOL)', helpKey: 'buyAmount', unit: '◎', step: '0.001' },
  { column: 'p_max_concurrent_tokens', group: 'sizing', section: 'sizing', kind: 'int', required: false, label: 'Max Concurrent Tokens', helpKey: 'maxConcurrentTokens' },
  { column: 'p_max_total_tokens', group: 'sizing', section: 'sizing', kind: 'int', required: false, label: 'Max Total Tokens', helpKey: 'maxTotalTokens' },

  // ── Entry gates · scalp continuation (p_entry_*) ──
  { column: 'p_entry_min_age_secs', group: 'entry', section: 'entry', kind: 'int', required: false, comboKey: 'entry_min_age_secs', label: 'Min Age (s)', helpKey: 'minAgeSecs', step: '1', nullable: true },
  { column: 'p_entry_max_age_secs', group: 'entry', section: 'entry', kind: 'int', required: false, comboKey: 'entry_max_age_secs', label: 'Max Age (s)', helpKey: 'maxAgeSecs', step: '1', nullable: true, placeholder: '0 = until dead' },
  { column: 'p_entry_min_alive_sol', group: 'entry', section: 'entry', kind: 'float', required: false, comboKey: 'entry_min_alive_sol', label: 'Min Alive SOL', helpKey: 'minAliveSol', unit: '◎', step: '0.01', nullable: true },
  { column: 'p_entry_min_organic_sol', group: 'entry', section: 'entry', kind: 'float', required: false, comboKey: 'entry_min_organic_sol', label: 'Min Organic SOL', helpKey: 'minOrganicSol', unit: '◎', step: '0.01', nullable: true },
  { column: 'p_entry_min_organic_liq', group: 'entry', section: 'entry', kind: 'float', required: false, comboKey: 'entry_min_organic_liq', label: 'Min Organic Liq', helpKey: 'minOrganicLiq', unit: '◎', step: '0.1', nullable: true },
  { column: 'p_entry_min_liquidity_sol', group: 'entry', section: 'entry', kind: 'float', required: false, comboKey: 'entry_min_liquidity_sol', label: 'Min Liquidity SOL', helpKey: 'minLiquiditySol', unit: '◎', step: '0.1', nullable: true },
  { column: 'p_entry_pullback_pct', group: 'entry', section: 'entry', kind: 'float', required: false, comboKey: 'entry_pullback_pct', label: 'Pullback %', helpKey: 'pullbackPct', unit: '%', step: '1', min: 0, max: 100, nullable: true },
  { column: 'p_entry_higher_low_secs', group: 'entry', section: 'entry', kind: 'int', required: false, comboKey: 'entry_higher_low_secs', label: 'Higher-Low (s)', helpKey: 'higherLowSecs', step: '1', nullable: true },

  // ── Exit gates (p_exit_*) ──
  { column: 'p_exit_take_profit', group: 'exit', section: 'exit', kind: 'float', required: true, comboKey: 'exit_take_profit', label: 'Take Profit %', helpKey: 'takeProfit', unit: '%', step: '1', min: 0, inputClass: 'focus:border-green' },
  { column: 'p_exit_stop_loss', group: 'exit', section: 'exit', kind: 'float', required: true, comboKey: 'exit_stop_loss', label: 'Stop Loss %', helpKey: 'stopLoss', unit: '%', step: '1', min: 0, max: 100, inputClass: 'focus:border-red' },
  { column: 'p_exit_trailing_stop_pct', group: 'exit', section: 'exit', kind: 'float', required: false, comboKey: 'exit_trailing_stop_pct', label: 'Trailing Stop %', helpKey: 'trailingStopPct', unit: '%', step: '1', min: 0, max: 100, nullable: true, inputClass: 'focus:border-warning' },
  { column: 'p_exit_time_stop_secs', group: 'exit', section: 'exit', kind: 'int', required: false, comboKey: 'exit_time_stop_secs', label: 'Time Stop (s)', helpKey: 'timeStopSecs', step: '1', nullable: true, inputClass: 'focus:border-info' },
  { column: 'p_exit_stall_secs', group: 'exit', section: 'exit', kind: 'int', required: false, comboKey: 'exit_stall_secs', label: 'Stall (s)', helpKey: 'stallSecs', step: '1', nullable: true, inputClass: 'focus:border-accent' },
  { column: 'p_exit_liquidity_drop_pct', group: 'exit', section: 'exit', kind: 'float', required: false, comboKey: 'exit_liquidity_drop_pct', label: 'Liquidity Drop %', helpKey: 'liquidityDropPct', unit: '%', step: '1', min: 0, max: 100, nullable: true, inputClass: 'focus:border-primary' },
];

const SECTIONS: SpecSection[] = [
  { key: 'fingerprint', label: 'Token Fingerprint', hint: 'which token to match', accent: 'text-info', liveEditable: false, cols: 6 },
  { key: 'sizing', label: 'Sizing & Limits', hint: 'position size + concurrency · editable while live', accent: 'text-text-dim', liveEditable: true, cols: 3 },
  { key: 'entry', label: 'Entry Gates · Scalp', hint: 'when to buy (trade-stream shape)', accent: 'text-accent', liveEditable: false, cols: 4 },
  { key: 'exit', label: 'Exit Gates', hint: 'when to sell', accent: 'text-warning', liveEditable: false, cols: 4 },
];

export const TPSL2_SPEC: StrategySpec = {
  strategy: 'tpsl2',
  title: 'TPSL2',
  fields: FIELDS,
  modeOptions: ['paper', 'real'],
  sections: SECTIONS,
};
