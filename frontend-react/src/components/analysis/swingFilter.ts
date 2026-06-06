import type { SwingLegRecord, SwingLegType } from 'types';

export type SwingLegTypeFilter = 'all' | SwingLegType;

/** Post-detection visibility filter (0 on a bound = ignore that bound). */
export interface SwingFilterCriteria {
  leg_type: SwingLegTypeFilter;
  filter_min_duration_ms: number;
  filter_max_duration_ms: number;
  filter_min_trades: number;
  filter_max_trades: number;
  filter_min_volume_sol: number;
  filter_max_volume_sol: number;
  filter_min_net_flow_sol: number;
  filter_max_net_flow_sol: number;
  filter_min_change_pct: number;
  filter_max_change_pct: number;
}

export const DEFAULT_SWING_FILTER: SwingFilterCriteria = {
  leg_type: 'all',
  filter_min_duration_ms: 0,
  filter_max_duration_ms: 0,
  filter_min_trades: 0,
  filter_max_trades: 0,
  filter_min_volume_sol: 0,
  filter_max_volume_sol: 0,
  filter_min_net_flow_sol: 0,
  filter_max_net_flow_sol: 0,
  filter_min_change_pct: 0,
  filter_max_change_pct: 0,
};

const SWING_FILTER_INT_KEYS = new Set<keyof SwingFilterCriteria>([
  'filter_min_duration_ms',
  'filter_max_duration_ms',
  'filter_min_trades',
  'filter_max_trades',
]);

/** Editable form shape: numeric bounds may be empty while the user is typing. */
export type SwingFilterForm = {
  [K in keyof SwingFilterCriteria]: SwingFilterCriteria[K] extends number
    ? number | ''
    : SwingFilterCriteria[K];
};

/** Coerce an in-progress form into criteria, treating empty bounds as 0 (ignored). */
export function swingFilterFromForm(form: SwingFilterForm): SwingFilterCriteria {
  const out: SwingFilterCriteria = { ...DEFAULT_SWING_FILTER, leg_type: form.leg_type };
  for (const key of Object.keys(DEFAULT_SWING_FILTER) as (keyof SwingFilterCriteria)[]) {
    if (key === 'leg_type') continue;
    const value = form[key];
    out[key] = typeof value === 'number' ? value : 0;
  }
  return out;
}

function legVolume(leg: SwingLegRecord): number {
  return leg.inflow + leg.outflow;
}

function legPriceChangePct(leg: SwingLegRecord): number {
  if (leg.start_price === 0) return 0;
  return ((leg.end_price - leg.start_price) / leg.start_price) * 100;
}

function boundActive(value: number): boolean {
  return value !== 0;
}

export function hasActiveSwingFilter(criteria: SwingFilterCriteria): boolean {
  return (
    criteria.leg_type !== 'all' ||
    boundActive(criteria.filter_min_duration_ms) ||
    boundActive(criteria.filter_max_duration_ms) ||
    boundActive(criteria.filter_min_trades) ||
    boundActive(criteria.filter_max_trades) ||
    boundActive(criteria.filter_min_volume_sol) ||
    boundActive(criteria.filter_max_volume_sol) ||
    boundActive(criteria.filter_min_net_flow_sol) ||
    boundActive(criteria.filter_max_net_flow_sol) ||
    boundActive(criteria.filter_min_change_pct) ||
    boundActive(criteria.filter_max_change_pct)
  );
}

export function filterSwings(
  legs: SwingLegRecord[],
  criteria: SwingFilterCriteria,
): SwingLegRecord[] {
  if (!hasActiveSwingFilter(criteria)) return legs;

  return legs.filter((leg) => {
    if (criteria.leg_type !== 'all' && leg.type !== criteria.leg_type) return false;

    if (
      boundActive(criteria.filter_min_duration_ms) &&
      leg.duration_ms < criteria.filter_min_duration_ms
    ) {
      return false;
    }
    if (
      boundActive(criteria.filter_max_duration_ms) &&
      leg.duration_ms > criteria.filter_max_duration_ms
    ) {
      return false;
    }

    if (
      boundActive(criteria.filter_min_trades) &&
      leg.trade_count < criteria.filter_min_trades
    ) {
      return false;
    }
    if (
      boundActive(criteria.filter_max_trades) &&
      leg.trade_count > criteria.filter_max_trades
    ) {
      return false;
    }

    const volume = legVolume(leg);
    if (boundActive(criteria.filter_min_volume_sol) && volume < criteria.filter_min_volume_sol) {
      return false;
    }
    if (boundActive(criteria.filter_max_volume_sol) && volume > criteria.filter_max_volume_sol) {
      return false;
    }

    if (
      boundActive(criteria.filter_min_net_flow_sol) &&
      leg.net_flow < criteria.filter_min_net_flow_sol
    ) {
      return false;
    }
    if (
      boundActive(criteria.filter_max_net_flow_sol) &&
      leg.net_flow > criteria.filter_max_net_flow_sol
    ) {
      return false;
    }

    const pct = legPriceChangePct(leg);
    if (boundActive(criteria.filter_min_change_pct) && pct < criteria.filter_min_change_pct) {
      return false;
    }
    if (boundActive(criteria.filter_max_change_pct) && pct > criteria.filter_max_change_pct) {
      return false;
    }

    return true;
  });
}

export function parseSwingFilterField<K extends keyof SwingFilterForm>(
  key: K,
  raw: string,
  prev: SwingFilterForm,
): SwingFilterForm[K] {
  if (key === 'leg_type') {
    const v = raw as SwingLegTypeFilter;
    return (v === 'swing_high' || v === 'swing_low' || v === 'all' ? v : 'all') as SwingFilterForm[K];
  }
  if (raw === '') return '' as SwingFilterForm[K];
  const parsed = SWING_FILTER_INT_KEYS.has(key as keyof SwingFilterCriteria)
    ? parseInt(raw, 10)
    : parseFloat(raw);
  return (Number.isFinite(parsed) ? parsed : prev[key]) as SwingFilterForm[K];
}
