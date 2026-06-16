import { TPSL1_AXES } from 'components/sweep/groupedTypes';
import { STORAGE_KEYS } from 'lib/storage';
import { GroupedSweepView } from './GroupedSweepView';

/** TPSL1 swept knobs — the exit ladder only (no scalp entry gates, no cohort
 *  exit). This array IS the param column order in the combo table. Kept stable
 *  (not data-derived) so the columns exist on first render (colToggle
 *  persistence). MUST match the backend tpsl1 `params_json` keys. */
const TPSL1_PARAM_KEYS = [
  'exit_take_profit',
  'exit_stop_loss',
  'exit_trailing_stop_pct',
  'exit_time_stop_secs',
  'exit_stall_secs',
  'exit_liquidity_drop_pct',
];

/** TPSL1 grouped sweep — the exit-ladder-only param space. Reuses the generic
 *  view, API layer, and columns. */
export function Tpsl1GroupedSweepPage() {
  return (
    <GroupedSweepView
      strategyId="tpsl1"
      paramKeys={TPSL1_PARAM_KEYS}
      axes={TPSL1_AXES}
      storageKey={`${STORAGE_KEYS.sweepConfig}.tpsl1`}
      title="Grouped Param Sweep · TPSL1"
    />
  );
}
