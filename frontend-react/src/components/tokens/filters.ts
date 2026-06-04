import type { TokenRecord } from '../../types';

export type TriState = '' | 'yes' | 'no';

export interface TokenFilters {
  // Identity (case-insensitive substring)
  symbol: string;
  name: string;
  mint: string;
  creator: string;
  create_tx: string;
  // Time (datetime-local "YYYY-MM-DDTHH:mm", interpreted as UTC)
  created_from: string;
  created_to: string;
  last_trade_from: string;
  last_trade_to: string;
  ath_from: string;
  ath_to: string;
  // Performance
  ath_fep_min: string;
  ath_fep_max: string;
  cur_fep_min: string;
  cur_fep_max: string;
  ath_price_min: string;
  ath_price_max: string;
  price_min: string;
  price_max: string;
  // Market
  volume_min: string;
  volume_max: string;
  mcap_min: string;
  mcap_max: string;
  trades_min: string;
  trades_max: string;
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
  // Technical
  cu_limit_min: string;
  cu_limit_max: string;
  cu_price_min: string;
  cu_price_max: string;
  ix_count_min: string;
  ix_count_max: string;
  ix_label: string;
  // Flags
  migrated: TriState;
  mayhem: TriState;
  cashback: TriState;
}

export const LS_TOKEN_FILTERS_KEY = 'tokens_global_filters';

const TRI_VALUES = new Set<TriState>(['', 'yes', 'no']);

function mergeTokenFilters(partial?: Partial<TokenFilters>): TokenFilters {
  const base = defaultFilters();
  if (!partial || typeof partial !== 'object') return base;
  const merged = { ...base };
  for (const key of Object.keys(base) as (keyof TokenFilters)[]) {
    const v = partial[key];
    if (v === undefined) continue;
    if (key === 'migrated' || key === 'mayhem' || key === 'cashback') {
      if (TRI_VALUES.has(v as TriState)) merged[key] = v as TriState;
    } else if (typeof v === 'string') {
      merged[key] = v;
    }
  }
  return merged;
}

export function loadStoredTokenFilters(): TokenFilters {
  try {
    const raw = localStorage.getItem(LS_TOKEN_FILTERS_KEY);
    if (!raw) return defaultFilters();
    return mergeTokenFilters(JSON.parse(raw) as Partial<TokenFilters>);
  } catch {
    return defaultFilters();
  }
}

export function saveStoredTokenFilters(f: TokenFilters): void {
  try {
    if (filtersEmpty(f)) {
      localStorage.removeItem(LS_TOKEN_FILTERS_KEY);
    } else {
      localStorage.setItem(LS_TOKEN_FILTERS_KEY, JSON.stringify(f));
    }
  } catch {
    /* ignore */
  }
}

