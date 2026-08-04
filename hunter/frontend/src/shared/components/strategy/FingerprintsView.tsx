import { useCallback, useMemo, useState, type ReactNode } from 'react';
import { Link } from 'react-router-dom';

import { DataTable } from 'components/table/DataTable';
import type { ColumnDef } from 'components/table/types';
import { IconButton } from 'components/ui/IconButton';
import { IconButtonGroup } from 'components/ui/IconButtonGroup';
import { DuplicateIcon, EditIcon, LinkIcon, PlusIcon, TrashIcon } from 'components/ui/icons';
import { Badge } from 'components/ui/Badge';
import { Modal } from 'components/ui/Modal';
import { EmptyState } from 'components/ui/EmptyState';
import { PageHeader } from 'components/ui/PageHeader';
import { IxLabelsDisplay } from 'components/ui/IxLabelsDisplay';
import { FingerprintForm } from './FingerprintForm';
import { ruleParamsCell } from './RuleParamsSummary';
import { useSelectionSearchParam } from 'hooks/useSelectionSearchParam';
import { apiErrorMessage } from 'store/baseApi';
import {
  useGetFingerprintsQuery,
  useGetStrategyRulesQuery,
  useCreateFingerprintMutation,
  useUpdateFingerprintMutation,
  useDeleteFingerprintMutation,
} from 'store/sharedEndpoints';
import {
  configuredIxLabels,
  formatIxLabelsText,
  IX_LABELS_FILTER_PLACEHOLDER,
  IX_LABELS_FILTER_TITLE,
  ixLabelsMatchFilter,
} from 'lib/ixLabels';
import { computeSameValueCellClasses } from 'lib/sameValueCellColors';
import { volumeIxPatternsFromConfig } from 'lib/strategy/registry';
import { flowDiscoveryHref, rulesHref, STRATEGY_PARAMS } from 'lib/strategy/nav';
import {
  formatBucketWidth,
  lamportsToSol,
  type Fingerprint,
  type FingerprintDraft,
  type StrategyRule,
} from 'lib/strategy/types';
import { tidySolDecimal } from 'utils/format';

function dash(): ReactNode {
  return <span className="text-text-dim">—</span>;
}

function solCell(lamports: number | null): ReactNode {
  const s = lamportsToSol(lamports);
  if (s == null) return dash();
  return <span className="font-mono tabular-nums">{s}◎</span>;
}

function intCell(n: number | null): ReactNode {
  if (n == null) return dash();
  return <span className="font-mono tabular-nums">{n}</span>;
}

function solKey(lamports: number | null): string | null {
  const s = lamportsToSol(lamports);
  return s == null ? null : String(s);
}

/** Param columns that tint when ≥2 fingerprints share the same value. */
const COLOR_COLS: {
  key: string;
  valueOf: (r: Fingerprint) => string | null;
}[] = [
  { key: 'cu_limit', valueOf: (r) => (r.cu_limit == null ? null : String(r.cu_limit)) },
  { key: 'cu_price', valueOf: (r) => (r.cu_price == null ? null : String(r.cu_price)) },
  { key: 'init_buy', valueOf: (r) => solKey(r.init_buy_lamports) },
  { key: 'max_cost', valueOf: (r) => solKey(r.max_cost_lamports) },
  { key: 'spendable', valueOf: (r) => solKey(r.spendable_lamports_in) },
  { key: 'fs_buy', valueOf: (r) => solKey(r.first_slot_buy_lamports) },
  { key: 'fs_sell', valueOf: (r) => solKey(r.first_slot_sell_lamports) },
  {
    key: 'ix_count',
    valueOf: (r) => {
      const labels = configuredIxLabels(r.ix_labels);
      return labels ? String(labels.length) : null;
    },
  },
  {
    key: 'ix_labels',
    valueOf: (r) => {
      const labels = configuredIxLabels(r.ix_labels);
      return labels ? formatIxLabelsText(labels) : null;
    },
  },
  { key: 'bucket', valueOf: (r) => formatBucketWidth(r.bucket_size_amount) },
];

