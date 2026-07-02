import { useMemo, useState, type MouseEvent } from 'react';
import type { ColumnDef } from 'components/table/types';
import type { TokenRecord } from 'types';
import { AgeCell } from 'components/table/AgeCell';
import { DateCell } from 'components/table/DateCell';
import { RelativeTimeCell } from 'components/table/RelativeTimeCell';
import {
  ageClass,
  formatAge,
  formatCompact,
  formatDecimalTrim,
  formatWithCommas,
  ratioClass,
} from 'utils/format';
import { AmountCell, CompactCell, CurrentPriceCell, PriceCell } from './priceCells';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { cn } from 'lib/cn';

/**
 * Token creation as epoch-ms. Prefers the field the RTK transform pre-parsed
 * once (`created_at_ms`), falling back to parsing the ISO string for any row
 * that predates it. The age itself is rendered live by {@link AgeCell}, which
 * subscribes to the shared clock and ticks on its own between polls.
 */
function createdMsOf(r: TokenRecord): number {
  return r.created_at_ms ?? Date.parse(r.created_at);
}

/** Age in whole seconds for the sort/search fallbacks (dead in server mode). */
function ageSecondsOf(r: TokenRecord): number {
  return Math.max(0, Math.floor((Date.now() - createdMsOf(r)) / 1000));
}

function fep(r: TokenRecord): number | null {
  if (r.initial_buy_sol == null || r.initial_supply_token == null || r.initial_supply_token <= 0) {
    return null;
  }
  return r.initial_buy_sol / r.initial_supply_token;
}

function ixLabelsArray(r: TokenRecord): string[] {
  const raw = r.instruction_labels;
  if (Array.isArray(raw)) return raw.map(String);
  const obj = raw as { instructions?: unknown[] } | null;
  if (obj?.instructions) return obj.instructions.map(String);
  return [];
}

function ixLabels(r: TokenRecord): string {
  const arr = ixLabelsArray(r);
  return arr.length ? arr.join(', ') : '-';
}

function ixLabelsJson(r: TokenRecord): string {
  return JSON.stringify(ixLabelsArray(r), null, 2);
}

function IxCountCell({ row }: { row: TokenRecord }) {
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
      {row.ix_labels_count}
    </span>
  );
}