export const defaultFilters = (): TokenFilters => ({
  symbol: '',
  name: '',
  mint: '',
  creator: '',
  create_tx: '',
  created_from: '',
  created_to: '',
  last_trade_from: '',
  last_trade_to: '',
  ath_from: '',
  ath_to: '',
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
  trades_min: '',
  trades_max: '',
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
  cu_limit_min: '',
  cu_limit_max: '',
  cu_price_min: '',
  cu_price_max: '',
  ix_count_min: '',
  ix_count_max: '',
  ix_label: '',
  migrated: '',
  mayhem: '',
  cashback: '',
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

/** datetime-local value -> epoch ms, interpreted as UTC to match the UTC table display. */
function parseDt(v: string): number | null {
  if (!v) return null;
  const iso = v.length === 16 ? `${v}:00Z` : `${v}Z`;
  const ms = Date.parse(iso);
  return Number.isNaN(ms) ? null : ms;
}

function dateInRange(iso: string | null | undefined, from: string, to: string): boolean {
  if (!from && !to) return true;
  if (!iso) return false;
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return false;
  const f = parseDt(from);
  if (f != null && t < f) return false;
  const u = parseDt(to);
  if (u != null && t > u) return false;
  return true;
}

function textMatch(value: string | null | undefined, needle: string): boolean {
  if (!needle) return true;
  return (value ?? '').toLowerCase().includes(needle.toLowerCase().trim());
}

function triMatch(value: boolean, tri: TriState): boolean {
  if (tri === 'yes') return value;
  if (tri === 'no') return !value;
  return true;
}

function fep(t: TokenRecord): number | null {
  if (t.initial_buy_sol == null || t.initial_supply_token == null || t.initial_supply_token <= 0) {
    return null;
  }
  return t.initial_buy_sol / t.initial_supply_token;
}

function ixLabelStrings(raw: unknown): string[] {
  const arr = Array.isArray(raw)
    ? (raw as unknown[])
    : ((raw as { instructions?: unknown[] })?.instructions ?? []);
  return arr.map((v) => String(v));
}

function ixLabelList(t: TokenRecord): string[] {
  return ixLabelStrings(t.instruction_labels).map((s) => s.toLowerCase());
}

type IxLabelFilter =
  | { kind: 'none' }
  | { kind: 'text'; needles: string[] }
  | { kind: 'json'; needles: string[] };

/** Plain lines/comma list (substring, any) vs pasted JSON array/object (ordered exact). */
function parseIxLabelFilter(raw: string): IxLabelFilter {
  const trimmed = raw.trim();
  if (!trimmed) return { kind: 'none' };

  if (trimmed.startsWith('[') || trimmed.startsWith('{')) {
    try {
      const parsed = JSON.parse(trimmed) as unknown;
      const arr = Array.isArray(parsed)
        ? parsed
        : (parsed as { instructions?: unknown[] })?.instructions;
      if (Array.isArray(arr)) {
        const needles = arr.map((v) => String(v).trim()).filter(Boolean);
        if (needles.length > 0) return { kind: 'json', needles };
      }
    } catch {
      /* fall through to text mode */
    }
  }

  const needles = trimmed
    .split(/[\n,]/)
    .map((s) => s.trim().toLowerCase())
    .filter(Boolean);
  return needles.length > 0 ? { kind: 'text', needles } : { kind: 'none' };
}

function ixLabelsMatchJson(needles: string[], labels: string[]): boolean {
  const want = needles.map((n) => n.toLowerCase());
  if (want.length !== labels.length) return false;
  return want.every((n, i) => labels[i] === n);
}

function ixLabelsMatchText(needles: string[], labels: string[]): boolean {
  return needles.some((n) => labels.some((l) => l.includes(n)));
}

export function filtersEmpty(f: TokenFilters): boolean {
  return Object.values(f).every((v) => !v);
}

/** Number of distinct active filter groups (range pairs count once). */
export function activeFilterCount(f: TokenFilters): number {
  const groups = [
    f.symbol,
    f.name,
    f.mint,
    f.creator,
    f.create_tx,
    f.created_from || f.created_to,
    f.last_trade_from || f.last_trade_to,
    f.ath_from || f.ath_to,
    f.ath_fep_min || f.ath_fep_max,
    f.cur_fep_min || f.cur_fep_max,
    f.ath_price_min || f.ath_price_max,
    f.price_min || f.price_max,
    f.volume_min || f.volume_max,
    f.mcap_min || f.mcap_max,
    f.trades_min || f.trades_max,
    f.init_buy_min || f.init_buy_max,
    f.init_supply_min || f.init_supply_max,
    f.token_amount_min || f.token_amount_max,
    f.max_sol_cost_min || f.max_sol_cost_max,
    f.spendable_sol_in_min || f.spendable_sol_in_max,
    f.min_tokens_out_min || f.min_tokens_out_max,
    f.cu_limit_min || f.cu_limit_max,
    f.cu_price_min || f.cu_price_max,
    f.ix_count_min || f.ix_count_max,
    f.ix_label.trim(),
    f.migrated,
    f.mayhem,
    f.cashback,
  ];
  return groups.filter(Boolean).length;
}

export function tokenPassesFilters(f: TokenFilters, t: TokenRecord): boolean {
  // Identity
  if (!textMatch(t.symbol, f.symbol)) return false;
  if (!textMatch(t.name, f.name)) return false;
  if (!textMatch(t.mint_address, f.mint)) return false;
  if (!textMatch(t.creator_address, f.creator)) return false;
  if (!textMatch(t.create_tx_address, f.create_tx)) return false;

  // Time
  if (!dateInRange(t.created_at, f.created_from, f.created_to)) return false;
  if (!dateInRange(t.last_trade_at, f.last_trade_from, f.last_trade_to)) return false;
  if (!dateInRange(t.ath_timestamp, f.ath_from, f.ath_to)) return false;

  // Performance
  const entry = fep(t);
  const athFep =
    entry != null && entry > 0 && t.ath_price != null ? t.ath_price / entry : null;
  const curFep =
    entry != null && entry > 0 && t.current_price != null ? t.current_price / entry : null;
  if (!optF64(athFep, f.ath_fep_min, f.ath_fep_max)) return false;
  if (!optF64(curFep, f.cur_fep_min, f.cur_fep_max)) return false;
  if (!optF64(t.ath_price, f.ath_price_min, f.ath_price_max)) return false;
  if (!optF64(t.current_price, f.price_min, f.price_max)) return false;

  // Market
  if (!rangeF64(t.volume_sol_total, f.volume_min, f.volume_max)) return false;
  if (!optF64(t.market_cap, f.mcap_min, f.mcap_max)) return false;
  if (!rangeF64(t.trade_count, f.trades_min, f.trades_max)) return false;
  if (!optF64(t.initial_buy_sol, f.init_buy_min, f.init_buy_max)) return false;
  if (!optF64(t.initial_supply_token, f.init_supply_min, f.init_supply_max)) return false;
  if (!optF64(t.token_amount, f.token_amount_min, f.token_amount_max)) return false;
  // max_sol_cost / spendable_sol_in are lamports; filter in SOL to match the table.
  if (!optF64(t.max_sol_cost != null ? t.max_sol_cost / 1e9 : null, f.max_sol_cost_min, f.max_sol_cost_max)) return false;
  if (!optF64(t.spendable_sol_in != null ? t.spendable_sol_in / 1e9 : null, f.spendable_sol_in_min, f.spendable_sol_in_max)) return false;
  if (!optF64(t.min_tokens_out, f.min_tokens_out_min, f.min_tokens_out_max)) return false;

  // Technical
  if (!optF64(t.cu_limit, f.cu_limit_min, f.cu_limit_max)) return false;
  if (!optF64(t.cu_price, f.cu_price_min, f.cu_price_max)) return false;
  if (!rangeF64(t.ix_labels_count, f.ix_count_min, f.ix_count_max)) return false;

  const ixFilter = parseIxLabelFilter(f.ix_label);
  if (ixFilter.kind !== 'none') {
    const labels = ixLabelList(t);
    const matched =
      ixFilter.kind === 'json'
        ? ixLabelsMatchJson(ixFilter.needles, labels)
        : ixLabelsMatchText(ixFilter.needles, labels);
    if (!matched) return false;
  }

  // Flags
  if (!triMatch(t.is_migrated, f.migrated)) return false;
  if (!triMatch(t.is_mayhem_mode, f.mayhem)) return false;
  if (!triMatch(t.is_cashback_enabled, f.cashback)) return false;

  return true;
}
