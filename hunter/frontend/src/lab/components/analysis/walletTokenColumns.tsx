import type { ColumnDef } from 'components/table/types';
import type { TraderTokenRow } from 'types';
import { DateCell } from 'components/table/DateCell';
import { AmountCell, FeeCell, PriceCell } from 'components/tokens/priceCells';
import { Badge } from 'components/ui/Badge';
import { ageClass, formatAge, formatDecimalTrim } from 'utils/format';
import { walletHoldSeconds } from './walletPnlStats';

/**
 * Trader Analysis wallet columns — the position the wallet held on each mint,
 * plus the bonding-curve depth it entered and exited at.
 *
 * Appended to the shared `tokenColumns()` set at the page level rather than
 * added to it: that file is the SSOT for every token table in both products, so
 * a wallet-only field must not leak into All Tokens / the strategy tables.
 *
 * Grain caveat, true of every field here: a row is the wallet's WHOLE window on
 * one mint, not one round trip. A wallet that re-entered a mint five times shows
 * one entry (its first buy), one exit (its last sell), and a `Hold` that spans
 * every re-entry — see the backend `kernel::wallet_mint_pnl` doc comment.
 */

/** Token creation as epoch-ms — the instant both age columns measure from.
 *  Prefers the value the RTK transform pre-parsed once. */
function createdMsOf(r: TraderTokenRow): number {
  return r.created_at_ms ?? Date.parse(r.created_at);
}

/**
 * Seconds from the token's creation to one of the wallet's legs — how old the
 * token was when it entered / exited.
 *
 * `null` when that leg is absent (no buy in the window / still holding).
 *
 * Both instants come from `trades.block_time`, which on live-ingested rows is
 * this box's observation clock rather than pure chain time — so an age is
 * accurate to the feed, not to the slot. Read it as minutes-vs-hours, not as a
 * latency measurement.
 */
function legAgeSeconds(r: TraderTokenRow, legMs: number | null | undefined): number | null {
  if (legMs == null || !Number.isFinite(legMs)) return null;
  const created = createdMsOf(r);
  if (!Number.isFinite(created)) return null;
  return Math.max(0, (legMs - created) / 1000);
}

/** Epoch-ms of a nullable wallet leg, preferring the pre-parsed field. */
function legMs(iso: string | null, pre: number | null | undefined): number | null {
  if (pre != null) return pre;
  return iso != null ? Date.parse(iso) : null;
}

const entryAgeOf = (r: TraderTokenRow) =>
  legAgeSeconds(r, legMs(r.wallet_entry_at, r.wallet_entry_at_ms));
const exitAgeOf = (r: TraderTokenRow) =>
  legAgeSeconds(r, legMs(r.wallet_exit_at, r.wallet_exit_at_ms));

/** The pump.fun protocol fee the reconstruction charges this row — the gap
 *  between the gross and net-of-fee realized figures. Network fees and the Jito
 *  tip are not in it: this grain cannot see either. */
const feeSolOf = (r: TraderTokenRow) =>
  r.wallet_realized_pnl_sol - r.wallet_realized_pnl_sol_net_of_fee;

/** Curve progress gained across the hold; `null` unless both legs are present. */
function curveDeltaOf(r: TraderTokenRow): number | null {
  const a = r.wallet_entry_curve_pct;
  const b = r.wallet_exit_curve_pct;
  return a != null && b != null ? b - a : null;
}

/** Curve depth as a percent of the graduation line, tinted by how close to
 *  migration the wallet traded. */
function CurveCell({ pct, sol }: { pct: number | null; sol: number | null }) {
  if (pct == null || sol == null) return <>-</>;
  const tone = pct >= 100 ? 'text-purple' : pct >= 50 ? 'text-green' : 'text-text';
  return (
    <span className={tone} title={`${formatDecimalTrim(sol, 2)} real SOL in the pool`}>
      {formatDecimalTrim(pct, 1)}%
    </span>
  );
}

/** An age fixed to a past instant — unlike `AgeCell`, which ticks against now. */
function StaticAge({ seconds }: { seconds: number | null }) {
  if (seconds == null) return <>-</>;
  return <span className={ageClass(seconds)}>{formatAge(seconds)}</span>;
}

/** A signed percent, green when it went the wallet's way. */
function SignedPct({ pct }: { pct: number | null }) {
  if (pct == null) return <>-</>;
  return (
    <span className={pct >= 0 ? 'text-green' : 'text-red'}>
      {pct >= 0 ? '+' : ''}
      {formatDecimalTrim(pct, 1)}%
    </span>
  );
}

/** Second-order fields, hidden on first paint so the position reads at a glance.
 *  Every one is a click away in the Columns panel. */
const HIDDEN_KEYS = new Set(['w_entry', 'w_exit', 'w_avg_buy', 'w_avg_sell', 'w_fee']);

