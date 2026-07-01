import type { ColumnDef } from 'components/table/types';
import type { TokenRecord } from 'types';
import { DateCell } from 'components/table/DateCell';
import { RelativeTimeCell } from 'components/table/RelativeTimeCell';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { AmountCell, CompactCell, CurrentPriceCell, PriceCell } from 'components/tokens/priceCells';
import { formatCompact, formatDecimalTrim, formatWithCommas } from 'utils/format';

// ---------------------------------------------------------------------------
// Token enrichment keys that every strategy result row carries after merging.
// Mirrors the `TokenRecord` fields the batch endpoint provides (excluding
// `mint_address` — strategy rows use `mint` — and `active_lifetime_secs` — not
// persisted, excluded per plan).
// ---------------------------------------------------------------------------

const TOKEN_ENRICH_FIELDS = [
  'symbol',
  'name',
  'created_at',
  'creator_address',
  'initial_buy_sol',
  'initial_supply_token',
  'token_amount',
  'max_sol_cost',
  'spendable_sol_in',
  'min_tokens_out',
  'cu_limit',
  'cu_price',
  'is_mayhem_mode',
  'is_cashback_enabled',
  'create_tx_address',
  'ix_labels_count',
  'instruction_labels',
  'trade_count',
  'current_price',
  'volume_sol_total',
  'market_cap',
  'ath_price',
  'ath_timestamp',
  'is_migrated',
  'is_dead',
  'last_trade_at',
  'last_synced_at',
] as const;

type EnrichField = (typeof TOKEN_ENRICH_FIELDS)[number];

/** Merge token-info fields from `tokenMap` into each row, preserving the
 * row's own fields (they win on overlap). Used by strategy pages before passing
 * rows to DataTable. */
export function mergeTokenData<T extends { mint: string }>(
  rows: T[],
  tokenMap: Map<string, TokenRecord>,
): T[] {
  return rows.map((r) => {
    const tok = tokenMap.get(r.mint);
    if (!tok) return r;
    const patch: Partial<Record<EnrichField, unknown>> = {};
    for (const f of TOKEN_ENRICH_FIELDS) {
      patch[f] = (tok as unknown as Record<string, unknown>)[f];
    }
    return { ...patch, ...r } as T;
  });
}

