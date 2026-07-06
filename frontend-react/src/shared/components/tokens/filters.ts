import type { TokenRecord } from 'types';
import type { FilterSpec } from 'components/table/numericFilter';
import { datetimeLocalToUtcWallClock } from 'utils/date';
import { STORAGE_KEYS, getString, setString, remove } from 'lib/storage';

export type TriState = '' | 'yes' | 'no';

export interface TokenFilters {
  // Identity (case-insensitive substring)
  symbol: string;
  name: string;
  mint_address: string;
  creator: string;
  create_tx: string;
  // Time (datetime-local "YYYY-MM-DDTHH:mm"). Stored as wall-clock in the
  // selected project timezone; normalized to the exact UTC instant at the
  // getTokensPage query boundary via datetimeLocalToUtcWallClock.
  created_from: string;
  created_to: string;
  last_trade_from: string;
  last_trade_to: string;
  ath_from: string;
  ath_to: string;
  // Lifetime in minutes (last trade − creation); applies to dead tokens only
  life_min: string;
  life_max: string;
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
  max_cost_lamports_min: string;
  max_cost_lamports_max: string;
  spendable_lamports_in_min: string;
  spendable_lamports_in_max: string;
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
  dead: TriState;
  mayhem: TriState;
  cashback: TriState;
}

export const LS_TOKEN_FILTERS_KEY = STORAGE_KEYS.tokenFilters;

const TRI_VALUES = new Set<TriState>(['', 'yes', 'no']);

function mergeTokenFilters(partial?: Partial<TokenFilters>): TokenFilters {
  const base = defaultFilters();
  if (!partial || typeof partial !== 'object') return base;
  const merged = { ...base };
  for (const key of Object.keys(base) as (keyof TokenFilters)[]) {
    const v = partial[key];
    if (v === undefined) continue;
    if (key === 'migrated' || key === 'dead' || key === 'mayhem' || key === 'cashback') {
      if (TRI_VALUES.has(v as TriState)) merged[key] = v as TriState;
    } else if (typeof v === 'string') {
      merged[key] = v;
    }
  }
  return merged;
}

export function loadStoredTokenFilters(): TokenFilters {
  try {
    const raw = getString(LS_TOKEN_FILTERS_KEY);
    if (!raw) return defaultFilters();
    return mergeTokenFilters(JSON.parse(raw) as Partial<TokenFilters>);
  } catch {
    return defaultFilters();
  }
}

export function saveStoredTokenFilters(f: TokenFilters): void {
  if (filtersEmpty(f)) {
    remove(LS_TOKEN_FILTERS_KEY);
  } else {
    setString(LS_TOKEN_FILTERS_KEY, JSON.stringify(f));
  }
}