export function walletTokenColumns(): ColumnDef<TraderTokenRow>[] {
  const cols: ColumnDef<TraderTokenRow>[] = [
    // ── position ────────────────────────────────────────────────────────────
    {
      key: 'w_entry',
      label: 'Entry',
      group: 'wallet_pos',
      width: '110px',
      tooltip: "The wallet's first buy on this mint in the window",
      sortable: true,
      render: (r) => (r.wallet_entry_at ? <DateCell iso={r.wallet_entry_at} /> : '-'),
      sortValue: (r) => r.wallet_entry_at,
      searchValue: (r) => r.wallet_entry_at ?? '',
    },
    {
      key: 'w_entry_age',
      label: 'Entry Age',
      group: 'wallet_pos',
      width: '80px',
      tooltip: 'Token age when the wallet bought in (creation → first buy)',
      sortable: true,
      render: (r) => <StaticAge seconds={entryAgeOf(r)} />,
      sortValue: entryAgeOf,
      searchValue: () => '',
      filterValue: (r) => {
        const s = entryAgeOf(r);
        return s != null ? formatAge(s) : '';
      },
      filterNumber: entryAgeOf,
    },
    {
      key: 'w_exit',
      label: 'Exit',
      group: 'wallet_pos',
      width: '110px',
      tooltip: "The wallet's last sell on this mint in the window",
      sortable: true,
      render: (r) => (r.wallet_exit_at ? <DateCell iso={r.wallet_exit_at} /> : '-'),
      sortValue: (r) => r.wallet_exit_at,
      searchValue: (r) => r.wallet_exit_at ?? '',
    },
    {
      key: 'w_exit_age',
      label: 'Exit Age',
      group: 'wallet_pos',
      width: '80px',
      tooltip: 'Token age when the wallet sold out (creation → last sell)',
      sortable: true,
      render: (r) => <StaticAge seconds={exitAgeOf(r)} />,
      sortValue: exitAgeOf,
      searchValue: () => '',
      filterValue: (r) => {
        const s = exitAgeOf(r);
        return s != null ? formatAge(s) : '';
      },
      filterNumber: exitAgeOf,
    },
    {
      key: 'w_hold',
      label: 'Hold',
      group: 'wallet_pos',
      width: '80px',
      tooltip:
        'First → last trade span in the window. A wallet that re-entered this mint has every re-entry inside this span, so it is an exposure envelope, not one round trip.',
      sortable: true,
      render: (r) => <StaticAge seconds={walletHoldSeconds(r)} />,
      sortValue: walletHoldSeconds,
      searchValue: () => '',
      filterValue: (r) => {
        const s = walletHoldSeconds(r);
        return s != null ? formatAge(s) : '';
      },
      filterNumber: walletHoldSeconds,
    },
    {
      key: 'w_buys',
      label: 'Buys',
      group: 'wallet_pos',
      width: '58px',
      tooltip: 'Buy legs in the window (0 = the wallet only exited here)',
      sortable: true,
      render: (r) => <span className="text-buy">{r.wallet_buy_count}</span>,
      sortValue: (r) => r.wallet_buy_count,
      searchValue: () => '',
      filterNumber: (r) => r.wallet_buy_count,
    },
    {
      key: 'w_sells',
      label: 'Sells',
      group: 'wallet_pos',
      width: '58px',
      tooltip: 'Sell legs in the window',
      sortable: true,
      render: (r) => <span className="text-sell">{r.wallet_sell_count}</span>,
      sortValue: (r) => r.wallet_sell_count,
      searchValue: () => '',
      filterNumber: (r) => r.wallet_sell_count,
    },
    {
      key: 'w_buy_sol',
      label: 'Bought',
      group: 'wallet_pos',
      width: '88px',
      tooltip: 'Sum of SOL bought in the window (curve-side amount, before the protocol fee)',
      sortable: true,
      render: (r) => <AmountCell sol={r.wallet_buy_sol} />,
      sortValue: (r) => r.wallet_buy_sol,
      searchValue: () => '',
      filterNumber: (r) => r.wallet_buy_sol,
    },
    {
      key: 'w_sell_sol',
      label: 'Sold',
      group: 'wallet_pos',
      width: '88px',
      tooltip: 'Sum of SOL sold in the window (curve-side proceeds, before the protocol fee)',
      sortable: true,
      render: (r) => <AmountCell sol={r.wallet_sell_sol} />,
      sortValue: (r) => r.wallet_sell_sol,
      searchValue: () => '',
      filterNumber: (r) => r.wallet_sell_sol,
    },
    {
      key: 'w_avg_buy',
      label: 'Avg Buy',
      group: 'wallet_pos',
      width: '88px',
      tooltip: 'Weighted-average buy price (SOL per raw token unit)',
      sortable: true,
      render: (r) => <PriceCell sol={r.wallet_avg_buy_price} />,
      sortValue: (r) => r.wallet_avg_buy_price,
      searchValue: () => '',
      filterNumber: (r) => r.wallet_avg_buy_price,
    },
    {
      key: 'w_avg_sell',
      label: 'Avg Sell',
      group: 'wallet_pos',
      width: '88px',
      tooltip: 'Weighted-average sell price (SOL per raw token unit)',
      sortable: true,
      render: (r) => <PriceCell sol={r.wallet_avg_sell_price} />,
      sortValue: (r) => r.wallet_avg_sell_price,
      searchValue: () => '',
      filterNumber: (r) => r.wallet_avg_sell_price,
    },
    {
      key: 'w_pnl',
      label: 'PnL',
      group: 'wallet_pos',
      width: '92px',
      tooltip: 'Realized PnL plus the mark-to-market on any still-open bag',
      sortable: true,
      render: (r) => (
        <span className={r.wallet_total_pnl_sol >= 0 ? 'text-green' : 'text-red'}>
          <AmountCell sol={r.wallet_total_pnl_sol} />
        </span>
      ),
      sortValue: (r) => r.wallet_total_pnl_sol,
      searchValue: () => '',
      filterNumber: (r) => r.wallet_total_pnl_sol,
    },
    {
      key: 'w_pnl_pct',
      label: 'PnL %',
      group: 'wallet_pos',
      width: '78px',
      tooltip: 'Realized PnL over the matched cost basis. Blank with no buys in the window.',
      sortable: true,
      render: (r) => <SignedPct pct={r.wallet_realized_pnl_pct} />,
      sortValue: (r) => r.wallet_realized_pnl_pct,
      searchValue: () => '',
      filterNumber: (r) => r.wallet_realized_pnl_pct,
    },
    {
      key: 'w_fee',
      label: 'Fee',
      group: 'wallet_pos',
      width: '82px',
      tooltip:
        'pump.fun protocol fee the PnL reconstruction charges (~125bps per leg). Network fees and the Jito tip are NOT in here — a tipping wallet pays more than this.',
      sortable: true,
      render: (r) => <FeeCell sol={feeSolOf(r)} />,
      sortValue: feeSolOf,
      searchValue: () => '',
      filterNumber: feeSolOf,
    },
    {
      key: 'w_state',
      label: 'State',
      group: 'wallet_pos',
      width: '78px',
      tooltip:
        'open = still holding a bag. partial = sold more than it bought in this window, so the cost basis predates the look-back and every PnL figure is an estimate.',
      sortable: true,
      render: (r) => (
        <span className="flex gap-1">
          {r.wallet_is_open && (
            <Badge variant="info" size="sm">
              open
            </Badge>
          )}
          {r.wallet_partial_data && (
            <Badge variant="warning" size="sm">
              partial
            </Badge>
          )}
          {!r.wallet_is_open && !r.wallet_partial_data && (
            <span className="text-text-dim">closed</span>
          )}
        </span>
      ),
      sortValue: (r) => (r.wallet_partial_data ? 2 : r.wallet_is_open ? 1 : 0),
      searchValue: (r) =>
        `${r.wallet_is_open ? 'open' : 'closed'}${r.wallet_partial_data ? ' partial' : ''}`,
    },

    // ── bonding curve ───────────────────────────────────────────────────────
    {
      key: 'w_entry_curve',
      label: 'Entry Curve',
      group: 'wallet_curve',
      width: '88px',
      tooltip:
        'Bonding-curve progress the wallet bought INTO — real pool SOL just before its first buy, as a % of the ~85 SOL graduation line. Its own price impact is backed out, so this is the depth it saw, not the one it left behind. Over 100% means a migrated pool.',
      sortable: true,
      render: (r) => <CurveCell pct={r.wallet_entry_curve_pct} sol={r.wallet_entry_curve_sol} />,
      sortValue: (r) => r.wallet_entry_curve_pct,
      searchValue: () => '',
      filterNumber: (r) => r.wallet_entry_curve_pct,
    },
    {
      key: 'w_exit_curve',
      label: 'Exit Curve',
      group: 'wallet_curve',
      width: '88px',
      tooltip:
        'Bonding-curve progress at the exit — real pool SOL just before the last sell, as a % of the ~85 SOL graduation line.',
      sortable: true,
      render: (r) => <CurveCell pct={r.wallet_exit_curve_pct} sol={r.wallet_exit_curve_sol} />,
      sortValue: (r) => r.wallet_exit_curve_pct,
      searchValue: () => '',
      filterNumber: (r) => r.wallet_exit_curve_pct,
    },
    {
      key: 'w_curve_delta',
      label: 'Curve Gain',
      group: 'wallet_curve',
      width: '88px',
      tooltip:
        'Exit progress minus entry progress: how far the curve advanced while the wallet held — a move measure independent of price. Blank unless both legs are in the window.',
      sortable: true,
      render: (r) => <SignedPct pct={curveDeltaOf(r)} />,
      sortValue: curveDeltaOf,
      searchValue: () => '',
      filterNumber: curveDeltaOf,
    },
  ];

  return cols.map((col) => (HIDDEN_KEYS.has(col.key) ? { ...col, defaultVisible: false } : col));
}
