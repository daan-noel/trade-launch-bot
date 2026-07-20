import { useCallback, useMemo, useState, type ReactNode } from 'react';
import { Link } from 'react-router-dom';

import { DataTable } from 'components/table/DataTable';
import type { ColumnDef } from 'components/table/types';
import { IconButton } from 'components/ui/IconButton';
import { IconButtonGroup } from 'components/ui/IconButtonGroup';
import { EditIcon, PlusIcon, TrashIcon } from 'components/ui/icons';
import { Badge } from 'components/ui/Badge';
import { Modal } from 'components/ui/Modal';
import { IxLabelsDisplay } from 'components/ui/IxLabelsDisplay';
import { FingerprintForm } from './FingerprintForm';
import { apiErrorMessage } from 'store/baseApi';
import {
  useGetFingerprintsQuery,
  useGetStrategyRulesQuery,
  useCreateFingerprintMutation,
  useUpdateFingerprintMutation,
  useDeleteFingerprintMutation,
} from 'store/sharedEndpoints';
import { formatIxLabelsText } from 'lib/ixLabels';
import { computeSameValueCellClasses } from 'lib/sameValueCellColors';
import { volumeIxPatternsFromConfig } from 'lib/strategy/registry';
import {
  lamportsToSol,
  type Fingerprint,
  type FingerprintDraft,
  type StrategyRule,
} from 'lib/strategy/types';
import { formatDecimalTrim, tidySolDecimal } from 'utils/format';

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
    valueOf: (r) => (r.ix_labels?.length ? String(r.ix_labels.length) : null),
  },
  {
    key: 'ix_labels',
    valueOf: (r) => (r.ix_labels?.length ? formatIxLabelsText(r.ix_labels) : null),
  },
  { key: 'bucket', valueOf: (r) => formatDecimalTrim(tidySolDecimal(r.bucket_size_amount), 6) },
];

/** Expanded row detail: full rule list with status / mode / buy size. */
function FingerprintUsedByDetail({ rules }: { rules: StrategyRule[] }) {
  if (rules.length === 0) {
    return (
      <div className="rounded-md border border-dashed border-white/12 bg-white/2 px-3 py-2.5">
        <p className="text-[12px] text-text-dim">
          Not used by any rules — safe to delete.
        </p>
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-2.5">
      <div className="flex items-baseline justify-between gap-3">
        <p className="text-[11px] font-semibold uppercase tracking-wider text-primary">
          Used by {rules.length} rule{rules.length === 1 ? '' : 's'}
        </p>
        <Link
          to="/strategies/rules"
          className="text-[11px] text-accent hover:text-primary hover:underline"
        >
          Open Rules →
        </Link>
      </div>
      <ul className="grid gap-1.5 sm:grid-cols-2 lg:grid-cols-3">
        {rules.map((r) => (
          <li
            key={r.id}
            className="flex flex-col gap-1 rounded-md border border-info/25 bg-info/8 px-2.5 py-2 text-left"
          >
            <span className="truncate text-[13px] font-semibold text-text">{r.rule_name}</span>
            <div className="flex flex-wrap items-center gap-1.5">
              <Badge variant={r.trade_mode === 'real' ? 'warning' : 'info'} size="sm">
                {r.trade_mode}
              </Badge>
              <Badge variant={r.is_active ? 'success' : 'neutral'} size="sm">
                {r.is_active ? 'Active' : 'Idle'}
              </Badge>
              <span className="tabular-nums text-[11px] text-text-dim">
                buy {lamportsToSol(r.buy_amount_lamports)}◎
              </span>
            </div>
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
export function FingerprintsView() {
  const { data: fps = [], isLoading } = useGetFingerprintsQuery();
  const { data: rules = [] } = useGetStrategyRulesQuery();
  const [createFp, { isLoading: creating }] = useCreateFingerprintMutation();
  const [updateFp, { isLoading: updating }] = useUpdateFingerprintMutation();
  const [deleteFp] = useDeleteFingerprintMutation();

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

  const submit = async (draft: FingerprintDraft) => {
    setErr(null);
    try {
      if (editing && editing !== 'new') await updateFp({ id: editing.id, body: draft }).unwrap();
      else await createFp(draft).unwrap();
      setEditing(null);
    } catch (e) {
      setErr(apiErrorMessage(e as never) ?? 'Save failed');
    }
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
        render: (r) => <span className="font-medium text-text">{r.name || r.id.slice(0, 8)}</span>,
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
          const n = r.ix_labels?.length;
          if (n == null || n === 0) return dash();
          return <span className="font-mono tabular-nums">{n}</span>;
        },
        searchValue: (r) => String(r.ix_labels?.length ?? ''),
        sortValue: (r) => r.ix_labels?.length ?? null,
        filterNumber: (r) => r.ix_labels?.length ?? null,
        sortable: true,
        cellClassName: cellTint('ix_count'),
      },
      {
        key: 'ix_labels',
        label: 'ix_labels',
        group: 'ix',
        width: '220px',
        render: (r) =>
          r.ix_labels?.length ? <IxLabelsDisplay labels={r.ix_labels} copyJson /> : dash(),
        searchValue: (r) => (r.ix_labels?.length ? formatIxLabelsText(r.ix_labels) : ''),
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
            {formatDecimalTrim(tidySolDecimal(r.bucket_size_amount), 6)}◎
          </span>
        ),
        searchValue: (r) => formatDecimalTrim(tidySolDecimal(r.bucket_size_amount), 6),
        sortValue: (r) => tidySolDecimal(r.bucket_size_amount),
        filterNumber: (r) => tidySolDecimal(r.bucket_size_amount),
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
    [valueColors],
  );

  const editingRules =
    editing && editing !== 'new' ? (rulesByFp.get(editing.id) ?? []) : [];

  return (
    <div className="flex flex-col gap-3 p-4">
      <div className="flex items-center justify-between gap-3">
        <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
          <h1 className="text-lg font-semibold text-text">Fingerprints</h1>
          <span className="text-sm text-text-mid">
            Match specs · select a row to see which rules use it
          </span>
        </div>
        <IconButton
          variant="success"
          size="lg"
          label="New fingerprint"
          title="New fingerprint"
          onClick={() => setEditing('new')}
        >
          <PlusIcon />
        </IconButton>
      </div>
      {err && <p className="text-[12px] text-red">{err}</p>}
      <DataTable
        columns={columns}
        rows={fps}
        rowKey={(r) => r.id}
        loading={isLoading}
        searchable
        colFilters
        colToggle
        tableId="fingerprints-v2"
        emptyMessage="No fingerprints yet — create one to start authoring rules."
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
        title={editing && editing !== 'new' ? 'Edit fingerprint' : 'New fingerprint'}
        open={editing !== null}
        onClose={() => setEditing(null)}
      >
        {editing !== null && (
          <div className="flex flex-col gap-3">
            {editing !== 'new' && <FingerprintUsedByDetail rules={editingRules} />}
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
