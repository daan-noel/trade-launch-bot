import type { ColumnDef } from 'components/table/types';
import { DateCell } from 'components/table/DateCell';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { Badge } from 'components/ui/Badge';
import { IxLabelsDisplay } from 'components/ui/IxLabelsDisplay';
import { IX_ABBREV_LEGEND } from 'lib/ixLabels';
import { LensButton, LensSpacer } from 'components/tokens/LensControls';
// Deep import: type-only w.r.t. lightweight-charts (see `LensControls`).
import { LENS_COLORS } from 'components/token-price-chart/constants';
import { preEntryColumns } from '@lab/components/analysis/preEntryColumns';
import { WALLET_STATS } from '@lab/components/analysis/walletPnlStats';
import { SignedPct } from '@lab/components/analysis/walletTokenColumns';
import type { WalletEpisode, WalletEpisodeStatus } from 'types';
import type { PreEntryVerdict } from '@lab/lib/preEntryProbeTypes';
import { ENTRY_AXES, formatAxis } from '@lab/lib/entryContext/axes';
import type { EntryGroupRow, EntryRow } from '@lab/lib/entryContext/types';
import { signalBuyColumns } from './SignalPanel';

/**
 * Entry rows: which token and when he bought, the pre-entry probe's own columns
 * while the probe is on (the same set Trader Analysis shows per token), then one
 * column per axis in `ENTRY_AXES` with its definition as the header tooltip. The
 * table's filter row filters on every one of them.
 */
export function entryColumns(
  windowSecs: number,
  tokenLabel: (mint: string) => string,
  verdictOf: ((e: EntryRow) => PreEntryVerdict | undefined) | null,
  probeSlots: number,
  /** His round trip this buy belongs to (`tradeOfBuy`), for the probe group's
   *  PnL % column; `null` while his token rows are not loaded. */
  tradeOf: ((e: EntryRow) => WalletEpisode | null) | null = null,
): ColumnDef<EntryRow>[] {
  const axisCols: ColumnDef<EntryRow>[] = ENTRY_AXES.map((a) => ({
    key: a.key,
    label: a.label,
    tooltip: a.definition(windowSecs),
    group: a.group,
    render: (e) => (e.unknown_reason ? <span className="text-text-dim">-</span> : formatAxis(a, a.get(e))),
    sortValue: (e) => (e.unknown_reason ? null : a.get(e)),
    searchValue: () => '',
    filterNumber: (e) => (e.unknown_reason ? null : a.get(e)),
  }));
  const ofGroup = (g: string) => axisCols.filter((c) => c.group === g);
  return [
    {
      key: 'token',
      label: 'Token',
      group: 'buy',
      render: (e) => (
        <AddressDisplay
          address={e.mint_address}
          kind="token"
          display={tokenLabel(e.mint_address)}
          className="font-semibold"
          stopPropagation
        />
      ),
      sortValue: (e) => tokenLabel(e.mint_address),
      searchValue: (e) => `${tokenLabel(e.mint_address)} ${e.mint_address}`,
    },
    {
      key: 'at',
      label: 'Time',
      group: 'buy',
      tooltip: 'When he bought.',
      render: (e) => <DateCell iso={e.at} />,
      sortValue: (e) => e.slot * 10_000 + e.tx_index,
      searchValue: (e) => e.at,
    },
    ...signalBuyColumns(),
    ...ofGroup('buy'),
    ...(verdictOf ? preEntryColumns<EntryRow>(verdictOf, probeSlots) : []),
    ...(verdictOf && tradeOf ? [tradePctColumn(tradeOf)] : []),
    ...axisCols.filter((c) => c.group !== 'buy'),
  ];
}

/** Why a buy's trade has no PnL %, on hover. */
const NO_PCT: Record<WalletEpisodeStatus, string> = {
  closed: '',
  open: 'Still holding: no exact PnL until it sells.',
  incomplete: 'Incomplete trade: it cannot be priced exactly.',
};

