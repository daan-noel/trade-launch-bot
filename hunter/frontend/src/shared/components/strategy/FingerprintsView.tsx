import { useMemo, useState, type ReactNode } from 'react';

import { DataTable } from 'components/table/DataTable';
import type { ColumnDef } from 'components/table/types';
import { Button } from 'components/ui/Button';
import { Badge } from 'components/ui/Badge';
import { Modal } from 'components/ui/Modal';
import { IxLabelsDisplay } from 'components/ui/IxLabelsDisplay';
import { FingerprintForm } from './FingerprintForm';
import { apiErrorMessage } from 'store/baseApi';
import {
  useGetFingerprintsQuery,
  useCreateFingerprintMutation,
  useUpdateFingerprintMutation,
  useDeleteFingerprintMutation,
} from 'store/sharedEndpoints';
import { formatIxLabelsText } from 'lib/ixLabels';
import { computeSameValueCellClasses } from 'lib/sameValueCellColors';
import { lamportsToSol, type Fingerprint, type FingerprintDraft } from 'lib/strategy/types';

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
  { key: 'bucket', valueOf: (r) => String(r.bucket_size_amount) },
];

/**
 * Fingerprint library, shared by the live and lab apps: the match specs rules
 * reference. List (used-by count) + create/edit form (SOL inputs, lamports at
 * the API boundary) + used-by-guarded delete.
 */
export function FingerprintsView() {
  const { data: fps = [], isLoading } = useGetFingerprintsQuery();
  const [createFp, { isLoading: creating }] = useCreateFingerprintMutation();
  const [updateFp, { isLoading: updating }] = useUpdateFingerprintMutation();
  const [deleteFp] = useDeleteFingerprintMutation();

  const [editing, setEditing] = useState<Fingerprint | 'new' | null>(null);
  const [err, setErr] = useState<string | null>(null);

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
        render: (r) => <span className="font-medium text-text">{r.name || r.id.slice(0, 8)}</span>,
        searchValue: (r) => r.name,
      },
      {
        key: 'cu_limit',
        label: 'cu_limit',
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
        width: '220px',
        render: (r) =>
          r.ix_labels?.length ? <IxLabelsDisplay labels={r.ix_labels} copyJson /> : dash(),
        searchValue: (r) => (r.ix_labels?.length ? formatIxLabelsText(r.ix_labels) : ''),
        cellClassName: cellTint('ix_labels'),
      },
      {
        key: 'bucket',
        label: 'Bucket',
        render: (r) => <span className="font-mono tabular-nums">{r.bucket_size_amount}◎</span>,
        searchValue: (r) => String(r.bucket_size_amount),
        sortValue: (r) => r.bucket_size_amount,
        filterNumber: (r) => r.bucket_size_amount,
        sortable: true,
        cellClassName: cellTint('bucket'),
      },
      {
        key: 'used_by',
        label: 'Used by',
        render: (r) => <Badge variant={r.used_by ? 'info' : 'neutral'}>{r.used_by ?? 0}</Badge>,
        searchValue: (r) => String(r.used_by ?? 0),
        sortValue: (r) => r.used_by ?? 0,
        filterNumber: (r) => r.used_by ?? 0,
        sortable: true,
      },
    ],
    [valueColors],
  );

  return (
    <div className="flex flex-col gap-3 p-4">
      <div className="flex items-center justify-between">
        <h1 className="text-lg font-semibold text-text">Fingerprints</h1>
        <Button variant="primary" size="sm" onClick={() => setEditing('new')}>
          + New fingerprint
        </Button>
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
        rowActions={(r) => (
          <div className="flex gap-1">
            <Button variant="ghost" size="xs" onClick={() => setEditing(r)}>
              Edit
            </Button>
            <Button
              variant="ghost"
              size="xs"
              disabled={Boolean(r.used_by && r.used_by > 0)}
              title={r.used_by ? `used by ${r.used_by} rule(s)` : 'delete'}
              onClick={() => remove(r)}
            >
              Delete
            </Button>
          </div>
        )}
      />
      <Modal
        title={editing && editing !== 'new' ? 'Edit fingerprint' : 'New fingerprint'}
        open={editing !== null}
        onClose={() => setEditing(null)}
      >
        {editing !== null && (
          <FingerprintForm
            initial={editing === 'new' ? undefined : editing}
            onSubmit={submit}
            onCancel={() => setEditing(null)}
            submitting={creating || updating}
            error={err}
          />
        )}
      </Modal>
    </div>
  );
}
