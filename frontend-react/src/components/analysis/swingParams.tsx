import type { ReactNode } from 'react';
import { Input } from 'components/ui/Input';
import { InfoTooltip } from 'components/ui/InfoTooltip';
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

/** Per-field tooltip copy. Keyed by the field's representative (left) {@link SwingParams}
 *  key — paired SOL/% and min/max cells share one entry whose body covers both sides.
 *  Looked up automatically by {@link SwingRangeField} and the Big-tx field. */
export const SWING_PARAM_HELP: Partial<Record<keyof SwingParams, { title: string; body: string }>> = {
  high_to_low_threshold_sol: {
    title: 'High → low reversal',
    body:
      'Confirms a swing high (buy-dominant) flipping into a swing low. While in the high, sell SOL is accumulated; the reversal fires when it reaches whichever is smaller — the SOL value (left) or that percent (right) of the current high’s net flow when selling began. Set a term to 0 to ignore it. Lower = more sensitive (more, shorter legs).',
  },
  low_to_high_threshold_sol: {
    title: 'Low → high reversal',
    body:
      'Confirms a swing low (sell-dominant) flipping into a swing high. Buy SOL accumulated during the low triggers the flip when it reaches whichever is smaller — the SOL value (left) or that percent (right) of the current low’s net flow when buying began. Set a term to 0 to ignore it. Lower = more sensitive.',
  },
  min_leg_trades: {
    title: 'Leg trades (min / max)',
    body:
      'Quality filter on the number of trades in a leg. Legs with fewer than min or more than max trades are dropped after detection. A high→low pair is removed if either side fails. Min 0 and max 0 each mean “no bound”. Raise min to discard 1–2 trade blips.',
  },
  min_leg_duration_ms: {
    title: 'Leg duration (min / max, ms)',
    body:
      'Quality filter on a leg’s wall-clock span (end − start) in milliseconds. Legs shorter than min or longer than max are filtered out; a high→low pair is dropped if either fails. 0 = no bound. Use to keep sustained moves or exclude very long drifts.',
  },
  min_leg_volume: {
    title: 'Leg volume (min / max, SOL)',
    body:
      'Quality filter on a leg’s total traded SOL — inflow + outflow, both sides combined. Legs outside [min, max] are filtered; a high→low pair is dropped if either fails. 0 = no bound. Raise min to keep only legs with real money behind them.',
  },
  min_leg_net_flow: {
    title: 'Leg net flow (min / max, SOL)',
    body:
      'Quality filter on |net flow| = |inflow − outflow|, a leg’s net directional pressure. Compared by magnitude so swing lows (negative) use the same positive bounds. Outside [min, max] → filtered; pair dropped if either fails. 0 = no bound.',
  },
  swing_high_min_delta_pct: {
    title: 'Swing-high delta (min / max, %)',
    body:
      'Quality filter on the leg’s price change: |(end − start) / start| × 100. For a swing high this is the size of the rise. Keeps legs that moved at least min% and at most max%. 0 = no bound.',
  },
  swing_high_min_net_flow_per_sec: {
    title: 'Swing-high net flow / s (min / max, SOL)',
    body:
      'Quality filter on net-flow rate: |net flow| ÷ leg duration in seconds — how fast buy pressure built. Keeps legs whose rate is within [min, max] SOL/s. Instant (0-duration) legs skip this bound. 0 = no bound.',
  },
  swing_low_min_delta_pct: {
    title: 'Swing-low delta (min / max, %)',
    body:
      'Quality filter on the leg’s price change by magnitude: |(end − start) / start| × 100. For a swing low this is the size of the drop, e.g. min 30 keeps only lows that fell ≥ 30%. 0 = no bound.',
  },
  swing_low_min_net_flow_per_sec: {
    title: 'Swing-low net flow / s (min / max, SOL)',
    body:
      'Quality filter on net-flow rate by magnitude: |net flow| ÷ leg duration in seconds — how fast sell pressure drained SOL. Keeps legs within [min, max] SOL/s. Instant (0-duration) legs skip this bound. 0 = no bound.',
  },
  big_tx_sol: {
    title: 'Big tx (SOL)',
    body:
      'Marks a single trade as “big” when its SOL ≥ this value. A big tx (1) instantly confirms a reversal on its own — independent of the thresholds above — so large buys/sells always start a new leg; and (2) anchors the leg’s drawn end point to the last big same-side tx (the real pump/dump) instead of a trailing dust trade. 0 = disabled (end snaps to the leg’s price extreme).',
  },
};

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
  label: ReactNode;
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
  const help = SWING_PARAM_HELP[left.key];
  const labelNode = help ? (
    <span className="inline-flex items-center gap-1">
      {label}
      <InfoTooltip title={help.title} body={help.body} />
    </span>
  ) : (
    label
  );
  return <RangeInputs label={labelNode} separator={separator} left={side(left)} right={side(right)} />;
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
        <span className="inline-flex items-center gap-1">
          Big tx (SOL)
          {SWING_PARAM_HELP.big_tx_sol && (
            <InfoTooltip
              title={SWING_PARAM_HELP.big_tx_sol.title}
              body={SWING_PARAM_HELP.big_tx_sol.body}
            />
          )}
        </span>
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
