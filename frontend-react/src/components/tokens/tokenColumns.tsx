import { useState, type MouseEvent } from 'react';
import type { ColumnDef } from 'components/table/types';
import type { TokenRecord } from 'types';
import { DateCell } from 'components/table/DateCell';
import { RelativeTimeCell } from 'components/table/RelativeTimeCell';
import {
  ageClass,
  formatAge,
  formatCompact,
  formatDecimalTrim,
  formatWithCommas,
  priceClass,
  ratioClass,
} from 'utils/format';
import type { usePriceDisplay } from 'hooks/usePriceDisplay';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { cn } from 'lib/cn';

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
  const json = ixLabelsJson(row);

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

export function tokenColumns(price: ReturnType<typeof usePriceDisplay>): ColumnDef<TokenRecord>[] {
  return [
    {
      key: 'symbol',
      label: 'Symbol',
      width: '90px',
      sortable: true,
      render: (r) => r.symbol,
      sortValue: (r) => r.symbol,
      searchValue: (r) => `${r.symbol} ${r.name} ${r.mint_address}`,
    },
    {
      key: 'name',
      label: 'Name',
      width: '120px',
      sortable: true,
      render: (r) => r.name,
      sortValue: (r) => r.name,
      searchValue: (r) => r.name,
    },
    {
      key: 'mint',
      label: 'Mint',
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
      width: '165px',
      render: (r) => (
        <AddressDisplay address={r.create_tx_address} kind="transaction" stopPropagation />
      ),
      searchValue: (r) => r.create_tx_address,
    },
    {
      key: 'token_age',
      label: 'Token Age',
      width: '72px',
      sortable: true,
      render: (r) => <span className={ageClass(r.age)}>{formatAge(r.age)}</span>,
      sortValue: (r) => r.age,
      searchValue: (r) => formatAge(r.age),
    },
    {
      key: 'created',
      label: 'Created',
      width: '110px',
      sortable: true,
      render: (r) => <DateCell iso={r.created_at} />,
      sortValue: (r) => r.created_at,
      searchValue: (r) => r.created_at,
    },
    {
      key: 'last_trade',
      label: 'Last Trade',
      width: '110px',
      sortable: true,
      render: (r) => <DateCell iso={r.last_trade_at} />,
      sortValue: (r) => r.last_trade_at,
      searchValue: (r) => r.last_trade_at ?? '',
    },
    {
      key: 'lifetime',
      label: 'Lifetime',
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
      width: '96px',
      sortable: true,
      render: (r) => <RelativeTimeCell iso={r.last_synced_at} />,
      sortValue: (r) => r.last_synced_at,
      searchValue: (r) => r.last_synced_at ?? '',
    },
    {
      key: 'trade_count',
      label: 'Trades',
      width: '66px',
      sortable: true,
      render: (r) => r.trade_count,
      sortValue: (r) => r.trade_count,
      searchValue: (r) => String(r.trade_count),
    },
    {
      key: 'ath_price',
      label: 'ATH',
      width: '88px',
      sortable: true,
      render: (r) => (r.ath_price != null ? price.displayPrice(r.ath_price) : '-'),
      sortValue: (r) => r.ath_price,
      searchValue: (r) => String(r.ath_price ?? ''),
    },
    {
      key: 'ath_timestamp',
      label: 'ATH At',
      width: '110px',
      sortable: true,
      render: (r) => <DateCell iso={r.ath_timestamp} />,
      sortValue: (r) => r.ath_timestamp,
      searchValue: (r) => r.ath_timestamp ?? '',
    },
    {
      key: 'ath_fep_ratio',
      label: 'ATH/FEP',
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
    },
    {
      key: 'current_price',
      label: 'Price',
      width: '88px',
      sortable: true,
      render: (r) => (
        <span className={priceClass(r.current_price ?? undefined)}>
          {r.current_price != null ? price.displayPrice(r.current_price) : '-'}
        </span>
      ),
      sortValue: (r) => r.current_price,
      searchValue: (r) => String(r.current_price ?? ''),
    },
    {
      key: 'current_fep_ratio',
      label: 'Cur/FEP',
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
    },
    {
      key: 'market_cap',
      label: 'MCap',
      width: '84px',
      sortable: true,
      render: (r) => (r.market_cap != null ? price.displayCompact(r.market_cap, 3) : '-'),
      sortValue: (r) => r.market_cap,
      searchValue: (r) => String(r.market_cap ?? ''),
    },
    {
      key: 'volume',
      label: 'Volume',
      width: '78px',
      sortable: true,
      render: (r) => price.displayCompact(r.volume_sol_total, 4),
      sortValue: (r) => r.volume_sol_total,
      searchValue: (r) => String(r.volume_sol_total),
    },
    {
      key: 'initial_buy',
      label: 'Init Buy',
      width: '78px',
      sortable: true,
      render: (r) => (r.initial_buy_sol != null ? price.displayAmount(r.initial_buy_sol) : '-'),
      sortValue: (r) => r.initial_buy_sol,
      searchValue: (r) => String(r.initial_buy_sol ?? ''),
    },
    {
      key: 'init_supply',
      label: 'Init Supply',
      width: '90px',
      sortable: true,
      render: (r) =>
        r.initial_supply_token != null ? formatCompact(r.initial_supply_token, 2) : '-',
      sortValue: (r) => r.initial_supply_token,
      searchValue: (r) => String(r.initial_supply_token ?? ''),
    },
    {
      key: 'token_amount',
      label: 'Token Amt',
      width: '90px',
      sortable: true,
      render: (r) => (r.token_amount != null ? formatCompact(r.token_amount, 2) : '-'),
      sortValue: (r) => r.token_amount,
      searchValue: () => '',
    },
    {
      key: 'max_sol_cost',
      label: 'Max SOL Cost',
      width: '100px',
      sortable: true,
      render: (r) =>
        r.max_sol_cost != null ? formatDecimalTrim(r.max_sol_cost / 1e9, 3) : '-',
      sortValue: (r) => r.max_sol_cost,
      searchValue: () => '',
    },
    {
      key: 'spendable_sol_in',
      label: 'Spendable SOL In',
      width: '100px',
      sortable: true,
      render: (r) =>
        r.spendable_sol_in != null ? formatDecimalTrim(r.spendable_sol_in / 1e9, 3) : '-',
      sortValue: (r) => r.spendable_sol_in,
      searchValue: () => '',
    },
    {
      key: 'min_tokens_out',
      label: 'Min Tokens',
      width: '90px',
      sortable: true,
      render: (r) => (r.min_tokens_out != null ? formatCompact(r.min_tokens_out, 2) : '-'),
      sortValue: (r) => r.min_tokens_out,
      searchValue: () => '',
    },
    {
      key: 'cu_limit',
      label: 'CU Limit',
      width: '72px',
      sortable: true,
      render: (r) => (r.cu_limit != null ? r.cu_limit : '-'),
      sortValue: (r) => r.cu_limit,
      searchValue: (r) => String(r.cu_limit ?? ''),
    },
    {
      key: 'cu_price',
      label: 'CU Price',
      width: '72px',
      sortable: true,
      render: (r) => (r.cu_price != null ? formatWithCommas(r.cu_price) : '-'),
      sortValue: (r) => r.cu_price,
      searchValue: (r) => String(r.cu_price ?? ''),
    },
    {
      key: 'ix_count',
      label: 'IX Count',
      width: '54px',
      sortable: true,
      render: (r) => <IxCountCell row={r} />,
      sortValue: (r) => r.ix_labels_count,
      searchValue: (r) => String(r.ix_labels_count),
    },
    {
      key: 'ix_labels',
      label: 'IX Labels',
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
      width: '66px',
      sortable: true,
      render: (r) => (r.is_migrated ? '✓' : ''),
      sortValue: (r) => (r.is_migrated ? 1 : 0),
      searchValue: (r) => String(r.is_migrated),
    },
    {
      key: 'mayhem_mode',
      label: 'Mayhem',
      width: '66px',
      sortable: true,
      render: (r) => (r.is_mayhem_mode ? '✓' : ''),
      sortValue: (r) => (r.is_mayhem_mode ? 1 : 0),
      searchValue: (r) => String(r.is_mayhem_mode),
    },
    {
      key: 'cashback',
      label: 'Cashback',
      width: '72px',
      sortable: true,
      render: (r) => (r.is_cashback_enabled ? '✓' : ''),
      sortValue: (r) => (r.is_cashback_enabled ? 1 : 0),
      searchValue: (r) => String(r.is_cashback_enabled),
    },
  ];
}
