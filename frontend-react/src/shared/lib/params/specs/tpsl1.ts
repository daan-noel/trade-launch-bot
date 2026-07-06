// TPSL1 spec — the token-fingerprint + sizing + exit-ladder rule (no entry gates).
// Standalone, hand-written. Column names mirror the `RuleRecord` / backend
// `tpsl_sniper_1` columns; `comboKey`s mirror `lab/src/sweep/strategies/tpsl1.rs`
// `params_json`. Presentation (label/help/unit/step) copied from the old
// `tpsl1/RuleFormModal`.

import type { ParamField, SpecSection, StrategySpec } from '../types';

const FIELDS: ParamField[] = [
  // ── Token fingerprint ──
  { column: 'p_token_initial_buy_sol', group: 'fingerprint', section: 'fingerprint', kind: 'float', required: false, label: 'Initial Buy SOL', helpKey: 'initialBuy', unit: '◎', step: '0.001', nullable: true },
  { column: 'p_token_first_slot_buy_sol', group: 'fingerprint', section: 'fingerprint', kind: 'float', required: false, label: 'First-slot Buy SOL', unit: '◎', step: '0.001', nullable: true },
  { column: 'p_token_first_slot_sell_sol', group: 'fingerprint', section: 'fingerprint', kind: 'float', required: false, label: 'First-slot Sell SOL', unit: '◎', step: '0.001', nullable: true },
  { column: 'tolerance_pct', group: 'fingerprint', section: 'fingerprint', kind: 'float', required: false, label: 'Tolerance %', helpKey: 'tolerance', unit: '%', step: '0.1', min: 0, max: 100, nullable: true },
  { column: 'p_token_cu_limit', group: 'fingerprint', section: 'fingerprint', kind: 'int', required: false, label: 'CU Limit', helpKey: 'cuLimit', nullable: true },
  { column: 'p_token_cu_price', group: 'fingerprint', section: 'fingerprint', kind: 'int', required: false, label: 'CU Price', helpKey: 'cuPrice', nullable: true },
  { column: 'p_token_max_sol_cost', group: 'fingerprint', section: 'fingerprint', kind: 'float', required: false, label: 'Max SOL Cost', helpKey: 'maxSolCost', unit: '◎', step: '0.001', nullable: true },
  { column: 'p_token_spendable_sol_in', group: 'fingerprint', section: 'fingerprint', kind: 'float', required: false, label: 'Spendable SOL In', helpKey: 'spendableSolIn', unit: '◎', step: '0.001', nullable: true },
  { column: 'p_token_ix_labels', group: 'fingerprint', section: 'fingerprint', kind: 'array', required: false, label: 'Instruction Labels', helpKey: 'ixLabels' },

  // ── Sizing & limits ──
  { column: 'buy_amount_sol', group: 'sizing', section: 'sizing', kind: 'float', required: true, label: 'Buy Amount (SOL)', helpKey: 'buyAmount', unit: '◎', step: '0.001' },
  { column: 'p_max_concurrent_tokens', group: 'sizing', section: 'sizing', kind: 'int', required: false, label: 'Max Concurrent Tokens', helpKey: 'maxConcurrentTokens' },
  { column: 'p_max_total_tokens', group: 'sizing', section: 'sizing', kind: 'int', required: false, label: 'Max Total Tokens', helpKey: 'maxTotalTokens' },

  // ── Exit gates (p_exit_*) ──
  { column: 'p_exit_take_profit', group: 'exit', section: 'exit', kind: 'float', required: true, comboKey: 'exit_take_profit', label: 'Take Profit %', helpKey: 'takeProfit', unit: '%', step: '1', min: 0, inputClass: 'focus:border-green' },
  { column: 'p_exit_stop_loss', group: 'exit', section: 'exit', kind: 'float', required: true, comboKey: 'exit_stop_loss', label: 'Stop Loss %', helpKey: 'stopLoss', unit: '%', step: '1', min: 0, max: 100, inputClass: 'focus:border-red' },
  { column: 'p_exit_trailing_stop_pct', group: 'exit', section: 'exit', kind: 'float', required: false, comboKey: 'exit_trailing_stop_pct', label: 'Trailing Stop %', helpKey: 'trailingStopPct', unit: '%', step: '1', min: 0, max: 100, nullable: true, inputClass: 'focus:border-warning' },
  { column: 'p_exit_time_stop_secs', group: 'exit', section: 'exit', kind: 'int', required: false, comboKey: 'exit_time_stop_secs', label: 'Time Stop (s)', helpKey: 'timeStopSecs', step: '1', nullable: true, inputClass: 'focus:border-info' },
  { column: 'p_exit_stall_secs', group: 'exit', section: 'exit', kind: 'int', required: false, comboKey: 'exit_stall_secs', label: 'Stall (s)', helpKey: 'stallSecs', step: '1', nullable: true, inputClass: 'focus:border-accent' },
  { column: 'p_exit_liquidity_drop_pct', group: 'exit', section: 'exit', kind: 'float', required: false, comboKey: 'exit_liquidity_drop_pct', label: 'Liquidity Drop %', helpKey: 'liquidityDropPct', unit: '%', step: '1', min: 0, max: 100, nullable: true, inputClass: 'focus:border-primary' },
];

const SECTIONS: SpecSection[] = [
  { key: 'fingerprint', label: 'Token Fingerprint', hint: 'which token to match', accent: 'text-info', liveEditable: false, cols: 2 },
  { key: 'sizing', label: 'Sizing & Limits', hint: 'position size + concurrency · editable while live', accent: 'text-text-dim', liveEditable: true, cols: 3 },
  { key: 'exit', label: 'Exit Gates', hint: 'when to sell', accent: 'text-warning', liveEditable: false, cols: 3 },
];

export const TPSL1_SPEC: StrategySpec = {
  strategy: 'tpsl1',
  title: 'TPSL1',
  fields: FIELDS,
  modeOptions: ['paper', 'real'],
  sections: SECTIONS,
};
