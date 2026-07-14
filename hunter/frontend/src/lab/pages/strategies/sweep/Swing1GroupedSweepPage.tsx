import { SWING1_AXES } from '@lab/components/sweep/groupedTypes';
import { swing1AxesShapeWarning } from '@shared/lib/swing1Axes';
import { STORAGE_KEYS } from 'lib/storage';
import { GroupedSweepView } from './GroupedSweepView';

/** swing1 swept knobs — this array IS the param column order in the combo table
 *  (`buildSweepColumns` renders them in this order). Kept stable (not
 *  data-derived) so the columns exist on first render (colToggle persistence).
 *  Ordered as the rule reads: exit ladder leads (TP/SL), then the swing/kill/
 *  volume detection + entry-confirmation gates, then the symmetric next-kill
 *  flee. MUST match the backend swing1 `params_json` keys. */
const SWING1_PARAM_KEYS = [
  // Exit ladder
  'exit_take_profit',
  'exit_stop_loss',
  'exit_trailing_stop_pct',
  'exit_time_stop_secs',
  'exit_stall_secs',
  'exit_liquidity_drop_pct',
  // Swing detection (entry)
  'swing_high_to_low_pct',
  'swing_low_to_high_pct',
  'swing_high_to_low_sol',
  'swing_low_to_high_sol',
  'swing_min_leg_trades',
  // Kill-low profile (entry)
  'kill_depth_min_pct',
  'kill_max_duration_ms',
  'kill_min_net_flow_per_sec',
  // Volume-low profile + transition floor (entry)
  'vol_depth_max_pct',
  'vol_min_duration_ms',
  'vol_min_up_duration_ms',
  'min_kills_before_volume',
  // Entry confirmation
  'entry_pullback_pct',
  'entry_higher_low_secs',
  'entry_max_age_secs',
  'entry_min_liquidity_sol',
  // Symmetric next-kill exit
  'exit_next_kill_depth_min_pct',
  'exit_next_kill_max_duration_ms',
];

/** swing1 grouped sweep — the kill→volume swing-phase param space. Reads best
 *  ungrouped (the pattern is rare per-dev → singleton groups), so leave the
 *  group-by empty for one "ALL" group. */
export function Swing1GroupedSweepPage() {
  return (
    <GroupedSweepView
      strategyId="swing_1"
      paramKeys={SWING1_PARAM_KEYS}
      axes={SWING1_AXES}
      storageKey={`${STORAGE_KEYS.sweepConfig}.swing1`}
      title="Grouped Param Sweep · swing1"
      axesWarning={swing1AxesShapeWarning}
    />
  );
}
