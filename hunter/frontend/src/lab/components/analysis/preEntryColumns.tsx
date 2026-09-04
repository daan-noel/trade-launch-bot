import type { ColumnDef } from 'components/table/types';
import { Badge } from 'components/ui/Badge';
import { formatDecimalTrim } from 'utils/format';
import type { TraderTokenRow } from 'types';
import {
  formatLag,
  lagSortValue,
  UNKNOWN_HINT,
  type PreEntryVerdict,
} from '@lab/lib/preEntryProbeTypes';

/**
 * Trader Analysis **pre-entry** columns — for each token, whether a structure
 * from the flow lens landed on the tape before this trader entered, how far
 * ahead of him it was, and how often the same thing happens in the window
 * before that one.
 *
 * Columns rather than a hard row filter, on purpose. Narrowing runs through the
 * table's own filter row, so the unfiltered denominator and the CONTROL count
 * stay on screen while the filter is on: a filter alone can only ever show
 * confirmations, and a structure a crowd shares sits before everything.
 *
 * The verdicts arrive as a per-mint map, not as row fields — `TraderTokenRow` is
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
function stateTitle(v: PreEntryVerdict): string {
  if (v.state === 'unknown') {
    return v.unknown_reason
      ? `Unknown — ${UNKNOWN_HINT[v.unknown_reason]}`
      : 'Unknown';
  }
  const hits = `${v.hits} matching print${v.hits === 1 ? '' : 's'} (${formatDecimalTrim(v.sol, 3)} SOL)`;
  const control = `${v.control_hits} in the control window before it`;
  return v.state === 'matched'
    ? `A lens structure landed ahead of the entry: ${hits}; ${control}`
    : `No lens structure cleared the thresholds before the entry: ${hits}; ${control}`;
}

export function preEntryColumns(
  verdicts: ReadonlyMap<string, PreEntryVerdict>,
): ColumnDef<TraderTokenRow>[] {
  const at = (r: TraderTokenRow) => verdicts.get(r.mint_address);

  return [
    {
      key: 'pe_state',
      label: 'Pre-entry',
      group: 'pre_entry',
      width: '92px',
      tooltip:
        'Did a lens structure land on the tape before this trader entered? Unknown = the row cannot be answered (no buy leg, tape past retention, or fee pins with no fee readings) — never counted as absent.',
      sortable: true,
      render: (r) => {
        const v = at(r);
        if (!v) return <span className="text-text-dim">-</span>;
        return (
          <span title={stateTitle(v)}>
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
        "How far ahead of the entry the NEAREST match landed. 'same slot' means it did not lead the entry at all — at a seat that reads the tape a slot late, that is co-arrival, not a trigger.",
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
                ? 'The nearest match is in the trader’s own slot — nothing led his entry here'
                : `${v.nearest_lag_slots} slots ahead of the entry`
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
        'Matching transactions in the window before the entry (legs collapsed per tx), and their Σ SOL on hover.',
      sortable: true,
      render: (r) => {
        const v = at(r);
        if (!v) return <span className="text-text-dim">-</span>;
        return (
          <span title={`${formatDecimalTrim(v.sol, 3)} SOL of matching prints`}>
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
      tooltip: 'Σ SOL of the matching prints in the window before the entry.',
      sortable: true,
      render: (r) => {
        const v = at(r);
        return v && v.hits > 0 ? formatDecimalTrim(v.sol, 3) : <span className="text-text-dim">-</span>;
      },
      sortValue: (r) => at(r)?.sol ?? null,
      searchValue: () => '',
      filterNumber: (r) => at(r)?.sol ?? null,
    },
    {
      key: 'pe_unit',
      label: 'Matched',
      group: 'pre_entry',
      width: '140px',
      tooltip:
        'Which unit of the lens matched nearest the entry: the grain id, or the exact row’s group.',
      sortable: true,
      render: (r) => {
        const unit = at(r)?.matched_unit;
        return unit ? (
          <span className="truncate" title={unit}>
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
      label: 'Control',
      group: 'pre_entry',
      width: '80px',
      tooltip:
        'Matching prints in the window one W EARLIER — the same question asked where the entry is not. A count as high as Hits means the structure sits before everything on this tape, not before his entries.',
      sortable: true,
      render: (r) => {
        const v = at(r);
        if (!v) return <span className="text-text-dim">-</span>;
        return (
          <span
            className={v.control_matched ? 'text-warning' : undefined}
            title={
              v.control_matched
                ? `The control window clears the same thresholds (${formatDecimalTrim(v.control_sol, 3)} SOL) — this structure is not specific to the entry`
                : `${formatDecimalTrim(v.control_sol, 3)} SOL in the control window`
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