/** His result on the round trip this buy opened or added to, beside the probe's
 *  verdict on the same buy. A filter on it picks which buys are asked (a scope in
 *  `entryLogic`, the key carries no `pe_` prefix), never a signal. */
function tradePctColumn(tradeOf: (e: EntryRow) => WalletEpisode | null): ColumnDef<EntryRow> {
  const pct = (e: EntryRow) => tradeOf(e)?.pnl_pct ?? null;
  return {
    key: 'his_pnl_pct',
    label: WALLET_STATS.tradePct.label,
    group: 'pre_entry',
    width: '78px',
    tooltip:
      `His result on the trade this buy belongs to. ${WALLET_STATS.tradePct.def}\n` +
      `Every buy of one round trip shows the same number. '-' = still holding, incomplete, or the trade closed outside the dates.`,
    sortable: true,
    render: (e) => {
      const ep = tradeOf(e);
      return (
        <span title={ep ? NO_PCT[ep.status] || undefined : 'The trade closed outside the dates.'}>
          <SignedPct pct={ep?.pnl_pct ?? null} />
        </span>
      );
    },
    sortValue: pct,
    searchValue: () => '',
    filterNumber: pct,
  };
}

/** The breakdown's time span: the analysis window, or the range picked on the chart. */
const IN_RANGE = 'in the analysis range (the last seconds before his buy, or the range you picked on the chart)';

const pctText = (v: number | null) => (v == null ? '-' : `${v.toFixed(0)}%`);

/** Arms the chart's structure highlight from a Structure cell (`useTokenHighlight`). */
export interface StructureLens {
  /** Add this exact ordered structure, unpinned (a breakdown row has no single
   *  fee reading to copy); an armed one again removes it. */
  toggle: (labels: readonly string[]) => void;
  /** The color the unpinned structure washes in, `null` = not armed, so its row
   *  renders the button lit in that color. */
  colorOf: (labels: readonly string[]) => string | null;
}

/** The Structure column of both structure tables: one narrow line, so the number
 *  columns stay on screen. Under Exact it is the ordered sequence abbreviated
 *  (`abbreviateIxLabels`), the full list on hover and click-to-copy; under
 *  Template / Program the group's name. With a `lens`, an Exact row carries the
 *  trades table's highlight button: it washes every candle with that structure. */
export function structureColumn<R extends { key: string; labels?: string[] }>(
  lens?: StructureLens | null,
): ColumnDef<R> {
  const button = (g: R) => {
    if (!lens) return null;
    if (!g.labels?.length) return <LensSpacer />;
    const armedColor = lens.colorOf(g.labels);
    const armed = armedColor != null;
    return (
      <LensButton
        armed={armed}
        color={armedColor ?? LENS_COLORS.structure[0]}
        title={armed ? 'Stop highlighting this ix structure' : 'Highlight every candle with this exact ordered structure'}
        onClick={() => lens.toggle(g.labels!)}
      />
    );
  };
  return {
    key: 'key',
    label: 'Structure',
    group: 'structure',
    width: '200px',
    tooltip:
      'Instructions in order, abbreviated. Hover a cell for the full list.' +
      (lens ? ' Click the target to highlight that structure on the chart.' : '') +
      `

${IX_ABBREV_LEGEND}`,
    render: (g) => (
      <span className="flex min-w-0 items-center gap-1">
        {button(g)}
        {g.labels ? (
          <IxLabelsDisplay labels={g.labels} compact copyJson className="min-w-0 flex-1" />
        ) : (
          <span className="block min-w-0 flex-1 truncate font-mono text-[11px]" title={g.key}>
            {g.key}
          </span>
        )}
      </span>
    ),
    searchValue: (g) => g.key,
  };
}

/** Group banners of a buy's breakdown table. */
export const GROUP_TABLE_LABELS: Record<string, string> = {
  structure: 'Structure',
  buys: 'Buys',
  sells: 'Sells',
  spread: 'Spread',
};

