// Params column factory for Rules + Simulate: one visible chip cell whose
// header offers TP / SL / in / out sort toggles, backed by hidden sort-only
// columns. SSOT so both pages sort the same axes the same way.

import type { ColumnDef, SortValue } from 'components/table/types';
import { MultiSortHeader } from 'components/table/MultiSortHeader';

import {
  ruleParamsCell,
  ruleParamsSearchText,
  ruleParamsSortParts,
} from './RuleParamsSummary';

type ParamsSortAxis = {
  key: string;
  label: string;
  sortValue: (parts: ReturnType<typeof ruleParamsSortParts>) => SortValue;
};

const PARAMS_SORT_AXES: ParamsSortAxis[] = [
  { key: 'params_tp', label: 'tp', sortValue: (p) => p.take_profit },
  { key: 'params_sl', label: 'sl', sortValue: (p) => p.stop_loss },
  { key: 'params_in', label: 'in', sortValue: (p) => p.entry_count },
  { key: 'params_out', label: 'out', sortValue: (p) => p.exit_count },
];

export type RuleParamsRow = { params: unknown };

/**
 * Visible params cell + hidden per-axis sort columns for a row that holds
 * `params` (strategy rule / simulate row).
 */
export function buildRuleParamsColumns<R extends RuleParamsRow>(): ColumnDef<R>[] {
  const visible: ColumnDef<R> = {
    key: 'params',
    label: 'Params',
    group: 'params',
    render: (r) => ruleParamsCell(r.params),
    searchValue: (r) => ruleParamsSearchText(r.params),
    renderHeader: (ctx) => (
      <MultiSortHeader title="Params" axes={PARAMS_SORT_AXES} ctx={ctx} />
    ),
  };

  const sortOnly: ColumnDef<R>[] = PARAMS_SORT_AXES.map((axis) => ({
    key: axis.key,
    label: axis.label,
    group: 'params',
    sortOnly: true,
    defaultVisible: false,
    sortable: true,
    render: () => null,
    searchValue: () => '',
    sortValue: (r) => axis.sortValue(ruleParamsSortParts(r.params)),
  }));

  return [visible, ...sortOnly];
}
