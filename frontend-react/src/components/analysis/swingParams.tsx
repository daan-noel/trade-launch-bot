import { Input } from 'components/ui/Input';
import type { SwingParams } from 'types';

export const DEFAULT_SWING_PARAMS: SwingParams = {
  high_to_low_threshold_sol: 5,
  high_to_low_threshold_pct: 50,
  low_to_high_threshold_sol: 5,
  low_to_high_threshold_pct: 50,
  min_leg_trades: 2,
  min_leg_duration_ms: 0,
  min_leg_volume: 0,
  min_leg_net_flow: 0,
  max_leg_trades: 0,
  max_leg_duration_ms: 0,
  max_leg_volume: 0,
  max_leg_net_flow: 0,
};

/** Params parsed as integers (the rest are parsed as floats). */
export const SWING_PARAM_INT_KEYS = new Set<keyof SwingParams>([
  'min_leg_trades',
  'min_leg_duration_ms',
  'max_leg_trades',
  'max_leg_duration_ms',
]);

export const SWING_PARAM_KEYS = Object.keys(DEFAULT_SWING_PARAMS) as (keyof SwingParams)[];

/** Editable form shape: fields may be empty while the user is typing. */
export type SwingParamsForm = { [K in keyof SwingParams]: number | '' };

/** Coerce an in-progress form into params, treating empty fields as 0. */
export function swingParamsFromForm(form: SwingParamsForm): SwingParams {
  const out = {} as SwingParams;
  for (const key of SWING_PARAM_KEYS) {
    const value = form[key];
    out[key] = typeof value === 'number' ? value : 0;
  }
  return out;
}

export function isFiniteNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value);
}

export function mergeSwingParams(partial: Partial<SwingParams> | undefined): SwingParams {
  if (!partial) return DEFAULT_SWING_PARAMS;
  const merged = { ...DEFAULT_SWING_PARAMS };
  for (const key of SWING_PARAM_KEYS) {
    const value = partial[key];
    if (isFiniteNumber(value)) merged[key] = value;
  }
  return merged;
}

export const swingParamLabelClassName =
  'flex flex-col gap-1 text-[10px] font-bold uppercase tracking-widest text-text-dim';

interface SwingParamsGridProps {
  params: SwingParamsForm;
  onChange: <K extends keyof SwingParams>(key: K, raw: string) => void;
}

/** The 12-input swing-detection parameter grid, shared by the single-token and
 *  all-tokens ("Swing Detection All") Analysis panels. */
export function SwingParamsGrid({ params, onChange }: SwingParamsGridProps) {
  return (
    <div className="mb-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
      <label className={swingParamLabelClassName}>
        High → low (SOL)
        <Input
          fieldSize="md"
          variant="card"
          className="min-w-0 font-normal normal-case tracking-normal"
          type="number"
          min={0}
          step="any"
          value={params.high_to_low_threshold_sol}
          onChange={(e) => onChange('high_to_low_threshold_sol', e.target.value)}
        />
      </label>
      <label className={swingParamLabelClassName}>
        High → low (%)
        <Input
          fieldSize="md"
          variant="card"
          className="min-w-0 font-normal normal-case tracking-normal"
          type="number"
          min={0}
          max={100}
          step="any"
          value={params.high_to_low_threshold_pct}
          onChange={(e) => onChange('high_to_low_threshold_pct', e.target.value)}
        />
      </label>
      <label className={swingParamLabelClassName}>
        Low → high (SOL)
        <Input
          fieldSize="md"
          variant="card"
          className="min-w-0 font-normal normal-case tracking-normal"
          type="number"
          min={0}
          step="any"
          value={params.low_to_high_threshold_sol}
          onChange={(e) => onChange('low_to_high_threshold_sol', e.target.value)}
        />
      </label>
      <label className={swingParamLabelClassName}>
        Low → high (%)
        <Input
          fieldSize="md"
          variant="card"
          className="min-w-0 font-normal normal-case tracking-normal"
          type="number"
          min={0}
          max={100}
          step="any"
          value={params.low_to_high_threshold_pct}
          onChange={(e) => onChange('low_to_high_threshold_pct', e.target.value)}
        />
      </label>
      <label className={swingParamLabelClassName}>
        Min leg trades
        <Input
          fieldSize="md"
          variant="card"
          className="min-w-0 font-normal normal-case tracking-normal"
          type="number"
          min={0}
          step={1}
          value={params.min_leg_trades}
          onChange={(e) => onChange('min_leg_trades', e.target.value)}
        />
      </label>
      <label className={swingParamLabelClassName}>
        Min leg duration (ms)
        <Input
          fieldSize="md"
          variant="card"
          className="min-w-0 font-normal normal-case tracking-normal"
          type="number"
          min={0}
          step={1}
          value={params.min_leg_duration_ms}
          onChange={(e) => onChange('min_leg_duration_ms', e.target.value)}
        />
      </label>
      <label className={swingParamLabelClassName}>
        Min leg volume (SOL)
        <Input
          fieldSize="md"
          variant="card"
          className="min-w-0 font-normal normal-case tracking-normal"
          type="number"
          min={0}
          step="any"
          value={params.min_leg_volume}
          onChange={(e) => onChange('min_leg_volume', e.target.value)}
        />
      </label>
      <label className={swingParamLabelClassName}>
        Min leg net flow (SOL)
        <Input
          fieldSize="md"
          variant="card"
          className="min-w-0 font-normal normal-case tracking-normal"
          type="number"
          min={0}
          step="any"
          value={params.min_leg_net_flow}
          onChange={(e) => onChange('min_leg_net_flow', e.target.value)}
        />
      </label>
      <label className={swingParamLabelClassName}>
        Max leg trades
        <Input
          fieldSize="md"
          variant="card"
          className="min-w-0 font-normal normal-case tracking-normal"
          type="number"
          min={0}
          step={1}
          value={params.max_leg_trades}
          onChange={(e) => onChange('max_leg_trades', e.target.value)}
        />
      </label>
      <label className={swingParamLabelClassName}>
        Max leg duration (ms)
        <Input
          fieldSize="md"
          variant="card"
          className="min-w-0 font-normal normal-case tracking-normal"
          type="number"
          min={0}
          step={1}
          value={params.max_leg_duration_ms}
          onChange={(e) => onChange('max_leg_duration_ms', e.target.value)}
        />
      </label>
      <label className={swingParamLabelClassName}>
        Max leg volume (SOL)
        <Input
          fieldSize="md"
          variant="card"
          className="min-w-0 font-normal normal-case tracking-normal"
          type="number"
          min={0}
          step="any"
          value={params.max_leg_volume}
          onChange={(e) => onChange('max_leg_volume', e.target.value)}
        />
      </label>
      <label className={swingParamLabelClassName}>
        Max leg net flow (SOL)
        <Input
          fieldSize="md"
          variant="card"
          className="min-w-0 font-normal normal-case tracking-normal"
          type="number"
          min={0}
          step="any"
          value={params.max_leg_net_flow}
          onChange={(e) => onChange('max_leg_net_flow', e.target.value)}
        />
      </label>
    </div>
  );
}
