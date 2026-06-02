import type { TokenRecord } from '../../types';
import { isoHoursAgo } from '../../utils/date';

export interface TokenFilters {
  age_min: string;
  age_max: string;
  last_trade_min: string;
  last_trade_max: string;
  ath_age_min: string;
  ath_age_max: string;
  ath_fep_min: string;
  ath_fep_max: string;
  cur_fep_min: string;
  cur_fep_max: string;
  ath_price_min: string;
  ath_price_max: string;
  price_min: string;
  price_max: string;
  volume_min: string;
  volume_max: string;
  mcap_min: string;
  mcap_max: string;
  init_buy_min: string;
  init_buy_max: string;
  init_supply_min: string;
  init_supply_max: string;
  token_amount_min: string;
  token_amount_max: string;
  max_sol_cost_min: string;
  max_sol_cost_max: string;
  spendable_sol_in_min: string;
  spendable_sol_in_max: string;
  min_tokens_out_min: string;
  min_tokens_out_max: string;
  trades_min: string;
  trades_max: string;
  cu_limit_min: string;
  cu_limit_max: string;
  cu_price_min: string;
  cu_price_max: string;
  ix_count_min: string;
  ix_count_max: string;
  ix_label: string;
  migrated: string;
  creator: string;
}

export const defaultFilters = (): TokenFilters => ({
  age_min: '',
  age_max: '',
  last_trade_min: '',
  last_trade_max: '',
  ath_age_min: '',
  ath_age_max: '',
  ath_fep_min: '',
  ath_fep_max: '',
  cur_fep_min: '',
  cur_fep_max: '',
  ath_price_min: '',
  ath_price_max: '',
  price_min: '',
  price_max: '',
  volume_min: '',
  volume_max: '',
  mcap_min: '',
  mcap_max: '',
  init_buy_min: '',
  init_buy_max: '',
  init_supply_min: '',
  init_supply_max: '',
  token_amount_min: '',
  token_amount_max: '',
  max_sol_cost_min: '',
  max_sol_cost_max: '',
  spendable_sol_in_min: '',
  spendable_sol_in_max: '',
  min_tokens_out_min: '',
  min_tokens_out_max: '',
  trades_min: '',
  trades_max: '',
  cu_limit_min: '',
  cu_limit_max: '',
  cu_price_min: '',
  cu_price_max: '',
  ix_count_min: '',
  ix_count_max: '',
  ix_label: '',
  migrated: '',
  creator: '',
});

function rangeF64(val: number, min: string, max: string): boolean {
  if (min) {
    const v = parseFloat(min);
    if (!Number.isNaN(v) && val < v) return false;
  }
  if (max) {
    const v = parseFloat(max);
    if (!Number.isNaN(v) && val > v) return false;
  }
  return true;
}

function optF64(opt: number | null | undefined, min: string, max: string): boolean {
  if (!min && !max) return true;
  if (opt == null) return false;
  return rangeF64(opt, min, max);
}

function fep(t: TokenRecord): number | null {
  if (t.initial_buy_sol == null || t.initial_supply_token == null || t.initial_supply_token <= 0) {
    return null;
  }
  return t.initial_buy_sol / t.initial_supply_token;
}

export function filtersEmpty(f: TokenFilters): boolean {
  return Object.values(f).every((v) => !v);
}

export function activeFilterCount(f: TokenFilters): number {
  const groups = [
    f.age_min || f.age_max,
    f.last_trade_min || f.last_trade_max,
    f.ath_age_min || f.ath_age_max,
    f.ath_fep_min || f.ath_fep_max,
    f.cur_fep_min || f.cur_fep_max,
    f.ath_price_min || f.ath_price_max,
    f.price_min || f.price_max,
    f.volume_min || f.volume_max,
    f.mcap_min || f.mcap_max,
    f.init_buy_min || f.init_buy_max,
    f.init_supply_min || f.init_supply_max,
    f.token_amount_min || f.token_amount_max,
    f.max_sol_cost_min || f.max_sol_cost_max,
    f.spendable_sol_in_min || f.spendable_sol_in_max,
    f.min_tokens_out_min || f.min_tokens_out_max,
    f.trades_min || f.trades_max,
    f.cu_limit_min || f.cu_limit_max,
    f.cu_price_min || f.cu_price_max,
    f.ix_count_min || f.ix_count_max,
    f.migrated,
    f.ix_label,
    f.creator,
  ];
  return groups.filter(Boolean).length;
}

export function tokenPassesFilters(f: TokenFilters, t: TokenRecord): boolean {
  if (!rangeF64(t.age / 3600, f.age_min, f.age_max)) return false;

  if (f.last_trade_min || f.last_trade_max) {
    const h = t.last_trade_at ? isoHoursAgo(t.last_trade_at) : null;
    if (h == null || !rangeF64(h, f.last_trade_min, f.last_trade_max)) return false;
  }

  if (f.ath_age_min || f.ath_age_max) {
    const h = t.ath_timestamp ? isoHoursAgo(t.ath_timestamp) : null;
    if (h == null || !rangeF64(h, f.ath_age_min, f.ath_age_max)) return false;
  }

  const entry = fep(t);
  const athFep =
    entry != null && entry > 0 && t.ath_price != null ? t.ath_price / entry : null;
  const curFep =
    entry != null && entry > 0 && t.current_price != null ? t.current_price / entry : null;

  if (!optF64(athFep, f.ath_fep_min, f.ath_fep_max)) return false;
  if (!optF64(curFep, f.cur_fep_min, f.cur_fep_max)) return false;
  if (!optF64(t.ath_price, f.ath_price_min, f.ath_price_max)) return false;
  if (!optF64(t.current_price, f.price_min, f.price_max)) return false;
  if (!rangeF64(t.volume_sol_total, f.volume_min, f.volume_max)) return false;
  if (!optF64(t.market_cap, f.mcap_min, f.mcap_max)) return false;
  if (!optF64(t.initial_buy_sol, f.init_buy_min, f.init_buy_max)) return false;
  if (!optF64(t.initial_supply_token, f.init_supply_min, f.init_supply_max)) return false;
  if (!optF64(t.token_amount, f.token_amount_min, f.token_amount_max)) return false;
  if (!optF64(t.max_sol_cost, f.max_sol_cost_min, f.max_sol_cost_max)) return false;
  if (!optF64(t.spendable_sol_in, f.spendable_sol_in_min, f.spendable_sol_in_max)) return false;
  if (!optF64(t.min_tokens_out, f.min_tokens_out_min, f.min_tokens_out_max)) return false;
  if (!rangeF64(t.trade_count, f.trades_min, f.trades_max)) return false;
  if (!optF64(t.cu_limit, f.cu_limit_min, f.cu_limit_max)) return false;
  if (!optF64(t.cu_price, f.cu_price_min, f.cu_price_max)) return false;
  if (!rangeF64(t.ix_labels_count, f.ix_count_min, f.ix_count_max)) return false;

  if (f.ix_label) {
    const needle = f.ix_label.toLowerCase();
    const labels = Array.isArray(t.instruction_labels)
      ? (t.instruction_labels as unknown[])
      : (t.instruction_labels as { instructions?: unknown[] })?.instructions ?? [];
    const matched = labels.some((v) => String(v).toLowerCase().includes(needle));
    if (!matched) return false;
  }

  if (f.migrated === 'yes' && !t.is_migrated) return false;
  if (f.migrated === 'no' && t.is_migrated) return false;

  if (f.creator && !t.creator_address.toLowerCase().includes(f.creator.toLowerCase())) {
    return false;
  }

  return true;
}
