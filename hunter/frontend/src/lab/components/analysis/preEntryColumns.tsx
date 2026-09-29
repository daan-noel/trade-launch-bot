import type { ColumnDef } from 'components/table/types';
import { Badge } from 'components/ui/Badge';
import { IxLabelsDisplay } from 'components/ui/IxLabelsDisplay';
import { IX_ABBREV_LEGEND } from 'lib/ixLabels';
import { formatDecimalTrim } from 'utils/format';
import {
  formatLag,
  lagSortValue,
  slotsText,
  UNKNOWN_HINT,
  type PreEntryVerdict,
} from '@lab/lib/preEntryProbeTypes';

/**
 * **Pre-entry** columns — for each row (a token on Trader Analysis, one buy on
 * Entry Context), whether a structure from the flow lens landed on the tape
 * before this trader entered, how far ahead of him it was, and how often the same
 * thing happens in the window before that one. One set of columns for every page
 * that asks, so the verdict reads the same everywhere.
 *
 * Columns rather than a hard row filter, on purpose. Narrowing runs through the
 * table's own filter row, so the unfiltered denominator and the CONTROL count
 * stay on screen while the filter is on: a filter alone can only ever show
 * confirmations, and a structure a crowd shares sits before everything.
 *
 * The verdict is looked up per row (`at`), not read off a row field — the row is
 * the server's shape and this is a question asked about it, re-asked whenever
 * the window or the lens narrowing changes.
 *
 * Spliced in beside `walletTokenColumns()` only while the probe is on, and never
 * added to the shared `tokenColumns()` SSOT.
 */

const STATE_LABEL = {
  matched: 'Before',
  'no-match': 'Absent',
  unknown: 'Unknown',
} as const;

const STATE_VARIANT = {
  // The finding gets the loudest chip; an unanswerable row is dim, never green.
  matched: 'accent',
  'no-match': 'neutral',
  unknown: 'warning',
} as const;

/** Why this row reads the way it does, spelled out on hover. */
function stateTitle(v: PreEntryVerdict, last: string, earlier: string): string {
  if (v.state === 'unknown') {
    return v.unknown_reason
      ? `Unknown — ${UNKNOWN_HINT[v.unknown_reason]}`
      : 'Unknown';
  }
  const hits = `${v.hits} target transaction${v.hits === 1 ? '' : 's'} (${formatDecimalTrim(v.sol, 3)} SOL) ${last}`;
  const control = `${v.control_hits} ${earlier}`;
  return v.state === 'matched'
    ? `Before: the target traded right before his buy.\n${hits}; ${control}.`
    : `Absent: not enough of the target right before his buy.\n${hits}; ${control}.`;
}

