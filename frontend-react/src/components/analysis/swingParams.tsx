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
  swing_high_min_delta_pct: 0,
  swing_high_max_delta_pct: 0,
  swing_high_min_net_flow_per_sec: 0,
  swing_high_max_net_flow_per_sec: 0,
  swing_low_min_delta_pct: 0,
  swing_low_max_delta_pct: 0,
  swing_low_min_net_flow_per_sec: 0,
  swing_low_max_net_flow_per_sec: 0,
  big_tx_sol: 5,
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

const swingRangeInputClassName = 'min-w-0 font-normal normal-case tracking-normal';

/** One side of a {@link RangeInputs} pair: its current value and how to update it. */
export interface RangeInputSide {
  value: number | '';
  onChange: (raw: string) => void;
  placeholder?: string;
  /** `min` attribute for the native input (default 0). Pass `null` to allow negatives. */
  min?: number | null;
  /** `max` attribute for the native input (e.g. 100 for a percentage). */
  max?: number;
  step?: number | string;
  disabled?: boolean;
}

interface RangeInputsProps {
  label: string;
  left: RangeInputSide;
  right: RangeInputSide;
  /** Glyph between the two inputs: `–` for a min/max range, `/` for a SOL/% pair. */
  separator?: string;
}

/** A labelled cell holding two paired number inputs — a min/max range, a SOL/%
 *  threshold pair, a start/end window, etc. Presentational: each side carries
 *  its own value/onChange, so it isn't tied to any particular form shape. */
export function RangeInputs({ label, left, right, separator = '–' }: RangeInputsProps) {
  const renderInput = (side: RangeInputSide) => (
    <Input
      fieldSize="md"
      variant="card"
      className={swingRangeInputClassName}
      type="number"
      min={side.min === null ? undefined : (side.min ?? 0)}
      max={side.max}
      step={side.step ?? 'any'}
      placeholder={side.placeholder}
      disabled={side.disabled}
      value={side.value}
      onChange={(e) => side.onChange(e.target.value)}
    />
  );
  return (
    <div className={swingParamLabelClassName}>
      {label}
      <div className="flex items-center gap-1">
        {renderInput(left)}
        <span className="text-[10px] text-text-dim/50">{separator}</span>
        {renderInput(right)}
      </div>
    </div>
  );
}

/** One side of a {@link SwingRangeField} — which {@link SwingParams} key it edits. */
interface SwingRangeSide {
  key: keyof SwingParams;
  placeholder?: string;
  /** `max` attribute for the native input (e.g. 100 for a percentage). */
  max?: number;
  step?: number | string;
}

interface SwingRangeFieldProps {
  label: string;
  left: SwingRangeSide;
  right: SwingRangeSide;
  separator?: string;
  params: SwingParamsForm;
  onChange: <K extends keyof SwingParams>(key: K, raw: string) => void;
}

/** {@link RangeInputs} bound to a pair of {@link SwingParams} keys (0 = unbounded). */
export function SwingRangeField({ label, left, right, separator, params, onChange }: SwingRangeFieldProps) {
  const side = (s: SwingRangeSide): RangeInputSide => ({
    value: params[s.key],
    onChange: (raw) => onChange(s.key, raw),
    placeholder: s.placeholder,
    max: s.max,
    step: s.step,
  });
  return <RangeInputs label={label} separator={separator} left={side(left)} right={side(right)} />;
}

/** The swing-detection parameter grid (the reversal thresholds render as SOL/%
 *  pairs and the four leg min/max bounds as range fields), shared by the
 *  single-token and all-tokens ("Swing Detection All") Analysis panels. */
export function SwingParamsGrid({ params, onChange }: SwingParamsGridProps) {
  return (
    <div className="mb-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
      <SwingRangeField
        label="High → low (SOL / %)"
        left={{ key: 'high_to_low_threshold_sol', placeholder: 'SOL' }}
        right={{ key: 'high_to_low_threshold_pct', placeholder: '%', max: 100 }}
        separator="/"
        params={params}
        onChange={onChange}
      />
      <SwingRangeField
        label="Low → high (SOL / %)"
        left={{ key: 'low_to_high_threshold_sol', placeholder: 'SOL' }}
        right={{ key: 'low_to_high_threshold_pct', placeholder: '%', max: 100 }}
        separator="/"
        params={params}
        onChange={onChange}
      />

      <SwingRangeField
        label="Leg trades"
        left={{ key: 'min_leg_trades', placeholder: 'min', step: 1 }}
        right={{ key: 'max_leg_trades', placeholder: 'max', step: 1 }}
        params={params}
        onChange={onChange}
      />
      <SwingRangeField
        label="Leg duration (ms)"
        left={{ key: 'min_leg_duration_ms', placeholder: 'min', step: 1 }}
        right={{ key: 'max_leg_duration_ms', placeholder: 'max', step: 1 }}
        params={params}
        onChange={onChange}
      />
      <SwingRangeField
        label="Leg volume (SOL)"
        left={{ key: 'min_leg_volume', placeholder: 'min' }}
        right={{ key: 'max_leg_volume', placeholder: 'max' }}
        params={params}
        onChange={onChange}
      />
      <SwingRangeField
        label="Leg net flow (SOL)"
        left={{ key: 'min_leg_net_flow', placeholder: 'min' }}
        right={{ key: 'max_leg_net_flow', placeholder: 'max' }}
        params={params}
        onChange={onChange}
      />

      <label className={swingParamLabelClassName}>
        Big tx (SOL)
        <Input
          fieldSize="md"
          variant="card"
          className="min-w-0 font-normal normal-case tracking-normal"
          type="number"
          min={0}
          step="any"
          value={params.big_tx_sol}
          onChange={(e) => onChange('big_tx_sol', e.target.value)}
        />
      </label>
    </div>
  );
}
