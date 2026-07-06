// Swing 1 spec — the kill→volume swing-phase strategy. Standalone. Its 25 detection/
// exit axes plus the universal sizing + optional token-fingerprint pre-filter.
//
// Column mapping (mirrors `swing1AxisCol` in `@shared/lib/swing1Axes`): the exit
// ladder knobs get an `exit_` infix (`p_exit_take_profit`), `dust_frac`/`min_kills_
// before_volume` map to `p_<key>`, everything else is `p_<key>`. `comboKey` mirrors
// `lab/src/sweep/strategies/swing1.rs` `params_json` (exit-ladder → `exit_*`, else
// the bare key). `detectKey` mirrors `Swing1DetectParams` (exit-ladder → bare key,
// `exit_next_kill_*` keeps its prefix). Presentation copied from `SWING1_AXES`.

import type { ParamField, SpecSection, StrategySpec } from '../types';

// swing1 is paper-only; TP/SL are the two required axes.
const FIELDS: ParamField[] = [
  // ── Sizing & limits (universal) ──
  { column: 'buy_amount_sol', group: 'sizing', section: 'sizing', kind: 'float', required: true, label: 'Buy Amount (SOL)', unit: '◎', step: '0.001' },
  { column: 'p_max_concurrent_tokens', group: 'sizing', section: 'sizing', kind: 'int', required: false, label: 'Max Concurrent Tokens' },
  { column: 'p_max_total_tokens', group: 'sizing', section: 'sizing', kind: 'int', required: false, label: 'Max Total Tokens' },

  // ── Token fingerprint (optional pre-filter · p_token_*) ──
  { column: 'p_token_initial_buy_sol', group: 'fingerprint', section: 'fingerprint', kind: 'float', required: false, label: 'Initial Buy SOL', unit: '◎', step: '0.001', nullable: true },
  { column: 'tolerance_pct', group: 'fingerprint', section: 'fingerprint', kind: 'float', required: false, createBlankZero: true, label: 'Tolerance %', unit: '%', step: '0.1', min: 0, max: 100, nullable: true },
  { column: 'p_token_cu_limit', group: 'fingerprint', section: 'fingerprint', kind: 'int', required: false, label: 'CU Limit', nullable: true },
  { column: 'p_token_cu_price', group: 'fingerprint', section: 'fingerprint', kind: 'int', required: false, label: 'CU Price', nullable: true },
  { column: 'p_token_max_sol_cost', group: 'fingerprint', section: 'fingerprint', kind: 'float', required: false, label: 'Max SOL Cost', unit: '◎', step: '0.001', nullable: true },
  { column: 'p_token_spendable_sol_in', group: 'fingerprint', section: 'fingerprint', kind: 'float', required: false, label: 'Spendable SOL In', unit: '◎', step: '0.001', nullable: true },
  { column: 'p_token_ix_labels', group: 'fingerprint', section: 'fingerprint', kind: 'array', required: false, label: 'Instruction Labels' },

  // ── Entry · swing detection ──
  { column: 'p_swing_high_to_low_pct', group: 'entry', section: 'swing', kind: 'float', required: false, comboKey: 'swing_high_to_low_pct', detectKey: 'swing_high_to_low_pct', label: 'Swing high→low % (0–100)', nullable: true, help: 'how far price must drop from a local high to count that down-leg as a swing' },
  { column: 'p_swing_low_to_high_pct', group: 'entry', section: 'swing', kind: 'float', required: false, comboKey: 'swing_low_to_high_pct', detectKey: 'swing_low_to_high_pct', label: 'Swing low→high % (0–100)', nullable: true, help: 'how far price must rise from a local low to count that up-leg as a swing' },
  { column: 'p_swing_high_to_low_sol', group: 'entry', section: 'swing', kind: 'float', required: false, comboKey: 'swing_high_to_low_sol', detectKey: 'swing_high_to_low_sol', label: 'Swing high→low (SOL)', nullable: true, help: 'same down-leg gate, but in absolute SOL move instead of % (off = use % only)' },
  { column: 'p_swing_low_to_high_sol', group: 'entry', section: 'swing', kind: 'float', required: false, comboKey: 'swing_low_to_high_sol', detectKey: 'swing_low_to_high_sol', label: 'Swing low→high (SOL)', nullable: true, help: 'same up-leg gate, but in absolute SOL move instead of % (off = use % only)' },
  { column: 'p_swing_min_leg_trades', group: 'entry', section: 'swing', kind: 'int', required: false, comboKey: 'swing_min_leg_trades', detectKey: 'swing_min_leg_trades', label: 'Swing min leg trades', nullable: true, help: 'min trades a leg needs to count — filters out noise legs of 1–2 trades' },
  { column: 'p_dust_frac', group: 'entry', section: 'swing', kind: 'float', required: false, comboKey: 'dust_frac', detectKey: 'dust_frac', label: 'Dust frac (0–1)', nullable: true, help: 'drop a trade smaller than this fraction of the active leg’s biggest trade. off = keep all non-zero trades' },

  // ── Entry · kill-low profile ──
  { column: 'p_kill_depth_min_pct', group: 'entry', section: 'kill', kind: 'float', required: false, comboKey: 'kill_depth_min_pct', detectKey: 'kill_depth_min_pct', label: 'Kill depth min (0–1)', nullable: true, help: 'a "kill" low must drop at least this fraction of the prior high (0.6 = −60%)' },
  { column: 'p_kill_max_duration_ms', group: 'entry', section: 'kill', kind: 'int', required: false, comboKey: 'kill_max_duration_ms', detectKey: 'kill_max_duration_ms', label: 'Kill max duration (ms)', nullable: true, help: 'a kill must happen fast — within this many ms' },
  { column: 'p_kill_min_net_flow_per_sec', group: 'entry', section: 'kill', kind: 'float', required: false, comboKey: 'kill_min_net_flow_per_sec', detectKey: 'kill_min_net_flow_per_sec', label: 'Kill min net flow (SOL/s)', nullable: true, help: 'min SOL/s of selling during the kill — confirms a real flush (off = ignore)' },

  // ── Entry · volume-low profile + transition floor ──
  { column: 'p_vol_depth_max_pct', group: 'entry', section: 'volume', kind: 'float', required: false, comboKey: 'vol_depth_max_pct', detectKey: 'vol_depth_max_pct', label: 'Vol depth max (0–1)', nullable: true, help: 'a "volume-phase" low must be shallower than this — drawing traders, not flushing' },
  { column: 'p_vol_min_duration_ms', group: 'entry', section: 'volume', kind: 'int', required: false, comboKey: 'vol_min_duration_ms', detectKey: 'vol_min_duration_ms', label: 'Vol min duration (ms)', nullable: true, help: 'a volume-phase low must last at least this long — organic accumulation' },
  { column: 'p_vol_min_up_duration_ms', group: 'entry', section: 'volume', kind: 'int', required: false, comboKey: 'vol_min_up_duration_ms', detectKey: 'vol_min_up_duration_ms', label: 'Vol min up-leg (ms)', nullable: true, help: 'min duration of the recovery up-leg after a volume low (off = no floor)' },
  { column: 'p_min_kills_before_volume', group: 'entry', section: 'volume', kind: 'int', required: false, comboKey: 'min_kills_before_volume', detectKey: 'min_kills_before_volume', label: 'Min kills before volume', nullable: true, help: 'how many kill lows must occur before we accept the volume phase (0 = "buy first dip")' },

  // ── Entry · confirmation ──
  { column: 'p_entry_pullback_pct', group: 'entry', section: 'confirm', kind: 'float', required: false, comboKey: 'entry_pullback_pct', detectKey: 'entry_pullback_pct', label: 'Entry pullback % (0–100)', nullable: true, help: 'wait for this % pullback off the confirming high before buying' },
  { column: 'p_entry_higher_low_secs', group: 'entry', section: 'confirm', kind: 'int', required: false, comboKey: 'entry_higher_low_secs', detectKey: 'entry_higher_low_secs', label: 'Entry higher-low (s)', nullable: true, help: 'require a higher-low to hold for this many seconds before entering' },
  { column: 'p_entry_max_age_secs', group: 'entry', section: 'confirm', kind: 'int', required: false, comboKey: 'entry_max_age_secs', detectKey: 'entry_max_age_secs', label: 'Entry max age (s)', nullable: true, help: 'skip the token if it is already older than this at entry (off = no age cap)' },
  { column: 'p_entry_min_liquidity_sol', group: 'entry', section: 'confirm', kind: 'float', required: false, comboKey: 'entry_min_liquidity_sol', detectKey: 'entry_min_liquidity_sol', label: 'Entry min liq (SOL)', nullable: true, help: 'only enter if pool liquidity is at least this many SOL (off = no minimum)' },

  // ── Exit · the reused ladder (TP/SL required) ──
  { column: 'p_exit_take_profit', group: 'exit', section: 'ladder', kind: 'float', required: true, comboKey: 'exit_take_profit', detectKey: 'take_profit', label: 'Take profit %', help: 'sell once unrealized gain reaches this %' },
  { column: 'p_exit_stop_loss', group: 'exit', section: 'ladder', kind: 'float', required: true, comboKey: 'exit_stop_loss', detectKey: 'stop_loss', label: 'Stop loss %', help: 'sell once unrealized loss reaches this %' },
  { column: 'p_exit_trailing_stop_pct', group: 'exit', section: 'ladder', kind: 'float', required: false, comboKey: 'exit_trailing_stop_pct', detectKey: 'trailing_stop_pct', label: 'Trailing stop %', nullable: true, help: 'sell if price falls this % from its peak since entry (off = no trail)' },
  { column: 'p_exit_time_stop_secs', group: 'exit', section: 'ladder', kind: 'int', required: false, comboKey: 'exit_time_stop_secs', detectKey: 'time_stop_secs', label: 'Time stop (s)', nullable: true, help: 'hard exit after holding this many seconds regardless of PnL (off = no time cap)' },
  { column: 'p_exit_stall_secs', group: 'exit', section: 'ladder', kind: 'int', required: false, comboKey: 'exit_stall_secs', detectKey: 'stall_secs', label: 'Stall (s)', nullable: true, help: 'exit if no meaningful price move for this many seconds (off = ignore)' },
  { column: 'p_exit_liquidity_drop_pct', group: 'exit', section: 'ladder', kind: 'float', required: false, comboKey: 'exit_liquidity_drop_pct', detectKey: 'liquidity_drop_pct', label: 'Liq-drop exit %', nullable: true, help: 'bail if pool liquidity drops this % from entry (off = ignore)' },

  // ── Exit · symmetric next-kill flee ──
  { column: 'p_exit_next_kill_depth_min_pct', group: 'exit', section: 'next-kill', kind: 'float', required: false, comboKey: 'exit_next_kill_depth_min_pct', detectKey: 'exit_next_kill_depth_min_pct', label: 'Next-kill depth min (0–1)', nullable: true, help: 'flee if a fresh kill-style drop of at least this depth starts after entry (off = ignore)' },
  { column: 'p_exit_next_kill_max_duration_ms', group: 'exit', section: 'next-kill', kind: 'int', required: false, comboKey: 'exit_next_kill_max_duration_ms', detectKey: 'exit_next_kill_max_duration_ms', label: 'Next-kill max duration (ms)', nullable: true, help: 'how fast that fresh drop must be to count as a next-kill flee trigger' },
];