export const defaultFilters = (): TokenFilters => ({
  symbol: '',
  name: '',
  mint_address: '',
  creator: '',
  create_tx: '',
  created_from: '',
  created_to: '',
  last_trade_from: '',
  last_trade_to: '',
  ath_from: '',
  ath_to: '',
  life_min: '',
  life_max: '',
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
  max_cost_lamports_min: '',
  max_cost_lamports_max: '',
  spendable_lamports_in_min: '',
  spendable_lamports_in_max: '',
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
  dead: '',
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

/**
 * datetime-local value -> epoch ms, interpreted as UTC. NOTE: this (and
 * `dateInRange`/`tokenPassesFilters`) has no live call sites — both pages filter
 * server-side; only `activeFilterCount` is still used. Any future client-side
 * caller must first pre-convert the picker value via `datetimeLocalToUtcWallClock`
 * (utils/date.ts), since the picker now means wall-clock in the selected project
 * timezone, not UTC.
 */
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

/**
 * Inactivity window after which a token's lifetime is treated as "final" for the
 * short-lived (life_min/life_max) filter. Distinct from the `is_dead` flag, which
 * is the backend's richer liquidity/price/volume verdict.
 */
const LIFETIME_STALE_MS = 60 * 60 * 1000;

/**
 * Token lifetime in minutes. Returns null (→ "not short-lived", keep the token) when it
 * can't be determined (no last trade / unparseable timestamps) or when the token is still
 * alive (last trade within LIFETIME_STALE_MS), since a live token's lifetime isn't final.
 *
 * Prefers the backend's gap-aware `lifetime_secs` (creation → last non-stray trade,
 * stripping lone trades after the token went quiet) and falls back to the raw
 * last-trade − creation span when the field is absent.
 */
function lifetimeMinutes(t: TokenRecord): number | null {
  if (!t.last_trade_at) return null;
  const last = Date.parse(t.last_trade_at);
  if (Number.isNaN(last)) return null;
  if (Date.now() - last < LIFETIME_STALE_MS) return null; // still trading → exempt
  if (t.lifetime_secs != null) return t.lifetime_secs / 60;
  const created = Date.parse(t.created_at);
  if (Number.isNaN(created)) return null;
  return (last - created) / 60_000;
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
    f.mint_address,
    f.creator,
    f.create_tx,
    f.created_from || f.created_to,
    f.last_trade_from || f.last_trade_to,
    f.ath_from || f.ath_to,
    f.life_min || f.life_max,
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
    f.max_cost_lamports_min || f.max_cost_lamports_max,
    f.spendable_lamports_in_min || f.spendable_lamports_in_max,
    f.min_tokens_out_min || f.min_tokens_out_max,
    f.cu_limit_min || f.cu_limit_max,
    f.cu_price_min || f.cu_price_max,
    f.ix_count_min || f.ix_count_max,
    f.ix_label.trim(),
    f.migrated,
    f.dead,
    f.mayhem,
    f.cashback,
  ];
  return groups.filter(Boolean).length;
}

// ---------------------------------------------------------------------------
// TokenFilters panel → unified per-column `FilterSpec` map
//
// Folds the global filter panel into the SAME `filters: {col → FilterSpec}` map the
// DataTable per-column filters use, keyed by backend **column** key. Range pairs
// become `between` (or one-sided `gte`/`lte`); tri-state flags become `eq`
// "yes"/"no"; datetime pickers are normalized from project-tz wall-clock to the UTC
// instant the backend expects. The backend `lower_filter` is the exact inverse.
// ---------------------------------------------------------------------------

/** A numeric range pair → `between`/`gte`/`lte`, or `null` when both bounds blank. */
function rangeSpec(min: string, max: string): FilterSpec | null {
  const lo = min.trim();
  const hi = max.trim();
  if (lo && hi) return { op: 'between', min: Number(lo), max: Number(hi) };
  if (lo) return { op: 'gte', val: Number(lo) };
  if (hi) return { op: 'lte', val: Number(hi) };
  return null;
}

/** A datetime range pair → `between`/`gte`/`lte` over tz-normalized UTC wall-clock
 *  strings, or `null` when both bounds blank. */
function dateRangeSpec(from: string, to: string, tz: string): FilterSpec | null {
  const lo = from ? datetimeLocalToUtcWallClock(from, tz, 'lower') : '';
  const hi = to ? datetimeLocalToUtcWallClock(to, tz, 'upper') : '';
  if (lo && hi) return { op: 'between', min: lo, max: hi };
  if (lo) return { op: 'gte', val: lo };
  if (hi) return { op: 'lte', val: hi };
  return null;
}

/** The panel's numeric range groups: `[colKey, minField, maxField]`. `colKey` is the
 *  backend column key (NOT always the panel field prefix — e.g. `mcap`→`market_cap`). */
const NUMERIC_RANGE_GROUPS: [string, keyof TokenFilters, keyof TokenFilters][] = [
  ['ath_fep_ratio', 'ath_fep_min', 'ath_fep_max'],
  ['current_fep_ratio', 'cur_fep_min', 'cur_fep_max'],
  ['ath_price', 'ath_price_min', 'ath_price_max'],
  ['current_price', 'price_min', 'price_max'],
  ['volume', 'volume_min', 'volume_max'],
  ['market_cap', 'mcap_min', 'mcap_max'],
  ['trade_count', 'trades_min', 'trades_max'],
  ['initial_buy', 'init_buy_min', 'init_buy_max'],
  ['init_supply', 'init_supply_min', 'init_supply_max'],
  ['token_amount', 'token_amount_min', 'token_amount_max'],
  ['max_cost_lamports', 'max_cost_lamports_min', 'max_cost_lamports_max'],
  ['spendable_lamports_in', 'spendable_lamports_in_min', 'spendable_lamports_in_max'],
  ['min_tokens_out', 'min_tokens_out_min', 'min_tokens_out_max'],
  ['cu_limit', 'cu_limit_min', 'cu_limit_max'],
  ['cu_price', 'cu_price_min', 'cu_price_max'],
  ['ix_count', 'ix_count_min', 'ix_count_max'],
  // lifetime is minutes with a dead-only exemption — the backend `lifetime` column
  // handles that; here it's just a numeric range like the rest.
  ['lifetime', 'life_min', 'life_max'],
];

/** The panel's datetime range groups: `[colKey, fromField, toField]`. */
const DATE_RANGE_GROUPS: [string, keyof TokenFilters, keyof TokenFilters][] = [
  ['created', 'created_from', 'created_to'],
  ['last_trade', 'last_trade_from', 'last_trade_to'],
  ['ath_timestamp', 'ath_from', 'ath_to'],
];

/** The panel's tri-state flag groups: `[colKey, field]`. */
const FLAG_GROUPS: [string, keyof TokenFilters][] = [
  ['migrated', 'migrated'],
  ['dead', 'dead'],
  ['mayhem_mode', 'mayhem'],
  ['cashback', 'cashback'],
];

/**
 * Serialize the global `TokenFilters` panel into the unified per-column `FilterSpec`
 * map (keyed by backend column key). `timezone` normalizes the datetime pickers to
 * the UTC instant the backend expects. Blank groups are omitted.
 */
export function tokenFiltersToSpecs(f: TokenFilters, timezone: string): Record<string, FilterSpec> {
  const out: Record<string, FilterSpec> = {};

  // Identity: single-field case-insensitive substring.
  for (const [col, field] of [
    ['symbol', 'symbol'],
    ['name', 'name'],
    ['mint_address', 'mint_address'],
    ['creator', 'creator'],
    ['create_tx', 'create_tx'],
  ] as [string, keyof TokenFilters][]) {
    const v = f[field].trim();
    if (v) out[col] = { op: 'contains', val: v };
  }

  for (const [col, minF, maxF] of NUMERIC_RANGE_GROUPS) {
    const spec = rangeSpec(f[minF], f[maxF]);
    if (spec) out[col] = spec;
  }

  for (const [col, fromF, toF] of DATE_RANGE_GROUPS) {
    const spec = dateRangeSpec(f[fromF], f[toF], timezone);
    if (spec) out[col] = spec;
  }

  // Instruction labels (JSON/text grammar handled server-side).
  if (f.ix_label.trim()) out.ix_labels = { op: 'contains', val: f.ix_label.trim() };

  // Flags: tri-state "yes"/"no" (blank = inactive).
  for (const [col, field] of FLAG_GROUPS) {
    const v = f[field];
    if (v === 'yes' || v === 'no') out[col] = { op: 'eq', val: v };
  }

  return out;
}

export function tokenPassesFilters(f: TokenFilters, t: TokenRecord): boolean {
  // Identity
  if (!textMatch(t.symbol, f.symbol)) return false;
  if (!textMatch(t.name, f.name)) return false;
  if (!textMatch(t.mint_address, f.mint_address)) return false;
  if (!textMatch(t.creator_address, f.creator)) return false;
  if (!textMatch(t.create_tx_address, f.create_tx)) return false;

  // Time
  if (!dateInRange(t.created_at, f.created_from, f.created_to)) return false;
  if (!dateInRange(t.last_trade_at, f.last_trade_from, f.last_trade_to)) return false;
  if (!dateInRange(t.ath_timestamp, f.ath_from, f.ath_to)) return false;

  // Lifetime (minutes): dead tokens only — still-alive/unknown tokens are exempt (kept).
  if (f.life_min || f.life_max) {
    const life = lifetimeMinutes(t);
    if (life != null && !rangeF64(life, f.life_min, f.life_max)) return false;
  }

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
  // max_cost_lamports / spendable_lamports_in are lamports; filter in SOL to match the table.
  if (!optF64(t.max_cost_lamports != null ? t.max_cost_lamports / 1e9 : null, f.max_cost_lamports_min, f.max_cost_lamports_max)) return false;
  if (!optF64(t.spendable_lamports_in != null ? t.spendable_lamports_in / 1e9 : null, f.spendable_lamports_in_min, f.spendable_lamports_in_max)) return false;
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
  if (!triMatch(t.is_dead, f.dead)) return false;
  if (!triMatch(t.is_mayhem_mode, f.mayhem)) return false;
  if (!triMatch(t.is_cashback_enabled, f.cashback)) return false;

  return true;
}
