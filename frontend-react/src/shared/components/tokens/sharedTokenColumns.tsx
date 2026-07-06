import { useMemo, useState, type MouseEvent } from 'react';
import type { ColumnDef } from 'components/table/types';
import { DateCell } from 'components/table/DateCell';
import { RelativeTimeCell } from 'components/table/RelativeTimeCell';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { AmountCell, CompactCell, CurrentPriceCell, PriceCell } from 'components/tokens/priceCells';
import { numericColKeys } from 'services/tableRequest';
import { formatCompact, formatDecimalTrim, formatWithCommas } from 'utils/format';
import { cn } from 'lib/cn';

// ---------------------------------------------------------------------------
// Instruction-labels helpers + the copy-on-click IX-count cell. Shared by the
// Tokens page (`tokenColumns`) and every strategy table (via the enrichment
// columns below) so the label/JSON rendering lives in exactly one place.
// ---------------------------------------------------------------------------

function ixLabelsArray(r: { instruction_labels?: unknown }): string[] {
  const raw = r.instruction_labels;
  if (Array.isArray(raw)) return raw.map(String);
  const obj = raw as { instructions?: unknown[] } | null;
  if (obj?.instructions) return obj.instructions.map(String);
  return [];
}

export function ixLabelsText(r: { instruction_labels?: unknown }): string {
  const arr = ixLabelsArray(r);
  return arr.length ? arr.join(', ') : '-';
}

function ixLabelsJson(r: { instruction_labels?: unknown }): string {
  return JSON.stringify(ixLabelsArray(r), null, 2);
}

function IxCountCell({ row }: { row: { ix_labels_count?: number; instruction_labels?: unknown } }) {
  const [copied, setCopied] = useState(false);
  // Only re-stringify when the labels actually change, not on every poll-driven
  // render of the row.
  const json = useMemo(() => ixLabelsJson(row), [row]);

  const copy = async (e: MouseEvent<HTMLSpanElement>) => {
    e.stopPropagation();
    try {
      await navigator.clipboard.writeText(json);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* ignore */
    }
  };

  return (
    <span
      onClick={copy}
      title={copied ? 'Copied!' : json}
      className={cn('cursor-pointer', copied && 'text-primary')}
    >
      {row.ix_labels_count ?? '—'}
    </span>
  );
}

