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

/**
 * Token age in seconds, derived client-side from `created_at`. The server no
 * longer ships a `now`-relative `age` field (it would churn the list body on
 * every poll and defeat the endpoint's content-hash ETag); deriving it here
 * recomputes the value on each render — i.e. it refreshes on every poll/refetch
 * rather than baking a stale server snapshot into the row. (It does not tick on
 * its own between fetches; nothing drives a re-render in that window.)
 */
function ageSecondsOf(r: TokenRecord): number {
  return Math.max(0, (Date.now() - new Date(r.created_at).getTime()) / 1000);
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
      render: (r) => {
        const age = ageSecondsOf(r);
        return <span className={ageClass(age)}>{formatAge(age)}</span>;
      },
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
      render: (r) => (r.ath_price != null ? price.displayPrice(r.ath_price) : '-'),
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
      render: (r) => (r.market_cap != null ? price.displayCompact(r.market_cap, 3) : '-'),
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
      render: (r) => price.displayCompact(r.volume_sol_total, 4),
      sortValue: (r) => r.volume_sol_total,
      searchValue: (r) => String(r.volume_sol_total),
      filterNumber: (r) => r.volume_sol_total,
    },
    {
      key: 'initial_buy',
      label: 'Init Buy',
      group: 'initial',
      width: '78px',
      sortable: true,
      render: (r) => (r.initial_buy_sol != null ? price.displayAmount(r.initial_buy_sol) : '-'),
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
      key: 'max_sol_cost',
      label: 'Max SOL Cost',
      group: 'max_or_spendable',
      width: '100px',
      sortable: true,
      render: (r) =>
        r.max_sol_cost != null ? formatDecimalTrim(r.max_sol_cost / 1e9, 3) : '-',
      sortValue: (r) => r.max_sol_cost,
      searchValue: () => '',
      filterValue: (r) =>
        r.max_sol_cost != null ? formatDecimalTrim(r.max_sol_cost / 1e9, 3) : '',
      filterNumber: (r) => (r.max_sol_cost != null ? r.max_sol_cost / 1e9 : null),
    },
    {
      key: 'spendable_sol_in',
      label: 'Spendable SOL In',
      group: 'max_or_spendable',
      width: '100px',
      sortable: true,
      render: (r) =>
        r.spendable_sol_in != null ? formatDecimalTrim(r.spendable_sol_in / 1e9, 3) : '-',
      sortValue: (r) => r.spendable_sol_in,
      searchValue: () => '',
      filterValue: (r) =>
        r.spendable_sol_in != null ? formatDecimalTrim(r.spendable_sol_in / 1e9, 3) : '',
      filterNumber: (r) => (r.spendable_sol_in != null ? r.spendable_sol_in / 1e9 : null),
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