// ---------------------------------------------------------------------------
// Shared column definitions — same set as tokenColumns.tsx for the token-info
// group, all hidden by default. `appendedTokenColumns` filters out any key the
// caller's existing column set already covers.
// ---------------------------------------------------------------------------

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const ALL_TOKEN_COLS: ColumnDef<any>[] = [
  // identity
  {
    key: 'creator',
    label: 'Creator',
    group: 'identity',
    defaultVisible: false,
    render: (r: { creator_address?: string }) =>
      r.creator_address ? (
        <AddressDisplay address={r.creator_address} kind="account" stopPropagation />
      ) : (
        '—'
      ),
    searchValue: (r: { creator_address?: string }) => r.creator_address ?? '',
  },
  {
    key: 'create_tx',
    label: 'Create TX',
    group: 'identity',
    defaultVisible: false,
    render: (r: { create_tx_address?: string }) =>
      r.create_tx_address ? (
        <AddressDisplay address={r.create_tx_address} kind="transaction" stopPropagation />
      ) : (
        '—'
      ),
    searchValue: (r: { create_tx_address?: string }) => r.create_tx_address ?? '',
  },
  // activity
  {
    key: 'trade_count',
    label: 'Token Trades',
    tooltip: 'Total trades ever recorded for this token (all time).',
    group: 'activity',
    defaultVisible: false,
    sortable: true,
    render: (r: { trade_count?: number }) => r.trade_count ?? '—',
    sortValue: (r: { trade_count?: number }) => r.trade_count ?? null,
    searchValue: (r: { trade_count?: number }) => String(r.trade_count ?? ''),
  },
  {
    key: 'last_trade',
    label: 'Last Trade',
    group: 'activity',
    defaultVisible: false,
    sortable: true,
    render: (r: { last_trade_at?: string | null }) => <DateCell iso={r.last_trade_at ?? null} />,
    sortValue: (r: { last_trade_at?: string | null }) => r.last_trade_at ?? null,
    searchValue: (r: { last_trade_at?: string | null }) => r.last_trade_at ?? '',
  },
  {
    key: 'last_synced',
    label: 'Last Synced',
    group: 'activity',
    defaultVisible: false,
    sortable: true,
    render: (r: { last_synced_at?: string | null }) => <RelativeTimeCell iso={r.last_synced_at ?? null} />,
    sortValue: (r: { last_synced_at?: string | null }) => r.last_synced_at ?? null,
    searchValue: (r: { last_synced_at?: string | null }) => r.last_synced_at ?? '',
  },
  // price
  {
    key: 'current_price',
    label: 'Price',
    group: 'price',
    defaultVisible: false,
    sortable: true,
    render: (r: { current_price?: number | null }) => <CurrentPriceCell sol={r.current_price ?? null} />,
    sortValue: (r: { current_price?: number | null }) => r.current_price ?? null,
    searchValue: (r: { current_price?: number | null }) => String(r.current_price ?? ''),
  },
  {
    key: 'ath_price',
    label: 'ATH',
    group: 'price',
    defaultVisible: false,
    sortable: true,
    render: (r: { ath_price?: number | null }) => <PriceCell sol={r.ath_price ?? null} />,
    sortValue: (r: { ath_price?: number | null }) => r.ath_price ?? null,
    searchValue: (r: { ath_price?: number | null }) => String(r.ath_price ?? ''),
  },
  {
    key: 'ath_timestamp',
    label: 'ATH At',
    group: 'price',
    defaultVisible: false,
    sortable: true,
    render: (r: { ath_timestamp?: string | null }) => <DateCell iso={r.ath_timestamp ?? null} />,
    sortValue: (r: { ath_timestamp?: string | null }) => r.ath_timestamp ?? null,
    searchValue: (r: { ath_timestamp?: string | null }) => r.ath_timestamp ?? '',
  },
  // market
  {
    key: 'market_cap',
    label: 'MCap',
    group: 'market',
    defaultVisible: false,
    sortable: true,
    render: (r: { market_cap?: number | null }) => <CompactCell sol={r.market_cap ?? null} digits={3} />,
    sortValue: (r: { market_cap?: number | null }) => r.market_cap ?? null,
    searchValue: (r: { market_cap?: number | null }) => String(r.market_cap ?? ''),
  },
  {
    key: 'volume',
    label: 'Volume',
    group: 'market',
    defaultVisible: false,
    sortable: true,
    render: (r: { volume_sol_total?: number }) => <CompactCell sol={r.volume_sol_total ?? null} digits={4} />,
    sortValue: (r: { volume_sol_total?: number }) => r.volume_sol_total ?? null,
    searchValue: (r: { volume_sol_total?: number }) => String(r.volume_sol_total ?? ''),
  },
  // initial
  {
    key: 'initial_buy',
    label: 'Init Buy',
    group: 'initial',
    defaultVisible: false,
    sortable: true,
    render: (r: { initial_buy_sol?: number | null }) => <AmountCell sol={r.initial_buy_sol ?? null} />,
    sortValue: (r: { initial_buy_sol?: number | null }) => r.initial_buy_sol ?? null,
    searchValue: (r: { initial_buy_sol?: number | null }) => String(r.initial_buy_sol ?? ''),
  },
  {
    key: 'init_supply',
    label: 'Init Supply',
    group: 'initial',
    defaultVisible: false,
    sortable: true,
    render: (r: { initial_supply_token?: number | null }) =>
      r.initial_supply_token != null ? formatCompact(r.initial_supply_token, 2) : '—',
    sortValue: (r: { initial_supply_token?: number | null }) => r.initial_supply_token ?? null,
    searchValue: (r: { initial_supply_token?: number | null }) => String(r.initial_supply_token ?? ''),
  },
  // max_or_spendable
  {
    key: 'token_amount',
    label: 'Token Amt',
    group: 'max_or_spendable',
    defaultVisible: false,
    sortable: true,
    render: (r: { token_amount?: number | null }) =>
      r.token_amount != null ? formatCompact(r.token_amount, 2) : '—',
    sortValue: (r: { token_amount?: number | null }) => r.token_amount ?? null,
    searchValue: (r: { token_amount?: number | null }) => String(r.token_amount ?? ''),
  },
  {
    key: 'max_sol_cost',
    label: 'Max SOL Cost',
    group: 'max_or_spendable',
    defaultVisible: false,
    sortable: true,
    render: (r: { max_sol_cost?: number | null }) =>
      r.max_sol_cost != null ? formatDecimalTrim(r.max_sol_cost / 1e9, 3) : '—',
    sortValue: (r: { max_sol_cost?: number | null }) => r.max_sol_cost ?? null,
    searchValue: (r: { max_sol_cost?: number | null }) => String(r.max_sol_cost ?? ''),
  },
  {
    key: 'spendable_sol_in',
    label: 'Spendable SOL In',
    group: 'max_or_spendable',
    defaultVisible: false,
    sortable: true,
    render: (r: { spendable_sol_in?: number | null }) =>
      r.spendable_sol_in != null ? formatDecimalTrim(r.spendable_sol_in / 1e9, 3) : '—',
    sortValue: (r: { spendable_sol_in?: number | null }) => r.spendable_sol_in ?? null,
    searchValue: (r: { spendable_sol_in?: number | null }) => String(r.spendable_sol_in ?? ''),
  },
  {
    key: 'min_tokens_out',
    label: 'Min Tokens',
    group: 'max_or_spendable',
    defaultVisible: false,
    sortable: true,
    render: (r: { min_tokens_out?: number | null }) =>
      r.min_tokens_out != null ? formatCompact(r.min_tokens_out, 2) : '—',
    sortValue: (r: { min_tokens_out?: number | null }) => r.min_tokens_out ?? null,
    searchValue: (r: { min_tokens_out?: number | null }) => String(r.min_tokens_out ?? ''),
  },
  // compute
  {
    key: 'cu_limit',
    label: 'CU Limit',
    group: 'compute',
    defaultVisible: false,
    sortable: true,
    render: (r: { cu_limit?: number | null }) => (r.cu_limit != null ? r.cu_limit : '—'),
    sortValue: (r: { cu_limit?: number | null }) => r.cu_limit ?? null,
    searchValue: (r: { cu_limit?: number | null }) => String(r.cu_limit ?? ''),
  },
  {
    key: 'cu_price',
    label: 'CU Price',
    group: 'compute',
    defaultVisible: false,
    sortable: true,
    render: (r: { cu_price?: number | null }) =>
      r.cu_price != null ? formatWithCommas(r.cu_price) : '—',
    sortValue: (r: { cu_price?: number | null }) => r.cu_price ?? null,
    searchValue: (r: { cu_price?: number | null }) => String(r.cu_price ?? ''),
  },
  // ix
  {
    key: 'ix_count',
    label: 'IX Count',
    group: 'ix',
    defaultVisible: false,
    sortable: true,
    render: (r: { ix_labels_count?: number }) => r.ix_labels_count ?? '—',
    sortValue: (r: { ix_labels_count?: number }) => r.ix_labels_count ?? null,
    searchValue: (r: { ix_labels_count?: number }) => String(r.ix_labels_count ?? ''),
  },
  {
    key: 'ix_labels',
    label: 'IX Labels',
    group: 'ix',
    defaultVisible: false,
    render: (r: { instruction_labels?: unknown }) => {
      const raw = r.instruction_labels;
      const arr: string[] = Array.isArray(raw)
        ? (raw as unknown[]).map(String)
        : (raw as { instructions?: unknown[] } | null)?.instructions?.map(String) ?? [];
      const s = arr.length ? arr.join(', ') : '-';
      return (
        <span title={s} className="block max-w-[180px] truncate text-[11px] text-text-dim">
          {s}
        </span>
      );
    },
    searchValue: (r: { instruction_labels?: unknown }) => {
      const raw = r.instruction_labels;
      const arr: string[] = Array.isArray(raw)
        ? (raw as unknown[]).map(String)
        : (raw as { instructions?: unknown[] } | null)?.instructions?.map(String) ?? [];
      return arr.join(', ');
    },
  },
  // flags
  {
    key: 'migrated',
    label: 'Migrated',
    group: 'flags',
    defaultVisible: false,
    sortable: true,
    render: (r: { is_migrated?: boolean }) => (r.is_migrated ? '✓' : ''),
    sortValue: (r: { is_migrated?: boolean }) => (r.is_migrated ? 1 : 0),
    searchValue: (r: { is_migrated?: boolean }) => String(r.is_migrated ?? false),
  },
  {
    key: 'dead',
    label: 'Dead',
    group: 'flags',
    defaultVisible: false,
    sortable: true,
    render: (r: { is_dead?: boolean }) => (r.is_dead ? '💀' : ''),
    sortValue: (r: { is_dead?: boolean }) => (r.is_dead ? 1 : 0),
    searchValue: (r: { is_dead?: boolean }) => String(r.is_dead ?? false),
  },
  {
    key: 'mayhem_mode',
    label: 'Mayhem',
    group: 'flags',
    defaultVisible: false,
    sortable: true,
    render: (r: { is_mayhem_mode?: boolean }) => (r.is_mayhem_mode ? '✓' : ''),
    sortValue: (r: { is_mayhem_mode?: boolean }) => (r.is_mayhem_mode ? 1 : 0),
    searchValue: (r: { is_mayhem_mode?: boolean }) => String(r.is_mayhem_mode ?? false),
  },
  {
    key: 'cashback',
    label: 'Cashback',
    group: 'flags',
    defaultVisible: false,
    sortable: true,
    render: (r: { is_cashback_enabled?: boolean }) => (r.is_cashback_enabled ? '✓' : ''),
    sortValue: (r: { is_cashback_enabled?: boolean }) => (r.is_cashback_enabled ? 1 : 0),
    searchValue: (r: { is_cashback_enabled?: boolean }) => String(r.is_cashback_enabled ?? false),
  },
];

