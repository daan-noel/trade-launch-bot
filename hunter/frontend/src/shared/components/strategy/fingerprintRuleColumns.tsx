// Fingerprint column factory for Rules + Simulate: one visible cell (name +
// axis chips) whose header offers per-axis sort toggles, backed by hidden
// sort-only columns. SSOT so both pages sort the same axes the same way.

import { Link } from 'react-router-dom';

import type { ColumnDef, SortValue } from 'components/table/types';
import { MultiSortHeader } from 'components/table/MultiSortHeader';
import { LinkIcon } from 'components/ui/icons';
import {
  IX_LABELS_FILTER_PLACEHOLDER,
  IX_LABELS_FILTER_TITLE,
  isIxLabelJsonFilter,
  ixLabelsMatchFilter,
} from 'lib/ixLabels';
import type { Fingerprint } from 'lib/strategy/types';
import { fingerprintsHref } from 'lib/strategy/nav';

import {
  fingerprintIdentityKey,
  fingerprintParamsCell,
  fingerprintParamsSearchText,
  flowStatusBadge,
} from './FingerprintParamsSummary';

type FpSortAxis = {
  key: string;
  label: string;
  /** Optional native tooltip on the header toggle (the label alone is terse). */
  title?: string;
  sortValue: (fp: Fingerprint | undefined, fingerprintId: string) => SortValue;
};

/** Axes offered in the fingerprint header — labels match the param chips. The
 *  leading `fp` axis sorts by the whole-fingerprint identity so byte-identical
 *  fingerprints (which tie on every single axis) always land adjacent. */
const FP_SORT_AXES: FpSortAxis[] = [
  {
    key: 'fp_id',
    label: 'fp',
    title: 'Sort by the whole fingerprint — groups identical fingerprints together',
    sortValue: (fp, id) => fingerprintIdentityKey(fp, id),
  },
  {
    key: 'fp_name',
    label: 'name',
    sortValue: (fp, id) => fp?.name || id.slice(0, 8),
  },
  {
    key: 'fp_cu_limit',
    label: 'cu_limit',
    sortValue: (fp) => fp?.cu_limit ?? null,
  },
  {
    key: 'fp_cu_price',
    label: 'cu_price',
    sortValue: (fp) => fp?.cu_price ?? null,
  },
  {
    key: 'fp_init',
    label: 'init',
    sortValue: (fp) => fp?.init_buy_lamports ?? null,
  },
  {
    key: 'fp_max',
    label: 'max',
    sortValue: (fp) => fp?.max_cost_lamports ?? null,
  },
  {
    key: 'fp_spend',
    label: 'spend',
    sortValue: (fp) => fp?.spendable_lamports_in ?? null,
  },
  {
    key: 'fp_fs_buy',
    label: 'fs_buy',
    sortValue: (fp) => fp?.first_slot_buy_lamports ?? null,
  },
  {
    key: 'fp_fs_sell',
    label: 'fs_sell',
    sortValue: (fp) => fp?.first_slot_sell_lamports ?? null,
  },
  {
    key: 'fp_ix',
    label: 'ix',
    sortValue: (fp) => fp?.ix_labels?.length ?? null,
  },
  {
    key: 'fp_bkt',
    label: 'bkt',
    sortValue: (fp) => fp?.bucket_size_amount ?? null,
  },
];

export type FingerprintRuleRow = { id: string; fingerprint_id: string };

/**
 * Visible fingerprint cell + hidden per-axis sort columns for a rule-like row
 * that holds `fingerprint_id`. Pass `cellClassName` for same-value tints.
 */
export function buildFingerprintRuleColumns<R extends FingerprintRuleRow>(
  fpById: Map<string, Fingerprint>,
  opts?: { cellClassName?: (row: R) => string | undefined },
): ColumnDef<R>[] {
  const fpOf = (r: R) => fpById.get(r.fingerprint_id);

  const visible: ColumnDef<R> = {
    key: 'fingerprint',
    label: 'Fingerprint',
    group: 'fingerprint',
    render: (r) => {
      const fp = fpOf(r);
      const label = fp?.name || r.fingerprint_id.slice(0, 8);
      return (
        <div className="flex min-w-48 flex-col gap-1">
          <div className="flex items-center justify-center gap-1">
            <span className="font-mono text-[12px] text-text-dim">{label}</span>
            {fp ? flowStatusBadge(fp) : null}
            <Link
              to={fingerprintsHref(r.fingerprint_id)}
              title={`Open fingerprint “${label}”`}
              aria-label={`Open fingerprint ${label}`}
              className="inline-flex shrink-0 rounded p-0.5 text-accent hover:bg-accent/15 hover:text-primary"
              onClick={(e) => e.stopPropagation()}
            >
              <LinkIcon className="h-3.5 w-3.5" />
            </Link>
          </div>
          {fp ? fingerprintParamsCell(fp) : null}
        </div>
      );
    },
    searchValue: (r) => fingerprintParamsSearchText(fpOf(r), r.fingerprint_id),
    // JSON paste → ordered-exact on fp.ix_labels; otherwise substring on the
    // full fingerprint search text (name / axes / pretty labels).
    filterMatch: (r, raw) => {
      if (isIxLabelJsonFilter(raw)) {
        return ixLabelsMatchFilter(fpOf(r)?.ix_labels, raw);
      }
      return fingerprintParamsSearchText(fpOf(r), r.fingerprint_id)
        .toLowerCase()
        .includes(raw.toLowerCase());
    },
    filterPlaceholder: IX_LABELS_FILTER_PLACEHOLDER,
    filterTitle: `${IX_LABELS_FILTER_TITLE}. Non-JSON text still matches name/axes/labels as substring.`,
    cellClassName: opts?.cellClassName,
    renderHeader: (ctx) => (
      <MultiSortHeader title="Fingerprint" axes={FP_SORT_AXES} ctx={ctx} />
    ),
  };

  const sortOnly: ColumnDef<R>[] = FP_SORT_AXES.map((axis) => ({
    key: axis.key,
    label: axis.label,
    group: 'fingerprint',
    sortOnly: true,
    defaultVisible: false,
    sortable: true,
    render: () => null,
    searchValue: () => '',
    sortValue: (r) => axis.sortValue(fpOf(r), r.fingerprint_id),
  }));

  return [visible, ...sortOnly];
}