/** Buys columns in the buy color, Sells in the sell color (the candle colors); a 0
 *  or empty cell is dim. */
const SIDE_TINT: Readonly<Record<string, string>> = { buys: 'text-buy', sells: 'text-sell' };

/** One buy's window, by structure. `lens` wires the Structure cell's highlight button. */
export function groupColumns(lens?: StructureLens | null): ColumnDef<EntryGroupRow>[] {
  const num = (
    group: string,
    key: string,
    label: string,
    tooltip: string,
    get: (g: EntryGroupRow) => number | null,
    fmt: (v: number | null) => string,
  ): ColumnDef<EntryGroupRow> => ({
    key,
    label,
    group,
    tooltip,
    render: (g) => {
      const v = get(g);
      return <span className={v ? SIDE_TINT[group] : 'text-white/30'}>{fmt(v)}</span>;
    },
    sortValue: get,
    searchValue: () => '',
    filterNumber: get,
  });
  const sol = (v: number | null) => (v == null ? '-' : `◎${v.toFixed(3)}`);
  const int = (v: number | null) => (v == null ? '-' : String(v));
  return [
    structureColumn<EntryGroupRow>(lens),
    {
      key: 'tagged',
      label: 'Target',
      group: 'structure',
      tooltip:
        'Is this structure the target?\n' +
        "'tag' = all its buys matched the target. '3/5' = 3 of its 5 buys did. '-' = none.\n" +
        "'reserve' = the structure of the print reserve match named.",
      render: (g) =>
        g.tag_buy_tx > 0 || g.reserve ? (
          <span className="flex items-center gap-1">
            {g.reserve && (
              <Badge variant="info" size="sm">
                reserve
              </Badge>
            )}
            {g.tag_buy_tx > 0 && (
              <Badge variant="accent" size="sm">
                {g.tag_buy_tx === g.buy_tx ? 'tag' : `${g.tag_buy_tx}/${g.buy_tx}`}
              </Badge>
            )}
          </span>
        ) : (
          <span className="text-text-dim">-</span>
        ),
      sortValue: (g) => (g.reserve ? 1e9 : 0) + g.tag_buy_tx,
      searchValue: () => '',
    },
    num('buys', 'buy_tx', 'Buys', `How many buy transactions this structure made ${IN_RANGE}.`, (g) => g.buy_tx, int),
    num(
      'buys',
      'buy_tx_share',
      'Tx %',
      `Of all buy transactions ${IN_RANGE}, the % this structure made.\nExample: 25 buys, 5 by it = 20%.`,
      (g) => g.buy_tx_share_pct,
      pctText,
    ),
    num('buys', 'buy_sol', 'Buy SOL', `SOL this structure spent on buys ${IN_RANGE}.`, (g) => g.buy_sol, sol),
    num(
      'buys',
      'buy_sol_share',
      'SOL %',
      `Of all SOL spent on buys ${IN_RANGE}, the % this structure spent.\nExample: 10 SOL of buys, 2 SOL by it = 20%.`,
      (g) => g.buy_sol_share_pct,
      pctText,
    ),
    num('sells', 'sell_tx', 'Sells', `How many sell transactions this structure made ${IN_RANGE}.`, (g) => g.sell_tx, int),
    num('sells', 'sell_sol', 'Sell SOL', `SOL this structure's sells took out ${IN_RANGE}.`, (g) => g.sell_sol, sol),
    num(
      'spread',
      'wallets',
      'Wallets',
      `How many different wallets used this structure ${IN_RANGE}.\n1 = one wallet; many = a shared tool or a crowd.`,
      (g) => g.wallets,
      int,
    ),
    num(
      'spread',
      'buy_secs',
      'Buy secs',
      `In how many different seconds this structure bought ${IN_RANGE}.\n1 = all its buys in the same second (a burst); high = spread out.`,
      (g) => g.buy_secs,
      int,
    ),
  ];
}
