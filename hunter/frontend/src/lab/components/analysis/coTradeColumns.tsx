import type { ColumnDef } from 'components/table/types';
import { Badge } from 'components/ui/Badge';
import { formatDecimalTrim } from 'utils/format';
import type { ProfileWalletInfo } from 'components/token-price-chart/types';
import type { CoTradeBucket, CoTrader, TraderTokenRow } from 'types';
import {
  CO_TRADE_BUCKET_HINT,
  coBucket,
  coBucketRank,
  coCount,
  coLagSlots,
  firstMover,
  formatLagSlots,
} from './coTrade';

/**
 * Trader Analysis co-trade columns — who ELSE among the comparison wallets was
 * on this mint, and where their entry sits against the primary's on the tape.
 *
 * Spliced in beside `walletTokenColumns()` only while the query names comparison
 * wallets, so the single-wallet page (the common one) keeps exactly the layout it
 * had. Same rule as that file: wallet-only fields never reach the shared
 * `tokenColumns()` SSOT.
 *
 * Every ordering here comes from `(slot, tx_index)` off the entry leg, never
 * from a timestamp — `block_time` is second-precision and ties across a whole
 * slot, which is precisely the resolution the question needs.
 */

const shortAddr = (a: string) => `${a.slice(0, 4)}…${a.slice(-4)}`;

/** A wallet's display label + color from the tracked-profile set, falling back to
 *  a truncated address for one that isn't tracked. */
function walletLabel(address: string, byAddress: Map<string, ProfileWalletInfo>): string {
  return byAddress.get(address)?.label ?? shortAddr(address);
}

const BUCKET_VARIANT: Record<CoTradeBucket, 'accent' | 'success' | 'info' | 'neutral'> = {
  // Same slot is the finding, so it gets the loudest chip.
  'co-slot': 'accent',
  leads: 'success',
  follows: 'info',
  independent: 'neutral',
};

/** One comparison wallet as a colored chip carrying its own lag. */
function CoTraderChip({
  co,
  byAddress,
}: {
  co: CoTrader;
  byAddress: Map<string, ProfileWalletInfo>;
}) {
  const info = byAddress.get(co.wallet);
  const lag = co.entry_lag_slots;
  const detail = [
    `${co.wallet}`,
    lag == null
      ? 'entry ordering unknown (no entry leg for one of the two in this window)'
      : lag === 0
        ? `same slot, ${formatDecimalTrim(co.entry_lag_tx ?? 0, 0)} tx ${
            (co.entry_lag_tx ?? 0) < 0 ? 'ahead of' : 'behind'
          } the primary`
        : `${Math.abs(lag)} slot${Math.abs(lag) === 1 ? '' : 's'} ${lag < 0 ? 'ahead of' : 'behind'} the primary`,
    `${co.buy_count} buys / ${co.sell_count} sells`,
    co.entry_curve_pct != null ? `entered at ${formatDecimalTrim(co.entry_curve_pct, 1)}% curve` : '',
  ]
    .filter(Boolean)
    .join('\n');
  return (
    <span
      className="inline-flex items-center gap-1 rounded border border-white/10 bg-white/5 px-1 py-[1px] text-[10px]"
      title={detail}
    >
      <span className="size-1.5 rounded-full" style={{ background: info?.color ?? '#888' }} />
      <span className="text-text">{walletLabel(co.wallet, byAddress)}</span>
      <span className={lag == null ? 'text-text-dim' : lag < 0 ? 'text-green' : 'text-text-dim'}>
        {formatLagSlots(lag)}
      </span>
    </span>
  );
}

/**
 * The co-trade column block. `profileWallets` supplies each comparison wallet's
 * color and label so a chip here matches the same wallet's markers on the chart
 * — one identity across the page.
 */