export function preEntryColumns<R>(
  at: (row: R) => PreEntryVerdict | undefined,
  /** The probe window, for the tooltips to spell out. */
  windowSlots: number,
): ColumnDef<R>[] {
  const n = windowSlots;
  const last = `in the last ${slotsText(n)} before his buy`;
  const earlier = `from ${2 * n} to ${n} slots before his buy`;
  return [
    {
      key: 'pe_state',
      label: 'Pre-entry',
      group: 'pre_entry',
      width: '92px',
      tooltip:
        `Did the target trade ${last}?\n` +
        'Before = yes, at least the probe minimums (Min hits, Min SOL).\n' +
        'Absent = no.\n' +
        'Unknown = the stored data cannot tell (never counted as Absent). Hover a cell for its counts.',
      sortable: true,
      render: (r) => {
        const v = at(r);
        if (!v) return <span className="text-text-dim">-</span>;
        return (
          <span title={stateTitle(v, last, earlier)}>
            <Badge variant={STATE_VARIANT[v.state]} size="sm">
              {STATE_LABEL[v.state]}
            </Badge>
          </span>
        );
      },
      // Matched first under a descending sort — the rows the filter is for.
      sortValue: (r) => {
        const s = at(r)?.state;
        return s === 'matched' ? 2 : s === 'unknown' ? 1 : s === 'no-match' ? 0 : null;
      },
      searchValue: (r) => (at(r) ? STATE_LABEL[at(r)!.state] : ''),
      filterOptions: [
        { value: 'matched', label: 'Before' },
        { value: 'no-match', label: 'Absent' },
        { value: 'unknown', label: 'Unknown' },
      ],
      filterOptionValue: (r) => at(r)?.state ?? '',
    },
    {
      key: 'pe_lag',
      label: 'Lag',
      group: 'pre_entry',
      width: '96px',
      tooltip:
        'How long before his buy the closest target transaction traded.\n' +
        `Example: ${slotsText(3)} = it traded that long before him.\n` +
        "'same slot' = in his own slot: it landed with him, it did not lead him.",
      sortable: true,
      render: (r) => {
        const v = at(r);
        if (!v || v.nearest_lag_slots == null) return <span className="text-text-dim">-</span>;
        const sameSlot = v.nearest_lag_slots === 0;
        return (
          <span
            className={sameSlot ? 'text-warning' : undefined}
            title={
              sameSlot
                ? 'The closest one is in his own slot: it did not lead him'
                : `${v.nearest_lag_slots} slots before his entry`
            }
          >
            {formatLag(v)}
          </span>
        );
      },
      sortValue: (r) => lagSortValue(at(r)),
      searchValue: (r) => {
        const v = at(r);
        return v ? formatLag(v) : '';
      },
      filterNumber: (r) => lagSortValue(at(r)),
    },
    {
      key: 'pe_hits',
      label: 'Hits',
      group: 'pre_entry',
      width: '70px',
      tooltip:
        `How many target transactions traded ${last}. Hover a number for their SOL.\n` +
        'Example: 5 = the target bought or sold 5 times right before him.',
      sortable: true,
      render: (r) => {
        const v = at(r);
        if (!v) return <span className="text-text-dim">-</span>;
        return (
          <span title={`${formatDecimalTrim(v.sol, 3)} SOL`}>
            {v.hits || '-'}
          </span>
        );
      },
      sortValue: (r) => at(r)?.hits ?? null,
      searchValue: () => '',
      filterNumber: (r) => at(r)?.hits ?? null,
    },
    {
      key: 'pe_sol',
      label: 'Hit SOL',
      group: 'pre_entry',
      width: '80px',
      tooltip: `SOL the target's transactions moved ${last}.`,
      sortable: true,
      render: (r) => {
        const v = at(r);
        return v && v.hits > 0 ? `◎${formatDecimalTrim(v.sol, 3)}` : <span className="text-text-dim">-</span>;
      },
      sortValue: (r) => at(r)?.sol ?? null,
      searchValue: () => '',
      filterNumber: (r) => at(r)?.sol ?? null,
    },
    {
      key: 'pe_unit',
      label: 'Matched',
      group: 'pre_entry',
      width: '200px',
      tooltip: `The ix structure of the target transaction closest to his buy, abbreviated. Hover a cell for the full list.

${IX_ABBREV_LEGEND}`,
      sortable: true,
      render: (r) => {
        const v = at(r);
        const unit = v?.matched_unit;
        if (v?.matched_labels?.length) {
          return <IxLabelsDisplay labels={v.matched_labels} compact copyJson />;
        }
        return unit ? (
          <span className="block truncate" title={unit}>
            {unit}
          </span>
        ) : (
          <span className="text-text-dim">-</span>
        );
      },
      sortValue: (r) => at(r)?.matched_unit ?? null,
      searchValue: (r) => at(r)?.matched_unit ?? '',
    },
    {
      key: 'pe_control',
      label: 'Earlier',
      group: 'pre_entry',
      width: '80px',
      tooltip:
        `Hits, but one step earlier: target transactions ${earlier}.\n` +
        'It answers: is the target always there, or did it show up for his buy?\n' +
        'Example: Hits 5, Earlier 0 = it showed up right before him. Hits 5, Earlier 5 = it is always there.\n' +
        'Orange = the earlier stretch alone would also count as Before.',
      sortable: true,
      render: (r) => {
        const v = at(r);
        if (!v) return <span className="text-text-dim">-</span>;
        return (
          <span
            className={v.control_matched ? 'text-warning' : undefined}
            title={
              v.control_matched
                ? `The earlier stretch alone would also count as Before (${formatDecimalTrim(v.control_sol, 3)} SOL): the target is not tied to his buy`
                : `${formatDecimalTrim(v.control_sol, 3)} SOL ${earlier}`
            }
          >
            {v.control_hits || '-'}
          </span>
        );
      },
      sortValue: (r) => at(r)?.control_hits ?? null,
      searchValue: () => '',
      filterNumber: (r) => at(r)?.control_hits ?? null,
    },
  ];
}