/** Expanded row detail: rules that reference this fingerprint, with the same
 *  params summary as the Rules table so you can tell them apart at a glance.
 *  Each card navigates to Rules with that rule selected (`?rule=`). Same-tab
 *  by default; Ctrl/middle-click still opens a new tab. */
function FingerprintUsedByDetail({ rules }: { rules: StrategyRule[] }) {
  if (rules.length === 0) {
    return (
      <EmptyState compact message="Not used by any rules — safe to delete." />
    );
  }
  return (
    <div className="flex flex-col gap-2.5">
      <div className="flex items-baseline justify-between gap-3">
        <p className="text-[11px] font-semibold uppercase tracking-wider text-primary">
          Used by {rules.length} rule{rules.length === 1 ? '' : 's'}
        </p>
        <Link
          to={rulesHref()}
          className="text-[11px] text-accent hover:text-primary hover:underline"
        >
          Open Rules →
        </Link>
      </div>
      <ul className="grid gap-2 sm:grid-cols-2">
        {rules.map((r) => (
          <li key={r.id}>
            <Link
              to={rulesHref(r.id)}
              className="flex flex-col gap-2 rounded-md border border-info/25 bg-info/8 px-3 py-2.5 text-left transition-colors hover:border-accent/40 hover:bg-info/14"
              title={`Open rule “${r.rule_name}”`}
            >
              <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
                <span className="min-w-0 flex-1 truncate text-[13px] font-semibold text-text">
                  {r.rule_name}
                </span>
                <LinkIcon className="h-3.5 w-3.5 shrink-0 text-accent" />
                <Badge variant={r.trade_mode === 'real' ? 'warning' : 'info'} size="sm">
                  {r.trade_mode}
                </Badge>
                <Badge
                  variant={!r.is_enabled ? 'danger' : r.is_active ? 'success' : 'neutral'}
                  size="sm"
                >
                  {!r.is_enabled ? 'Disabled' : r.is_active ? 'Active' : 'Idle'}
                </Badge>
              </div>
              <div className="flex flex-wrap items-center gap-x-3 gap-y-0.5 text-[11px] tabular-nums text-text-dim">
                <span>buy {lamportsToSol(r.buy_amount_lamports)}◎</span>
                <span>
                  caps {r.max_concurrent_tokens}/{r.max_total_tokens || '∞'}
                </span>
              </div>
              {ruleParamsCell(r.params)}
            </Link>
          </li>
        ))}
      </ul>
    </div>
  );
}

/**
 * Fingerprint library, shared by the live and lab apps: the match specs rules
 * reference. List (used-by count) + create/edit form (SOL inputs, lamports at
 * the API boundary) + used-by-guarded delete. Selecting a row expands the
 * rules that reference it.
 */
