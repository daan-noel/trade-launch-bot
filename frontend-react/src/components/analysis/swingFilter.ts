import type { SwingLegRecord, SwingLegType } from '../../types';

export type SwingTypeFilter = 'all' | SwingLegType;

export interface SwingFilterCriteria {
  type: SwingTypeFilter;
  min_duration_ms: number;
  max_duration_ms: number;
  min_trade_count: number;
  max_trade_count: number;
  min_volume: number;
  max_volume: number;
  min_net_flow: number;
  max_net_flow: number;
  min_price_change_pct: number;
  max_price_change_pct: number;
}

export const DEFAULT_SWING_FILTER: SwingFilterCriteria = {
  type: 'all',
  min_duration_ms: 0,
  max_duration_ms: 0,
  min_trade_count: 0,
  max_trade_count: 0,
  min_volume: 0,
  max_volume: 0,
  min_net_flow: 0,
  max_net_flow: 0,
  min_price_change_pct: 0,
  max_price_change_pct: 0,
};

const SWING_FILTER_INT_KEYS = new Set<keyof SwingFilterCriteria>([
  'min_duration_ms',
  'max_duration_ms',
  'min_trade_count',
  'max_trade_count',
]);

function legVolume(leg: SwingLegRecord): number {
  return leg.inflow + leg.outflow;
}

function legPriceChangePct(leg: SwingLegRecord): number {
  if (leg.start_price === 0) return 0;
  return ((leg.end_price - leg.start_price) / leg.start_price) * 100;
}

export function hasActiveSwingFilter(criteria: SwingFilterCriteria): boolean {
  return (
    criteria.type !== 'all' ||
    criteria.min_duration_ms > 0 ||
    criteria.max_duration_ms > 0 ||
    criteria.min_trade_count > 0 ||
    criteria.max_trade_count > 0 ||
    criteria.min_volume > 0 ||
    criteria.max_volume > 0 ||
    criteria.min_net_flow !== 0 ||
    criteria.max_net_flow !== 0 ||
    criteria.min_price_change_pct !== 0 ||
    criteria.max_price_change_pct !== 0
  );
}

export function filterSwings(
  legs: SwingLegRecord[],
  criteria: SwingFilterCriteria,
): SwingLegRecord[] {
  if (!hasActiveSwingFilter(criteria)) return legs;

  return legs.filter((leg) => {
    if (criteria.type !== 'all' && leg.type !== criteria.type) return false;

    if (criteria.min_duration_ms > 0 && leg.duration_ms < criteria.min_duration_ms) {
      return false;
    }
    if (criteria.max_duration_ms > 0 && leg.duration_ms > criteria.max_duration_ms) {
      return false;
    }

    if (criteria.min_trade_count > 0 && leg.trade_count < criteria.min_trade_count) {
      return false;
    }
    if (criteria.max_trade_count > 0 && leg.trade_count > criteria.max_trade_count) {
      return false;
    }

    const volume = legVolume(leg);
    if (criteria.min_volume > 0 && volume < criteria.min_volume) return false;
    if (criteria.max_volume > 0 && volume > criteria.max_volume) return false;

    if (criteria.min_net_flow !== 0 && leg.net_flow < criteria.min_net_flow) return false;
    if (criteria.max_net_flow !== 0 && leg.net_flow > criteria.max_net_flow) return false;

    const pct = legPriceChangePct(leg);
    if (criteria.min_price_change_pct !== 0 && pct < criteria.min_price_change_pct) {
      return false;
    }
    if (criteria.max_price_change_pct !== 0 && pct > criteria.max_price_change_pct) {
      return false;
    }

    return true;
  });
}

export function parseSwingFilterField<K extends keyof SwingFilterCriteria>(
  key: K,
  raw: string,
  prev: SwingFilterCriteria,
): SwingFilterCriteria[K] {
  if (key === 'type') {
    const v = raw as SwingTypeFilter;
    return (v === 'swing_high' || v === 'swing_low' || v === 'all' ? v : 'all') as SwingFilterCriteria[K];
  }
  const parsed = SWING_FILTER_INT_KEYS.has(key) ? parseInt(raw, 10) : parseFloat(raw);
  return (Number.isFinite(parsed) ? parsed : prev[key]) as SwingFilterCriteria[K];
}
