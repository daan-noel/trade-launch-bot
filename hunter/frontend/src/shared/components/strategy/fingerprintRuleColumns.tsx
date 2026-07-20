// Fingerprint column factory for Rules + Simulate: one visible cell (name +
// axis chips) whose header offers per-axis sort toggles, backed by hidden
// sort-only columns. SSOT so both pages sort the same axes the same way.

import type { ColumnDef, SortValue } from 'components/table/types';
import { MultiSortHeader } from 'components/table/MultiSortHeader';
import type { Fingerprint } from 'lib/strategy/types';

import {
  fingerprintParamsCell,
  fingerprintParamsSearchText,
} from './FingerprintParamsSummary';

type FpSortAxis = {
  key: string;
  label: string;
  sortValue: (fp: Fingerprint | undefined, fingerprintId: string) => SortValue;
};

/** Axes offered in the fingerprint header — labels match the param chips. */
const FP_SORT_AXES: FpSortAxis[] = [
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
      return (
        <div className="flex min-w-48 flex-col gap-1">
          <span className="font-mono text-[12px] text-text-dim">
            {fp?.name || r.fingerprint_id.slice(0, 8)}
          </span>
          {fp ? fingerprintParamsCell(fp) : null}
        </div>
      );
    },
    searchValue: (r) => fingerprintParamsSearchText(fpOf(r), r.fingerprint_id),
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
