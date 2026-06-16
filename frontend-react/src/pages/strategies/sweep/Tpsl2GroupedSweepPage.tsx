import { TPSL2_AXES } from 'components/sweep/groupedTypes';
import { STORAGE_KEYS } from 'lib/storage';
import { GroupedSweepView } from './GroupedSweepView';

/** TPSL2 swept knobs — this array IS the param column order in the combo table
 *  (`buildSweepColumns` renders them in this order). Kept stable (not
 *  data-derived) so the columns exist on first render (colToggle persistence).
 *  Ordered to match the rule modal: TP/SL lead, entry gates next, then the
 *  trailing/time/stall exit knobs. MUST match the backend `params_json` keys. */
const TPSL2_PARAM_KEYS = [
  'exit_take_profit',
  'exit_stop_loss',
  'entry_min_age_secs',
  'entry_min_alive_sol',
  'entry_min_organic_sol',
  'entry_pullback_pct',
  'entry_higher_low_secs',
  'entry_max_cohort_held',
  'entry_min_liquidity_sol',
  'entry_min_organic_liq',
  'exit_trailing_stop_pct',
  'exit_time_stop_secs',
  'exit_stall_secs',
  'exit_liquidity_drop_pct',
  'exit_cohort_ratio',
];

/** TPSL2 grouped sweep — the full entry-gate + exit-ladder param space. */
export function Tpsl2GroupedSweepPage() {
  return (
    <GroupedSweepView
      strategyId="tpsl2"
      paramKeys={TPSL2_PARAM_KEYS}
      axes={TPSL2_AXES}
      storageKey={STORAGE_KEYS.sweepConfig}
      title="Grouped Param Sweep · TPSL2"
    />
  );
}