export function FingerprintsView({
  linkToFlowDiscovery = false,
}: {
  /** Lab-only: show a per-row deep-link into Flow discovery scoped to that
   *  fingerprint. Off in the live app (Flow discovery is a lab surface). */
  linkToFlowDiscovery?: boolean;
} = {}) {
  const { data: fps = [], isLoading } = useGetFingerprintsQuery();
  const { data: rules = [] } = useGetStrategyRulesQuery();
  const [createFp, { isLoading: creating }] = useCreateFingerprintMutation();
  const [updateFp, { isLoading: updating }] = useUpdateFingerprintMutation();
  const [deleteFp] = useDeleteFingerprintMutation();

  const [selectedKey, setSelectedKey] = useSelectionSearchParam(STRATEGY_PARAMS.fingerprint);
  const [editing, setEditing] = useState<Fingerprint | 'new' | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const rulesByFp = useMemo(() => {
    const map = new Map<string, StrategyRule[]>();
    for (const r of rules) {
      const list = map.get(r.fingerprint_id);
      if (list) list.push(r);
      else map.set(r.fingerprint_id, [r]);
    }
    return map;
  }, [rules]);

  const rowDetail = useCallback(
    (fp: Fingerprint) => (
      <FingerprintUsedByDetail rules={rulesByFp.get(fp.id) ?? []} />
    ),
    [rulesByFp],
  );

  const valueColors = useMemo(
    () => computeSameValueCellClasses(fps, (r) => r.id, COLOR_COLS),
    [fps],
  );

  const cellTint =
    (colKey: string) =>
    (row: Fingerprint): string | undefined =>
      valueColors.get(`${row.id}\0${colKey}`);

  const editingId =
    editing && editing !== 'new' && editing.id ? editing.id : undefined;

  const submit = async (draft: FingerprintDraft) => {
    setErr(null);
    try {
      if (editingId) await updateFp({ id: editingId, body: draft }).unwrap();
      else await createFp(draft).unwrap();
      setEditing(null);
    } catch (e) {
      setErr(apiErrorMessage(e as never) ?? 'Save failed');
    }
  };

  const duplicate = (fp: Fingerprint) => {
    setErr(null);
    setEditing({
      ...fp,
      id: '',
      name: `${fp.name || fp.id.slice(0, 8)} copy`,
      used_by: 0,
    });
  };

  const remove = async (fp: Fingerprint) => {
    if (fp.used_by && fp.used_by > 0) return;
    if (!window.confirm(`Delete fingerprint "${fp.name || fp.id.slice(0, 8)}"?`)) return;
    try {
      await deleteFp(fp.id).unwrap();
    } catch (e) {
      setErr(apiErrorMessage(e as never) ?? 'Delete failed');
    }
  };

  const columns: ColumnDef<Fingerprint>[] = useMemo(
    () => [
      {
        key: 'name',
        label: 'Name',
        group: 'name',
        render: (r) => (
          <div className="flex items-center gap-1">
            <span className="font-medium text-text">{r.name || r.id.slice(0, 8)}</span>
            {linkToFlowDiscovery && (
              <Link
                to={flowDiscoveryHref(r.id)}
                title={`Flow discovery for “${r.name || r.id.slice(0, 8)}”`}
                aria-label={`Flow discovery for ${r.name || r.id.slice(0, 8)}`}
                className="inline-flex shrink-0 rounded p-0.5 text-accent hover:bg-accent/15 hover:text-primary"
                onClick={(e) => e.stopPropagation()}
              >
                <LinkIcon className="h-3.5 w-3.5" />
              </Link>
            )}
          </div>
        ),
        searchValue: (r) => r.name,
      },
      {
        key: 'cu_limit',
        label: 'cu_limit',
        group: 'cu',
        render: (r) => intCell(r.cu_limit),
        searchValue: (r) => String(r.cu_limit ?? ''),
        sortValue: (r) => r.cu_limit,
        filterNumber: (r) => r.cu_limit,
        sortable: true,
        cellClassName: cellTint('cu_limit'),
      },
      {
        key: 'cu_price',
        label: 'cu_price',
        group: 'cu',
        render: (r) => intCell(r.cu_price),
        searchValue: (r) => String(r.cu_price ?? ''),
        sortValue: (r) => r.cu_price,
        filterNumber: (r) => r.cu_price,
        sortable: true,
        cellClassName: cellTint('cu_price'),
      },
      {
        key: 'init_buy',
        label: 'init_buy',
        group: 'init_buy',
        render: (r) => solCell(r.init_buy_lamports),
        searchValue: (r) => String(lamportsToSol(r.init_buy_lamports) ?? ''),
        sortValue: (r) => r.init_buy_lamports,
        filterNumber: (r) => lamportsToSol(r.init_buy_lamports),
        sortable: true,
        cellClassName: cellTint('init_buy'),
      },
      {
        key: 'max_cost',
        label: 'max_cost',
        group: 'init_buy',
        render: (r) => solCell(r.max_cost_lamports),
        searchValue: (r) => String(lamportsToSol(r.max_cost_lamports) ?? ''),
        sortValue: (r) => r.max_cost_lamports,
        filterNumber: (r) => lamportsToSol(r.max_cost_lamports),
        sortable: true,
        cellClassName: cellTint('max_cost'),
      },
      {
        key: 'spendable',
        label: 'spendable',
        group: 'init_buy',
        render: (r) => solCell(r.spendable_lamports_in),
        searchValue: (r) => String(lamportsToSol(r.spendable_lamports_in) ?? ''),
        sortValue: (r) => r.spendable_lamports_in,
        filterNumber: (r) => lamportsToSol(r.spendable_lamports_in),
        sortable: true,
        cellClassName: cellTint('spendable'),
      },
      {
        key: 'fs_buy',
        label: 'fs_buy',
        group: 'fs',
        render: (r) => solCell(r.first_slot_buy_lamports),
        searchValue: (r) => String(lamportsToSol(r.first_slot_buy_lamports) ?? ''),
        sortValue: (r) => r.first_slot_buy_lamports,
        filterNumber: (r) => lamportsToSol(r.first_slot_buy_lamports),
        sortable: true,
        cellClassName: cellTint('fs_buy'),
      },
      {
        key: 'fs_sell',
        label: 'fs_sell',
        group: 'fs',
        render: (r) => solCell(r.first_slot_sell_lamports),
        searchValue: (r) => String(lamportsToSol(r.first_slot_sell_lamports) ?? ''),
        sortValue: (r) => r.first_slot_sell_lamports,
        filterNumber: (r) => lamportsToSol(r.first_slot_sell_lamports),
        sortable: true,
        cellClassName: cellTint('fs_sell'),
      },
      {
        key: 'ix_count',
        label: 'ix count',
        group: 'ix',
        render: (r) => {
          const labels = configuredIxLabels(r.ix_labels);
          if (!labels) return dash();
          return <span className="font-mono tabular-nums">{labels.length}</span>;
        },
        searchValue: (r) => String(configuredIxLabels(r.ix_labels)?.length ?? ''),
        sortValue: (r) => configuredIxLabels(r.ix_labels)?.length ?? null,
        filterNumber: (r) => configuredIxLabels(r.ix_labels)?.length ?? null,
        sortable: true,
        cellClassName: cellTint('ix_count'),
      },
      {
        key: 'ix_labels',
        label: 'ix_labels',
        group: 'ix',
        width: '220px',
        render: (r) => {
          const labels = configuredIxLabels(r.ix_labels);
          return labels ? <IxLabelsDisplay labels={labels} copyJson /> : dash();
        },
        searchValue: (r) => {
          const labels = configuredIxLabels(r.ix_labels);
          return labels ? formatIxLabelsText(labels) : '';
        },
        filterMatch: (r, raw) => ixLabelsMatchFilter(r.ix_labels, raw),
        filterPlaceholder: IX_LABELS_FILTER_PLACEHOLDER,
        filterTitle: IX_LABELS_FILTER_TITLE,
        cellClassName: cellTint('ix_labels'),
      },
      {
        key: 'flow_patterns',
        label: 'flow patterns',
        group: 'ix',
        render: (r) => {
          const n = volumeIxPatternsFromConfig(r.metric_config).length;
          if (n === 0) return dash();
          return (
            <Badge variant="primary" className="font-mono tabular-nums">
              {n}
            </Badge>
          );
        },
        searchValue: (r) => String(volumeIxPatternsFromConfig(r.metric_config).length),
        sortValue: (r) => volumeIxPatternsFromConfig(r.metric_config).length,
        filterNumber: (r) => volumeIxPatternsFromConfig(r.metric_config).length || null,
        sortable: true,
      },
      {
        key: 'bucket',
        label: 'Bucket',
        group: 'bucket',
        render: (r) => (
          <span className="font-mono tabular-nums">
            {formatBucketWidth(r.bucket_size_amount)}{r.bucket_size_amount == null ? "" : "◎"}
          </span>
        ),
        searchValue: (r) => formatBucketWidth(r.bucket_size_amount),
        // Exact rows have no width — sort/filter them as null rather than coercing
        // to 0, which would rank them as the *finest* bucket instead of no bucket.
        sortValue: (r) =>
          r.bucket_size_amount == null ? null : tidySolDecimal(r.bucket_size_amount),
        filterNumber: (r) =>
          r.bucket_size_amount == null ? null : tidySolDecimal(r.bucket_size_amount),
        sortable: true,
        cellClassName: cellTint('bucket'),
      },
      {
        key: 'used_by',
        label: 'Used by',
        group: 'used',
        render: (r) => (
          <Badge className="text-lg" variant={r.used_by ? 'info' : 'neutral'}>
            {r.used_by ?? 0}
          </Badge>
        ),
        searchValue: (r) => String(r.used_by ?? 0),
        sortValue: (r) => r.used_by ?? 0,
        filterNumber: (r) => r.used_by ?? 0,
        sortable: true,
      },
    ],
    [valueColors, linkToFlowDiscovery],
  );

  const editingRules = editingId ? (rulesByFp.get(editingId) ?? []) : [];

  return (
    <div className="flex flex-col gap-3 p-4">
      <PageHeader
        className="mb-0"
        title="Fingerprints"
        description="Match specs · select a row to see which rules use it"
        actions={
          <IconButton
            variant="success"
            size="lg"
            label="New fingerprint"
            title="New fingerprint"
            onClick={() => setEditing('new')}
          >
            <PlusIcon />
          </IconButton>
        }
      />
      {err && <p className="text-xs text-red">{err}</p>}
      <DataTable
        columns={columns}
        rows={fps}
        rowKey={(r) => r.id}
        loading={isLoading}
        searchable
        colFilters
        colToggle
        tableId="fingerprints-v2"
        pinnable
        emptyMessage="No fingerprints yet — create one to start authoring rules."
        selectedKey={selectedKey}
        onSelect={setSelectedKey}
        rowDetail={rowDetail}
        rowActions={(r) => (
          <IconButtonGroup>
            <IconButton
              variant="accent"
              size="md"
              title="Edit"
              aria-label="Edit"
              onClick={() => setEditing(r)}
            >
              <EditIcon />
            </IconButton>
            <IconButton
              variant="ghost"
              size="md"
              title="Duplicate"
              aria-label="Duplicate"
              onClick={() => duplicate(r)}
            >
              <DuplicateIcon />
            </IconButton>
            <IconButton
              variant="danger"
              size="md"
              disabled={Boolean(r.used_by && r.used_by > 0)}
              title={r.used_by ? `Used by ${r.used_by} rule(s)` : 'Delete'}
              aria-label={r.used_by ? `Used by ${r.used_by} rule(s)` : 'Delete'}
              onClick={() => remove(r)}
            >
              <TrashIcon />
            </IconButton>
          </IconButtonGroup>
        )}
      />
      <Modal
        title={editingId ? 'Edit fingerprint' : 'New fingerprint'}
        open={editing !== null}
        onClose={() => setEditing(null)}
      >
        {editing !== null && (
          <div className="flex flex-col gap-3">
            {editingId && <FingerprintUsedByDetail rules={editingRules} />}
            <FingerprintForm
              initial={editing === 'new' ? undefined : editing}
              onSubmit={submit}
              onCancel={() => setEditing(null)}
              submitting={creating || updating}
              error={err}
            />
          </div>
        )}
      </Modal>
    </div>
  );
}
