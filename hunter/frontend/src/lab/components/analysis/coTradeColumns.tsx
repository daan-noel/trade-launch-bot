import type { ColumnDef } from 'components/table/types';
import { Badge } from 'components/ui/Badge';
import { formatDecimalTrim } from 'utils/format';
import { compareWalletColor } from 'components/token-price-chart/constants';
import type { ProfileWalletInfo } from 'components/token-price-chart/types';
import type { CoTrader, TraderTokenRow } from 'types';
import {
  CO_BUCKET_VARIANT,
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
 *
 * `Lag` / `Coupling` are SINGLE-answer columns over a set that can hold several
 * wallets, so they follow the strip's focus: unfocused they report the row's
 * tightest coupling, focused they report that one wallet. Without the focus a
 * second wallet on a shared row is unsortable — which reads as the page only
 * working with the first comparison wallet.
 */

const shortAddr = (a: string) => `${a.slice(0, 4)}…${a.slice(-4)}`;

/** A wallet's display label + color from the tracked-profile set, falling back to
 *  a truncated address for one that isn't tracked. */
function walletLabel(address: string, byAddress: Map<string, ProfileWalletInfo>): string {
  return byAddress.get(address)?.label ?? shortAddr(address);
}

/** Address → the color that wallet draws in everywhere on this page.
 *
 *  Comparison wallets are SLOT-keyed (`compareWalletColor`) — the same call the
 *  chart markers and the summary strip's legend make. Reading the profile
 *  rotation color here instead would give a chip one hue and that wallet's
 *  markers another, which is exactly the confusion the slot palette exists to
 *  remove, and it only shows up once a second wallet is compared. */
function colorMap(
  profileWallets: ProfileWalletInfo[],
  comparison: string[],
): Map<string, string> {
  const byAddress = new Map(profileWallets.map((w) => [w.address, w]));
  const out = new Map<string, string>(profileWallets.map((w) => [w.address, w.color]));
  comparison.forEach((addr, slot) => out.set(addr, compareWalletColor(slot, byAddress.get(addr))));
  return out;
}

/** One comparison wallet as a colored chip carrying its own lag. Under a focus
 *  the chip for that wallet keeps full strength and the rest recede, so the
 *  column still shows who else was here without competing with the answer the
 *  single-value columns are now giving. */
function CoTraderChip({
  co,
  byAddress,
  colors,
  focus,
}: {
  co: CoTrader;
  byAddress: Map<string, ProfileWalletInfo>;
  colors: Map<string, string>;
  focus: string | null;
}) {
  const lag = co.entry_lag_slots;
  const focused = focus === co.wallet;
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
      className={`inline-flex items-center gap-1 rounded border px-1 py-[1px] text-[10px] ${
        focused
          ? 'border-white/30 bg-white/10'
          : `border-white/10 bg-white/5 ${focus ? 'opacity-45' : ''}`
      }`}
      title={detail}
    >
      <span
        className="size-1.5 rounded-full"
        style={{ background: colors.get(co.wallet) ?? '#888' }}
      />
      <span className="text-text">{walletLabel(co.wallet, byAddress)}</span>
      <span className={lag == null ? 'text-text-dim' : lag < 0 ? 'text-green' : 'text-text-dim'}>
        {formatLagSlots(lag)}
      </span>
    </span>
  );
}

/**
 * The co-trade column block.
 *
 * `profileWallets` supplies each comparison wallet's label, `comparison` (the
 * COMMITTED `with` list, in picker order) its slot color — so a chip here draws
 * in the same hue as that wallet's markers on the chart and its swatch in the
 * summary strip. One identity across the page, from one function.
 *
 * `focus` is the strip's selected comparison wallet, or `null` for the tightest-
 * coupling headline. It re-points `Lag` / `Coupling` (and their sort/filter keys)
 * at that one wallet; the multi-wallet columns — the co-trader chips, `Also`,
 * `First In` — always answer for the whole set.
 */
export function coTradeColumns(
  profileWallets: ProfileWalletInfo[],
  comparison: string[],
  focus: string | null = null,
): ColumnDef<TraderTokenRow>[] {
  const byAddress = new Map(profileWallets.map((w) => [w.address, w]));
  const colors = colorMap(profileWallets, comparison);
  // Named in the header + tooltip of every column the focus re-points, so a
  // one-wallet answer can never be misread as the whole comparison set's.
  const focusLabel = focus ? walletLabel(focus, byAddress) : null;
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
              <CoTraderChip
                key={c.wallet}
                co={c}
                byAddress={byAddress}
                colors={colors}
                focus={focus}
              />
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
        return (
          <span className="inline-flex items-center gap-1" title={first}>
            <span
              className="size-1.5 rounded-full"
              style={{ background: colors.get(first) ?? '#888' }}
            />
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
      label: focusLabel ? `Lag · ${focusLabel}` : 'Lag',
      group: 'co_trade',
      width: focusLabel ? '120px' : '78px',
      tooltip: focusLabel
        ? `Slot distance from the primary's entry to ${focusLabel}'s, signed: negative = that wallet was ahead. Blank on a token ${focusLabel} did not trade. 0 means they landed in the same slot, where the ordering is the tx index inside it (hover the chip). Clear the focus in the strip above to go back to the closest comparison entry of any wallet.`
        : "Slot distance from the primary's entry to the CLOSEST comparison entry, signed: negative = that wallet was ahead. 0 means they landed in the same slot, where the ordering is the tx index inside it (hover the chip). With several wallets compared, focus one in the strip above to read ITS lag on every row.",
      sortable: true,
      render: (r) => {
        const lag = coLagSlots(r, focus);
        if (lag == null) return <span className="text-text-dim">-</span>;
        const tone = lag === 0 ? 'text-accent' : lag < 0 ? 'text-green' : 'text-text';
        return <span className={tone}>{formatLagSlots(lag)}</span>;
      },
      sortValue: (r) => coLagSlots(r, focus),
      searchValue: () => '',
      filterNumber: (r) => coLagSlots(r, focus),
    },
    {
      key: 'co_bucket',
      label: focusLabel ? `Coupling · ${focusLabel}` : 'Coupling',
      group: 'co_trade',
      width: focusLabel ? '146px' : '104px',
      tooltip:
        `The Lag bucketed${focusLabel ? `, for ${focusLabel} alone` : ''}. co-slot = same block, so neither wallet could have seen the other: they reacted to the SAME tape event. leads / follows = within 3 slots either side. independent = further apart than any one event could drive, i.e. most likely coincidence.`,
      sortable: true,
      render: (r) => {
        const b = coBucket(r, focus);
        if (!b) return <span className="text-text-dim">-</span>;
        return (
          <Badge variant={CO_BUCKET_VARIANT[b]} size="sm" title={CO_TRADE_BUCKET_HINT[b]}>
            {b}
          </Badge>
        );
      },
      sortValue: (r) => coBucketRank(r, focus),
      searchValue: (r) => coBucket(r, focus) ?? '',
      filterValue: (r) => coBucket(r, focus) ?? '',
    },
  ];
}