// ---------------------------------------------------------------------------
// Shared token-info column SSOT.
//
// The ~26 enrichment columns are defined here ONCE and consumed by:
//   • the Tokens page (`tokenColumns.tsx`) — via `tokenInfoColumnMap`, placed in
//     its bespoke order alongside identity + Tokens-only columns; and
//   • every strategy result table (positions/matched/simulated/wallet) — via
//     `appendedTokenColumns`, which overlays each column's default show/hide.
//
// The columns carry the render/sort/search/filter *logic* (the facts that must
// not drift). Per-view presentation — column width, order, and `defaultVisible`
// — is applied by the consumer, since those legitimately differ (the Tokens page
// shows every column and owns its widths; strategy tables default most hidden).
// ---------------------------------------------------------------------------

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function tokenInfoColumns(): ColumnDef<any>[] {
  return [
    // identity
    {
      key: 'creator',
      label: 'Creator',
      group: 'identity',
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
      render: (r: { creation_tx_signature?: string }) =>
        r.creation_tx_signature ? (
          <AddressDisplay address={r.creation_tx_signature} kind="transaction" stopPropagation />
        ) : (
          '—'
        ),
      searchValue: (r: { creation_tx_signature?: string }) => r.creation_tx_signature ?? '',
    },
    // activity
    {
      key: 'trade_count',
      label: 'Token Trades',
      tooltip: 'Total trades ever recorded for this token (all time).',
      group: 'activity',
      sortable: true,
      render: (r: { trade_count?: number }) => r.trade_count ?? '—',
      sortValue: (r: { trade_count?: number }) => r.trade_count ?? null,
      searchValue: (r: { trade_count?: number }) => String(r.trade_count ?? ''),
      filterNumber: (r: { trade_count?: number }) => r.trade_count ?? null,
    },
    {
      key: 'last_trade',
      label: 'Last Trade',
      group: 'activity',
      sortable: true,
      render: (r: { last_trade_at?: string | null }) => <DateCell iso={r.last_trade_at ?? null} />,
      sortValue: (r: { last_trade_at?: string | null }) => r.last_trade_at ?? null,
      searchValue: (r: { last_trade_at?: string | null }) => r.last_trade_at ?? '',
    },
    {
      key: 'last_synced',
      label: 'Last Synced',
      group: 'activity',
      sortable: true,
      render: (r: { last_synced_at?: string | null }) => (
        <RelativeTimeCell iso={r.last_synced_at ?? null} />
      ),
      sortValue: (r: { last_synced_at?: string | null }) => r.last_synced_at ?? null,
      searchValue: (r: { last_synced_at?: string | null }) => r.last_synced_at ?? '',
    },
    // price
    {
      key: 'current_price',
      label: 'Price',
      group: 'price',
      sortable: true,
      render: (r: { current_price?: number | null }) => (
        <CurrentPriceCell sol={r.current_price ?? null} />
      ),
      sortValue: (r: { current_price?: number | null }) => r.current_price ?? null,
      searchValue: (r: { current_price?: number | null }) => String(r.current_price ?? ''),
    },
    {
      key: 'ath_price',
      label: 'ATH',
      group: 'price',
      sortable: true,
      render: (r: { ath_price?: number | null }) => <PriceCell sol={r.ath_price ?? null} />,
      sortValue: (r: { ath_price?: number | null }) => r.ath_price ?? null,
      searchValue: (r: { ath_price?: number | null }) => String(r.ath_price ?? ''),
    },
    {
      key: 'ath_timestamp',
      label: 'ATH At',
      group: 'price',
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
      sortable: true,
      render: (r: { market_cap?: number | null }) => (
        <CompactCell sol={r.market_cap ?? null} digits={3} />
      ),
      sortValue: (r: { market_cap?: number | null }) => r.market_cap ?? null,
      searchValue: (r: { market_cap?: number | null }) => String(r.market_cap ?? ''),
      filterNumber: (r: { market_cap?: number | null }) => r.market_cap ?? null,
    },
    {
      key: 'volume',
      label: 'Volume',
      group: 'market',
      sortable: true,
      render: (r: { volume_sol_total?: number }) => (
        <CompactCell sol={r.volume_sol_total ?? null} digits={4} />
      ),
      sortValue: (r: { volume_sol_total?: number }) => r.volume_sol_total ?? null,
      searchValue: (r: { volume_sol_total?: number }) => String(r.volume_sol_total ?? ''),
      filterNumber: (r: { volume_sol_total?: number }) => r.volume_sol_total ?? null,
    },
    {
      key: 'first_slot_buy',
      label: '1st Slot Buy',
      group: 'market',
      sortable: true,
      render: (r: { first_slot_buy_sol?: number | null }) => (
        <AmountCell sol={r.first_slot_buy_sol ?? null} />
      ),
      sortValue: (r: { first_slot_buy_sol?: number | null }) => r.first_slot_buy_sol ?? null,
      searchValue: (r: { first_slot_buy_sol?: number | null }) => String(r.first_slot_buy_sol ?? ''),
      filterNumber: (r: { first_slot_buy_sol?: number | null }) => r.first_slot_buy_sol ?? null,
    },
    {
      key: 'first_slot_sell',
      label: '1st Slot Sell',
      group: 'market',
      sortable: true,
      render: (r: { first_slot_sell_sol?: number | null }) => (
        <AmountCell sol={r.first_slot_sell_sol ?? null} />
      ),
      sortValue: (r: { first_slot_sell_sol?: number | null }) => r.first_slot_sell_sol ?? null,
      searchValue: (r: { first_slot_sell_sol?: number | null }) => String(r.first_slot_sell_sol ?? ''),
      filterNumber: (r: { first_slot_sell_sol?: number | null }) => r.first_slot_sell_sol ?? null,
    },
    // initial
    {
      key: 'initial_buy',
      label: 'Init Buy',
      group: 'initial',
      sortable: true,
      render: (r: { initial_buy_sol?: number | null }) => <AmountCell sol={r.initial_buy_sol ?? null} />,
      sortValue: (r: { initial_buy_sol?: number | null }) => r.initial_buy_sol ?? null,
      searchValue: (r: { initial_buy_sol?: number | null }) => String(r.initial_buy_sol ?? ''),
      filterNumber: (r: { initial_buy_sol?: number | null }) => r.initial_buy_sol ?? null,
    },
    {
      key: 'init_supply',
      label: 'Init Supply',
      group: 'initial',
      sortable: true,
      render: (r: { initial_supply_token?: number | null }) =>
        r.initial_supply_token != null ? formatCompact(r.initial_supply_token, 2) : '—',
      sortValue: (r: { initial_supply_token?: number | null }) => r.initial_supply_token ?? null,
      searchValue: (r: { initial_supply_token?: number | null }) => String(r.initial_supply_token ?? ''),
      filterNumber: (r: { initial_supply_token?: number | null }) => r.initial_supply_token ?? null,
    },
    // max_or_spendable
    {
      key: 'token_amount',
      label: 'Token Amt',
      group: 'max_or_spendable',
      sortable: true,
      render: (r: { token_amount?: number | null }) =>
        r.token_amount != null ? formatCompact(r.token_amount, 2) : '—',
      sortValue: (r: { token_amount?: number | null }) => r.token_amount ?? null,
      searchValue: () => '',
      filterValue: (r: { token_amount?: number | null }) =>
        r.token_amount != null ? formatCompact(r.token_amount, 2) : '',
      filterNumber: (r: { token_amount?: number | null }) => r.token_amount ?? null,
    },
    {
      key: 'max_cost_lamports',
      label: 'Max SOL Cost',
      group: 'max_or_spendable',
      sortable: true,
      render: (r: { max_cost_lamports?: number | null }) =>
        r.max_cost_lamports != null ? formatDecimalTrim(r.max_cost_lamports / 1e9, 3) : '—',
      sortValue: (r: { max_cost_lamports?: number | null }) => r.max_cost_lamports ?? null,
      searchValue: () => '',
      filterValue: (r: { max_cost_lamports?: number | null }) =>
        r.max_cost_lamports != null ? formatDecimalTrim(r.max_cost_lamports / 1e9, 3) : '',
      // Numeric filter compares in the *displayed* unit (SOL), so divide lamports by 1e9.
      filterNumber: (r: { max_cost_lamports?: number | null }) =>
        r.max_cost_lamports != null ? r.max_cost_lamports / 1e9 : null,
    },
    {
      key: 'spendable_lamports_in',
      label: 'Spendable SOL In',
      group: 'max_or_spendable',
      sortable: true,
      render: (r: { spendable_lamports_in?: number | null }) =>
        r.spendable_lamports_in != null ? formatDecimalTrim(r.spendable_lamports_in / 1e9, 3) : '—',
      sortValue: (r: { spendable_lamports_in?: number | null }) => r.spendable_lamports_in ?? null,
      searchValue: () => '',
      filterValue: (r: { spendable_lamports_in?: number | null }) =>
        r.spendable_lamports_in != null ? formatDecimalTrim(r.spendable_lamports_in / 1e9, 3) : '',
      // Numeric filter compares in the *displayed* unit (SOL), so divide lamports by 1e9.
      filterNumber: (r: { spendable_lamports_in?: number | null }) =>
        r.spendable_lamports_in != null ? r.spendable_lamports_in / 1e9 : null,
    },
    {
      key: 'min_tokens_out',
      label: 'Min Tokens',
      group: 'max_or_spendable',
      sortable: true,
      render: (r: { min_tokens_out?: number | null }) =>
        r.min_tokens_out != null ? formatCompact(r.min_tokens_out, 2) : '—',
      sortValue: (r: { min_tokens_out?: number | null }) => r.min_tokens_out ?? null,
      searchValue: () => '',
      filterValue: (r: { min_tokens_out?: number | null }) =>
        r.min_tokens_out != null ? formatCompact(r.min_tokens_out, 2) : '',
      filterNumber: (r: { min_tokens_out?: number | null }) => r.min_tokens_out ?? null,
    },
    // compute
    {
      key: 'cu_limit',
      label: 'CU Limit',
      group: 'compute',
      sortable: true,
      render: (r: { cu_limit?: number | null }) => (r.cu_limit != null ? r.cu_limit : '—'),
      sortValue: (r: { cu_limit?: number | null }) => r.cu_limit ?? null,
      searchValue: (r: { cu_limit?: number | null }) => String(r.cu_limit ?? ''),
      filterNumber: (r: { cu_limit?: number | null }) => r.cu_limit ?? null,
    },
    {
      key: 'cu_price',
      label: 'CU Price',
      group: 'compute',
      sortable: true,
      render: (r: { cu_price?: number | null }) =>
        r.cu_price != null ? formatWithCommas(r.cu_price) : '—',
      sortValue: (r: { cu_price?: number | null }) => r.cu_price ?? null,
      searchValue: (r: { cu_price?: number | null }) => String(r.cu_price ?? ''),
      filterNumber: (r: { cu_price?: number | null }) => r.cu_price ?? null,
    },
    // ix
    {
      key: 'ix_count',
      label: 'IX Count',
      group: 'ix',
      sortable: true,
      render: (r: { ix_labels_count?: number; instruction_labels?: unknown }) => <IxCountCell row={r} />,
      sortValue: (r: { ix_labels_count?: number }) => r.ix_labels_count ?? null,
      searchValue: (r: { ix_labels_count?: number }) => String(r.ix_labels_count ?? ''),
      filterNumber: (r: { ix_labels_count?: number }) => r.ix_labels_count ?? null,
    },
    {
      key: 'ix_labels',
      label: 'IX Labels',
      group: 'ix',
      render: (r: { instruction_labels?: unknown }) => {
        const s = ixLabelsText(r);
        return (
          <span title={s} className="block max-w-[180px] truncate text-[11px] text-text-dim">
            {s}
          </span>
        );
      },
      searchValue: (r: { instruction_labels?: unknown }) => ixLabelsText(r),
    },
    // flags
    {
      key: 'migrated',
      label: 'Migrated',
      group: 'flags',
      sortable: true,
      render: (r: { is_migrated?: boolean }) => (r.is_migrated ? '✓' : ''),
      sortValue: (r: { is_migrated?: boolean }) => (r.is_migrated ? 1 : 0),
      searchValue: (r: { is_migrated?: boolean }) => String(r.is_migrated ?? false),
    },
    {
      key: 'dead',
      label: 'Dead',
      group: 'flags',
      sortable: true,
      render: (r: { is_dead?: boolean }) => (r.is_dead ? '💀' : ''),
      sortValue: (r: { is_dead?: boolean }) => (r.is_dead ? 1 : 0),
      searchValue: (r: { is_dead?: boolean }) => String(r.is_dead ?? false),
    },
    {
      key: 'mayhem_mode',
      label: 'Mayhem',
      group: 'flags',
      sortable: true,
      render: (r: { is_mayhem_mode?: boolean }) => (r.is_mayhem_mode ? '✓' : ''),
      sortValue: (r: { is_mayhem_mode?: boolean }) => (r.is_mayhem_mode ? 1 : 0),
      searchValue: (r: { is_mayhem_mode?: boolean }) => String(r.is_mayhem_mode ?? false),
    },
    {
      key: 'cashback',
      label: 'Cashback',
      group: 'flags',
      sortable: true,
      render: (r: { is_cashback_enabled?: boolean }) => (r.is_cashback_enabled ? '✓' : ''),
      sortValue: (r: { is_cashback_enabled?: boolean }) => (r.is_cashback_enabled ? 1 : 0),
      searchValue: (r: { is_cashback_enabled?: boolean }) => String(r.is_cashback_enabled ?? false),
    },
  ];
}