/**
 * Core token-identity columns (symbol, name, created) for tables that don't
 * carry these fields on their result record (e.g. positions). All visible by
 * default. Pass `existingKeys` to skip any already present in the table.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function coreTokenColumns(existingKeys?: Set<string>): ColumnDef<any>[] {
  const cols: ColumnDef<any>[] = [
    {
      key: 'symbol',
      label: 'Symbol',
      group: 'identity',
      sortable: true,
      render: (r: { mint: string; symbol?: string; name?: string }) => (
        <AddressDisplay address={r.mint} kind="token" display={r.symbol ?? r.mint.slice(0, 6)} />
      ),
      sortValue: (r: { symbol?: string }) => r.symbol ?? '',
      searchValue: (r: { symbol?: string; name?: string }) => `${r.symbol ?? ''} ${r.name ?? ''}`,
    },
    {
      key: 'name',
      label: 'Name',
      group: 'identity',
      sortable: true,
      render: (r: { name?: string }) => (
        <span className="text-text-dim">{r.name ?? '—'}</span>
      ),
      sortValue: (r: { name?: string }) => r.name ?? '',
      searchValue: (r: { name?: string }) => r.name ?? '',
    },
    {
      key: 'created',
      label: 'Created',
      group: 'identity',
      sortable: true,
      render: (r: { created_at?: string | null }) => <DateCell iso={r.created_at ?? null} />,
      sortValue: (r: { created_at?: string | null }) => r.created_at ?? '',
      searchValue: (r: { created_at?: string | null }) => r.created_at ?? '',
    },
  ];
  return existingKeys ? cols.filter((c) => !existingKeys.has(c.key)) : cols;
}

/**
 * Return token-info columns to append to a strategy result table. All columns
 * have `defaultVisible: false` so they don't show by default. Columns whose key
 * is already in `existingKeys` are filtered out to avoid duplicates.
 *
 * Callers should pass a Set that includes both the exact column keys already in
 * the table AND semantic aliases for the same data (e.g. if the table already
 * shows `initial_buy_sol` under key `init_buy`, add `'initial_buy'` too).
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function appendedTokenColumns(existingKeys: Set<string>): ColumnDef<any>[] {
  return ALL_TOKEN_COLS.filter((c) => !existingKeys.has(c.key));
}