export function tokenColumns(): ColumnDef<TokenRecord>[] {
  return [
    {
      key: 'symbol',
      label: 'Symbol',
      group: 'identity',
      width: '90px',
      sortable: true,
      render: (r) => r.symbol,
      sortValue: (r) => r.symbol,
      searchValue: (r) => `${r.symbol} ${r.name} ${r.mint_address}`,
    },
    {
      key: 'name',
      label: 'Name',
      group: 'identity',
      width: '120px',
      sortable: true,
      render: (r) => r.name,
      sortValue: (r) => r.name,
      searchValue: (r) => r.name,
    },
    {
      key: 'mint',
      label: 'Mint',
      group: 'identity',
      width: '165px',
      sortable: true,
      render: (r) => (
        <AddressDisplay address={r.mint_address} kind="token" stopPropagation />
      ),
      sortValue: (r) => r.mint_address,
      searchValue: (r) => r.mint_address,
    },
    {
      key: 'creator',
      label: 'Creator',
      group: 'identity',
      width: '165px',
      sortable: true,
      render: (r) => (
        <AddressDisplay address={r.creator_address} kind="account" stopPropagation />
      ),
      sortValue: (r) => r.creator_address,
      searchValue: (r) => r.creator_address,
    },
    {
      key: 'create_tx',
      label: 'Create TX',
      group: 'identity',
      width: '165px',
      render: (r) => (
        <AddressDisplay address={r.create_tx_address} kind="transaction" stopPropagation />
      ),
      searchValue: (r) => r.create_tx_address,
    },
    {
      key: 'token_age',
      label: 'Token Age',
      group: 'activity',
      width: '72px',
      sortable: true,
      render: (r) => <AgeCell createdMs={createdMsOf(r)} />,
      sortValue: (r) => ageSecondsOf(r),
      searchValue: (r) => formatAge(ageSecondsOf(r)),
    },
    {
      key: 'created',
      label: 'Created',
      group: 'activity',
      width: '110px',
      sortable: true,
      render: (r) => <DateCell iso={r.created_at} />,
      sortValue: (r) => r.created_at,
      searchValue: (r) => r.created_at,
    },
    {
      key: 'last_trade',
      label: 'Last Trade',
      group: 'activity',
      width: '110px',
      sortable: true,
      render: (r) => <DateCell iso={r.last_trade_at} />,
      sortValue: (r) => r.last_trade_at,
      searchValue: (r) => r.last_trade_at ?? '',
    },
    {
      key: 'lifetime',
      label: 'Lifetime',
      group: 'activity',
      width: '72px',
      sortable: true,
      render: (r) =>
        r.active_lifetime_secs != null ? (
          <span className={ageClass(r.active_lifetime_secs)}>
            {formatAge(r.active_lifetime_secs)}
          </span>
        ) : (
          '-'
        ),
      sortValue: (r) => r.active_lifetime_secs,
      searchValue: (r) =>
        r.active_lifetime_secs != null ? formatAge(r.active_lifetime_secs) : '',
    },
    {
      key: 'last_synced',
      label: 'Last Synced',
      group: 'activity',
      width: '96px',
      sortable: true,
      render: (r) => <RelativeTimeCell iso={r.last_synced_at} />,
      sortValue: (r) => r.last_synced_at,
      searchValue: (r) => r.last_synced_at ?? '',
    },
    {
      key: 'trade_count',
      label: 'Trades',
      group: 'activity',
      width: '66px',
      sortable: true,
      render: (r) => r.trade_count,
      sortValue: (r) => r.trade_count,
      searchValue: (r) => String(r.trade_count),
      filterNumber: (r) => r.trade_count,
    },
    {
      key: 'ath_price',
      label: 'ATH',
      group: 'price',
      width: '88px',
      sortable: true,
      render: (r) => <PriceCell sol={r.ath_price} />,
      sortValue: (r) => r.ath_price,
      searchValue: (r) => String(r.ath_price ?? ''),
    },
    {
      key: 'ath_timestamp',
      label: 'ATH At',
      group: 'price',
      width: '110px',
      sortable: true,
      render: (r) => <DateCell iso={r.ath_timestamp} />,
      sortValue: (r) => r.ath_timestamp,
      searchValue: (r) => r.ath_timestamp ?? '',
    },
    {
      key: 'ath_fep_ratio',
      label: 'ATH/FEP',
      group: 'price',
      width: '88px',
      sortable: true,
      render: (r) => {
        const entry = fep(r);
        const ratio =
          entry && r.ath_price && entry !== 0 ? r.ath_price / entry : null;
        return ratio != null ? (
          <span className={ratioClass(ratio)}>{formatDecimalTrim(ratio, 2)}x</span>
        ) : (
          '-'
        );
      },
      sortValue: (r) => {
        const entry = fep(r);
        return entry && r.ath_price && entry !== 0 ? r.ath_price / entry : null;
      },
      searchValue: () => '',
      filterValue: (r) => {
        const entry = fep(r);
        const ratio = entry && r.ath_price && entry !== 0 ? r.ath_price / entry : null;
        return ratio != null ? `${formatDecimalTrim(ratio, 2)}x` : '';
      },
      filterNumber: (r) => {
        const entry = fep(r);
        return entry && r.ath_price && entry !== 0 ? r.ath_price / entry : null;
      },
    },
    {
      key: 'current_price',
      label: 'Price',
      group: 'price',
      width: '88px',
      sortable: true,
      render: (r) => <CurrentPriceCell sol={r.current_price} />,
      sortValue: (r) => r.current_price,
      searchValue: (r) => String(r.current_price ?? ''),
    },
    {
      key: 'current_fep_ratio',
      label: 'Cur/FEP',
      group: 'price',
      width: '76px',
      sortable: true,
      render: (r) => {
        const entry = fep(r);
        const ratio =
          entry && r.current_price && entry !== 0 ? r.current_price / entry : null;
        return ratio != null ? (
          <span className={ratioClass(ratio)}>{formatDecimalTrim(ratio, 2)}x</span>
        ) : (
          '-'
        );
      },
      sortValue: (r) => {
        const entry = fep(r);
        return entry && r.current_price && entry !== 0 ? r.current_price / entry : null;
      },
      searchValue: () => '',
      filterValue: (r) => {
        const entry = fep(r);
        const ratio = entry && r.current_price && entry !== 0 ? r.current_price / entry : null;
        return ratio != null ? `${formatDecimalTrim(ratio, 2)}x` : '';
      },
      filterNumber: (r) => {
        const entry = fep(r);
        return entry && r.current_price && entry !== 0 ? r.current_price / entry : null;
      },
    },
    {
      key: 'market_cap',
      label: 'MCap',
      group: 'market',
      width: '84px',
      sortable: true,
      render: (r) => <CompactCell sol={r.market_cap} digits={3} />,
      sortValue: (r) => r.market_cap,
      searchValue: (r) => String(r.market_cap ?? ''),
      filterNumber: (r) => r.market_cap,
    },
    {
      key: 'volume',
      label: 'Volume',
      group: 'market',
      width: '78px',
      sortable: true,
      render: (r) => <CompactCell sol={r.volume_sol_total} digits={4} />,
      sortValue: (r) => r.volume_sol_total,
      searchValue: (r) => String(r.volume_sol_total),
      filterNumber: (r) => r.volume_sol_total,
    },
    {
      key: 'first_slot_buy',
      label: '1st Slot Buy',
      group: 'market',
      width: '84px',
      sortable: true,
      render: (r) => <AmountCell sol={r.first_slot_buy_sol ?? null} />,
      sortValue: (r) => r.first_slot_buy_sol ?? null,
      searchValue: (r) => String(r.first_slot_buy_sol ?? ''),
      filterNumber: (r) => r.first_slot_buy_sol ?? null,
    },
    {
      key: 'first_slot_sell',
      label: '1st Slot Sell',
      group: 'market',
      width: '84px',
      sortable: true,
      render: (r) => <AmountCell sol={r.first_slot_sell_sol ?? null} />,
      sortValue: (r) => r.first_slot_sell_sol ?? null,
      searchValue: (r) => String(r.first_slot_sell_sol ?? ''),
      filterNumber: (r) => r.first_slot_sell_sol ?? null,
    },
    {
      key: 'initial_buy',
      label: 'Init Buy',
      group: 'initial',
      width: '78px',
      sortable: true,
      render: (r) => <AmountCell sol={r.initial_buy_sol} />,
      sortValue: (r) => r.initial_buy_sol,
      searchValue: (r) => String(r.initial_buy_sol ?? ''),
      filterNumber: (r) => r.initial_buy_sol,
    },
    {
      key: 'init_supply',
      label: 'Init Supply',
      group: 'initial',
      width: '90px',
      sortable: true,
      render: (r) =>
        r.initial_supply_token != null ? formatCompact(r.initial_supply_token, 2) : '-',
      sortValue: (r) => r.initial_supply_token,
      searchValue: (r) => String(r.initial_supply_token ?? ''),
      filterNumber: (r) => r.initial_supply_token,
    },
    {
      key: 'token_amount',
      label: 'Token Amt',
      group: 'max_or_spendable',
      width: '90px',
      sortable: true,
      render: (r) => (r.token_amount != null ? formatCompact(r.token_amount, 2) : '-'),
      sortValue: (r) => r.token_amount,
      searchValue: () => '',
      filterValue: (r) => (r.token_amount != null ? formatCompact(r.token_amount, 2) : ''),
      filterNumber: (r) => r.token_amount,
    },
    {
      key: 'max_cost_lamports',
      label: 'Max SOL Cost',
      group: 'max_or_spendable',
      width: '100px',
      sortable: true,
      render: (r) =>
        r.max_cost_lamports != null ? formatDecimalTrim(r.max_cost_lamports / 1e9, 3) : '-',
      sortValue: (r) => r.max_cost_lamports,
      searchValue: () => '',
      filterValue: (r) =>
        r.max_cost_lamports != null ? formatDecimalTrim(r.max_cost_lamports / 1e9, 3) : '',
      filterNumber: (r) => (r.max_cost_lamports != null ? r.max_cost_lamports / 1e9 : null),
    },
    {
      key: 'spendable_lamports_in',
      label: 'Spendable SOL In',
      group: 'max_or_spendable',
      width: '100px',
      sortable: true,
      render: (r) =>
        r.spendable_lamports_in != null ? formatDecimalTrim(r.spendable_lamports_in / 1e9, 3) : '-',
      sortValue: (r) => r.spendable_lamports_in,
      searchValue: () => '',
      filterValue: (r) =>
        r.spendable_lamports_in != null ? formatDecimalTrim(r.spendable_lamports_in / 1e9, 3) : '',
      filterNumber: (r) => (r.spendable_lamports_in != null ? r.spendable_lamports_in / 1e9 : null),
    },
    {
      key: 'min_tokens_out',
      label: 'Min Tokens',
      group: 'max_or_spendable',
      width: '90px',
      sortable: true,
      render: (r) => (r.min_tokens_out != null ? formatCompact(r.min_tokens_out, 2) : '-'),
      sortValue: (r) => r.min_tokens_out,
      searchValue: () => '',
      filterValue: (r) => (r.min_tokens_out != null ? formatCompact(r.min_tokens_out, 2) : ''),
      filterNumber: (r) => r.min_tokens_out,
    },
    {
      key: 'cu_limit',
      label: 'CU Limit',
      group: 'compute',
      width: '72px',
      sortable: true,
      render: (r) => (r.cu_limit != null ? r.cu_limit : '-'),
      sortValue: (r) => r.cu_limit,
      searchValue: (r) => String(r.cu_limit ?? ''),
      filterNumber: (r) => r.cu_limit,
    },
    {
      key: 'cu_price',
      label: 'CU Price',
      group: 'compute',
      width: '72px',
      sortable: true,
      render: (r) => (r.cu_price != null ? formatWithCommas(r.cu_price) : '-'),
      sortValue: (r) => r.cu_price,
      searchValue: (r) => String(r.cu_price ?? ''),
      filterNumber: (r) => r.cu_price,
    },
    {
      key: 'ix_count',
      label: 'IX Count',
      group: 'ix',
      width: '54px',
      sortable: true,
      render: (r) => <IxCountCell row={r} />,
      sortValue: (r) => r.ix_labels_count,
      searchValue: (r) => String(r.ix_labels_count),
      filterNumber: (r) => r.ix_labels_count,
    },
    {
      key: 'ix_labels',
      label: 'IX Labels',
      group: 'ix',
      width: '180px',
      sortable: false,
      render: (r) => {
        const s = ixLabels(r);
        return (
          <span title={s} className="block max-w-[180px] truncate text-[11px] text-text-dim">
            {s}
          </span>
        );
      },
      searchValue: (r) => ixLabels(r),
    },
    {
      key: 'migrated',
      label: 'Migrated',
      group: 'flags',
      width: '66px',
      sortable: true,
      render: (r) => (r.is_migrated ? '✓' : ''),
      sortValue: (r) => (r.is_migrated ? 1 : 0),
      searchValue: (r) => String(r.is_migrated),
    },
    {
      key: 'dead',
      label: 'Dead',
      group: 'flags',
      width: '66px',
      sortable: true,
      render: (r) => (r.is_dead ? '💀' : ''),
      sortValue: (r) => (r.is_dead ? 1 : 0),
      searchValue: (r) => String(r.is_dead),
    },
    {
      key: 'mayhem_mode',
      label: 'Mayhem',
      group: 'flags',
      width: '66px',
      sortable: true,
      render: (r) => (r.is_mayhem_mode ? '✓' : ''),
      sortValue: (r) => (r.is_mayhem_mode ? 1 : 0),
      searchValue: (r) => String(r.is_mayhem_mode),
    },
    {
      key: 'cashback',
      label: 'Cashback',
      group: 'flags',
      width: '72px',
      sortable: true,
      render: (r) => (r.is_cashback_enabled ? '✓' : ''),
      sortValue: (r) => (r.is_cashback_enabled ? 1 : 0),
      searchValue: (r) => String(r.is_cashback_enabled),
    },
  ];
}