/**
 * Enrichment columns that default to HIDDEN in strategy result tables (the
 * appended set). Everything else in {@link tokenInfoColumns} defaults visible.
 * The Tokens page ignores this — it shows every column by default.
 */
const APPENDED_HIDDEN_KEYS = new Set([
  'creator',
  'create_tx',
  'last_synced',
  'ath_timestamp',
  'market_cap',
  'volume',
  'first_slot_buy',
  'first_slot_sell',
  'init_supply',
  'token_amount',
  'min_tokens_out',
  'ix_labels',
]);

/**
 * The shared token-info columns keyed by column key, so the Tokens page can
 * place each one in its own bespoke order alongside its identity/extra columns.
 * This IS the single definition — the Tokens page owns only order + width.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function tokenInfoColumnMap(): Record<string, ColumnDef<any>> {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const map: Record<string, ColumnDef<any>> = {};
  for (const c of tokenInfoColumns()) map[c.key] = c;
  return map;
}

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
      render: (r: { mint_address: string; symbol?: string; name?: string }) => (
        <AddressDisplay address={r.mint_address} kind="token" display={r.symbol ?? r.mint_address.slice(0, 6)} />
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
 * Return token-info columns to append to a strategy result table. Each column's
 * `defaultVisible` flag (overlaid from {@link APPENDED_HIDDEN_KEYS}) controls its
 * initial show/hide state. Columns whose key is already in `existingKeys` are
 * filtered out to avoid duplicates.
 *
 * Callers should pass a Set that includes both the exact column keys already in
 * the table AND semantic aliases for the same data (e.g. if the table already
 * shows `initial_buy_sol` under key `init_buy`, add `'initial_buy'` too).
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function appendedTokenColumns(existingKeys: Set<string>): ColumnDef<any>[] {
  return tokenInfoColumns()
    .filter((c) => !existingKeys.has(c.key))
    .map((c) => ({ ...c, defaultVisible: !APPENDED_HIDDEN_KEYS.has(c.key) }));
}

/**
 * Every column key the shared token-info set contributes. Pass this as a token
 * table's `existingKeys` to append NOTHING (the Tokens page lays out the full
 * field set itself, in its own order) while still routing through `TokenTable`
 * for the shared features.
 */
export const ALL_TOKEN_INFO_KEYS: Set<string> = new Set(
  tokenInfoColumns().map((c) => c.key),
);

/**
 * The subset of token-info column keys that filter numerically (declare
 * `filterNumber`). These columns are appended by `TokenTable`, so a table's own
 * `numericColKeys(baseColumns)` misses them — union this in when serializing the
 * view-state, or use {@link tokenNumericColKeys}.
 */
export const TOKEN_INFO_NUMERIC_KEYS: ReadonlySet<string> = numericColKeys(
  tokenInfoColumns(),
);

/**
 * Numeric-filtering keys for a token table: the base columns' numeric keys PLUS
 * the appended token-info numeric keys. Use this (not `numericColKeys(base)`) for
 * the `numericCols` a token table serializes with, so numeric filters on the
 * appended enrichment columns (`>5`/`1..10`) still lower to structured ops.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function tokenNumericColKeys(baseColumns: ColumnDef<any>[]): Set<string> {
  return new Set([...numericColKeys(baseColumns), ...TOKEN_INFO_NUMERIC_KEYS]);
}