const SECTIONS: SpecSection[] = [
  { key: 'sizing', label: 'Sizing & Limits', hint: 'position size + concurrency · editable while live', accent: 'text-text-dim', liveEditable: true, cols: 3 },
  { key: 'fingerprint', label: 'Token Fingerprint', hint: 'optional · which token to match · blank = off', accent: 'text-info', liveEditable: false, cols: 6 },
  { key: 'swing', label: 'Swing', hint: 'reversal legs', accent: 'text-sky-300', liveEditable: false, cols: 4 },
  { key: 'kill', label: 'Kill', hint: 'deep + fast flush', accent: 'text-red', liveEditable: false, cols: 4 },
  { key: 'volume', label: 'Volume', hint: 'shallow accumulation', accent: 'text-green', liveEditable: false, cols: 4 },
  { key: 'confirm', label: 'Confirm', hint: 'pullback + guards', accent: 'text-accent', liveEditable: false, cols: 4 },
  { key: 'ladder', label: 'Exit ladder', hint: 'TP/SL + trails', accent: 'text-warning', liveEditable: false, cols: 4 },
  { key: 'next-kill', label: 'Next-kill flee', hint: 'symmetric exit', accent: 'text-orange-300', liveEditable: false, cols: 4 },
];

export const SWING1_SPEC: StrategySpec = {
  strategy: 'swing_1',
  title: 'Swing 1',
  fields: FIELDS,
  modeOptions: ['paper'],
  sections: SECTIONS,
};