export function coTradeColumns(profileWallets: ProfileWalletInfo[]): ColumnDef<TraderTokenRow>[] {
  const byAddress = new Map(profileWallets.map((w) => [w.address, w]));
  return [
    {
      key: 'co_n',
      label: 'Also',
      group: 'co_trade',
      width: '54px',
      tooltip:
        'How many of the comparison wallets also traded this mint in the window. 0 = only the primary was here.',
      sortable: true,
      render: (r) => {
        const n = coCount(r);
        return n === 0 ? <span className="text-text-dim">-</span> : <span className="text-text">{n}</span>;
      },
      sortValue: coCount,
      searchValue: () => '',
      filterNumber: coCount,
    },
    {
      key: 'co_wallets',
      label: 'Co-traders',
      group: 'co_trade',
      width: '220px',
      tooltip:
        "The comparison wallets that were also on this mint, earliest entry first, each with its entry lag in slots against the primary. A negative lag means that wallet got in FIRST. Hover a chip for its full detail.",
      sortable: false,
      render: (r) =>
        r.co_traders.length === 0 ? (
          <span className="text-text-dim">-</span>
        ) : (
          <span className="flex flex-wrap gap-1">
            {r.co_traders.map((c) => (
              <CoTraderChip key={c.wallet} co={c} byAddress={byAddress} />
            ))}
          </span>
        ),
      // Searching / filtering on the labels is how you isolate one wallet's
      // overlaps without re-running the query.
      searchValue: (r) => r.co_traders.map((c) => `${walletLabel(c.wallet, byAddress)} ${c.wallet}`).join(' '),
      filterValue: (r) => r.co_traders.map((c) => walletLabel(c.wallet, byAddress)).join(' '),
    },
    {
      key: 'co_first',
      label: 'First In',
      group: 'co_trade',
      width: '92px',
      tooltip:
        'Who entered first across the primary and every comparison wallet, by (slot, tx_index). Blank when the ordering is unknown — an entry that predates the window has no tape position.',
      sortable: true,
      render: (r) => {
        const first = firstMover(r);
        if (first == null) return <span className="text-text-dim">-</span>;
        if (first === '') return <span className="text-text-dim">primary</span>;
        const info = byAddress.get(first);
        return (
          <span className="inline-flex items-center gap-1" title={first}>
            <span className="size-1.5 rounded-full" style={{ background: info?.color ?? '#888' }} />
            <span className="text-text">{walletLabel(first, byAddress)}</span>
          </span>
        );
      },
      // Sort primary-first, then by label, unknown last.
      sortValue: (r) => {
        const first = firstMover(r);
        if (first == null) return null;
        return first === '' ? '' : walletLabel(first, byAddress);
      },
      searchValue: (r) => {
        const first = firstMover(r);
        return first == null ? '' : first === '' ? 'primary' : `${walletLabel(first, byAddress)} ${first}`;
      },
    },
    {
      key: 'co_lag',
      label: 'Lag',
      group: 'co_trade',
      width: '78px',
      tooltip:
        "Slot distance from the primary's entry to the CLOSEST comparison entry, signed: negative = that wallet was ahead. 0 means they landed in the same slot, where the ordering is the tx index inside it (hover the chip).",
      sortable: true,
      render: (r) => {
        const lag = coLagSlots(r);
        if (lag == null) return <span className="text-text-dim">-</span>;
        const tone = lag === 0 ? 'text-accent' : lag < 0 ? 'text-green' : 'text-text';
        return <span className={tone}>{formatLagSlots(lag)}</span>;
      },
      sortValue: coLagSlots,
      searchValue: () => '',
      filterNumber: coLagSlots,
    },
    {
      key: 'co_bucket',
      label: 'Coupling',
      group: 'co_trade',
      width: '104px',
      tooltip:
        'The Lag bucketed. co-slot = same block, so neither wallet could have seen the other: they reacted to the SAME tape event. leads / follows = within 3 slots either side. independent = further apart than any one event could drive, i.e. most likely coincidence.',
      sortable: true,
      render: (r) => {
        const b = coBucket(r);
        if (!b) return <span className="text-text-dim">-</span>;
        return (
          <Badge variant={BUCKET_VARIANT[b]} size="sm" title={CO_TRADE_BUCKET_HINT[b]}>
            {b}
          </Badge>
        );
      },
      sortValue: coBucketRank,
      searchValue: (r) => coBucket(r) ?? '',
      filterValue: (r) => coBucket(r) ?? '',
    },
  ];
}
