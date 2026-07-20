// Caps column factory for Rules + Simulate: one visible `conc/total` cell whose
// header offers concurrent / total sort toggles, backed by hidden sort-only
// columns. SSOT so both pages sort the same axes the same way. `0` total means
// unlimited and displays/filters as `∞` (sorts as largest).

import type { ColumnDef, SortValue } from 'components/table/types';
import { MultiSortHeader } from 'components/table/MultiSortHeader';

export type CapsRuleRow = {
  max_concurrent_tokens: number;
  max_total_tokens: number;
};

/** Display + filter text — unlimited total (`0`) is `∞`, matching the cell. */
export function capsDisplayText(r: CapsRuleRow): string {
  return `${r.max_concurrent_tokens}/${r.max_total_tokens || '∞'}`;
}

/** Sort key for total: `0` (unlimited) ranks above any finite cap. */
function totalSortValue(total: number): SortValue {
  return total === 0 ? Number.MAX_SAFE_INTEGER : total;
}

type CapsSortAxis = {
  key: string;
  label: string;
  sortValue: (r: CapsRuleRow) => SortValue;
};

const CAPS_SORT_AXES: CapsSortAxis[] = [
  {
    key: 'caps_conc',
    label: 'conc',
    sortValue: (r) => r.max_concurrent_tokens,
  },
  {
    key: 'caps_total',
    label: 'total',
    sortValue: (r) => totalSortValue(r.max_total_tokens),
  },
];

/**
 * Visible caps cell + hidden per-axis sort columns for a row that holds
 * concurrent/total token caps.
 */
export function buildCapsColumns<R extends CapsRuleRow>(): ColumnDef<R>[] {
  const visible: ColumnDef<R> = {
    key: 'caps',
    label: 'Caps',
    group: 'caps',
    render: (r) => (
      <span className="tabular-nums text-text-dim">{capsDisplayText(r)}</span>
    ),
    searchValue: (r) => capsDisplayText(r),
    filterValue: (r) => capsDisplayText(r),
    // Numeric filter ops (`>2`, `1..5`) apply to concurrent; plain text still
    // matches the displayed `conc/total` (incl. `∞`).
    filterNumber: (r) => r.max_concurrent_tokens,
    renderHeader: (ctx) => (
      <MultiSortHeader title="Caps" axes={CAPS_SORT_AXES} ctx={ctx} />
    ),
  };

  const sortOnly: ColumnDef<R>[] = CAPS_SORT_AXES.map((axis) => ({
    key: axis.key,
    label: axis.label,
    group: 'caps',
    sortOnly: true,
    defaultVisible: false,
    sortable: true,
    render: () => null,
    searchValue: () => '',
    sortValue: (r) => axis.sortValue(r),
  }));

  return [visible, ...sortOnly];
}
