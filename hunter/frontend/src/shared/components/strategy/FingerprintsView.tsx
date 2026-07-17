import { useState } from 'react';

import { DataTable } from 'components/table/DataTable';
import type { ColumnDef } from 'components/table/types';
import { Button } from 'components/ui/Button';
import { Badge } from 'components/ui/Badge';
import { Modal } from 'components/ui/Modal';
import { FingerprintForm } from './FingerprintForm';
import { apiErrorMessage } from 'store/baseApi';
import {
  useGetFingerprintsQuery,
  useCreateFingerprintMutation,
  useUpdateFingerprintMutation,
  useDeleteFingerprintMutation,
} from 'store/sharedEndpoints';
import { lamportsToSol, type Fingerprint, type FingerprintDraft } from 'lib/strategy/types';

/** Short chips summarizing a fingerprint's configured match criteria. */
function criteria(fp: Fingerprint): string[] {
  const out: string[] = [];
  if (fp.cu_limit != null) out.push(`cu_limit=${fp.cu_limit}`);
  if (fp.cu_price != null) out.push(`cu_price=${fp.cu_price}`);
  const sol = (l: number | null, label: string) => {
    const s = lamportsToSol(l);
    if (s != null) out.push(`${label}=${s}◎`);
  };
  sol(fp.init_buy_lamports, 'init_buy');
  sol(fp.max_cost_lamports, 'max_cost');
  sol(fp.spendable_lamports_in, 'spendable');
  sol(fp.first_slot_buy_lamports, 'fs_buy');
  sol(fp.first_slot_sell_lamports, 'fs_sell');
  if (fp.ix_labels?.length) out.push(`ix:[${fp.ix_labels.join(',')}]`);
  return out;
}

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

  const columns: ColumnDef<Fingerprint>[] = [
    {
      key: 'name',
      label: 'Name',
      render: (r) => <span className="font-medium text-text">{r.name || r.id.slice(0, 8)}</span>,
      searchValue: (r) => r.name,
    },
    {
      key: 'criteria',
      label: 'Match criteria',
      render: (r) => (
        <div className="flex flex-wrap gap-1">
          {criteria(r).map((c, i) => (
            <span key={i} className="rounded bg-white/6 px-1.5 py-0.5 font-mono text-[10px] text-text-dim">
              {c}
            </span>
          ))}
        </div>
      ),
      searchValue: (r) => criteria(r).join(' '),
    },
    {
      key: 'bucket',
      label: 'Bucket',
      render: (r) => <span className="tabular-nums">{r.bucket_size_amount}◎</span>,
      searchValue: (r) => String(r.bucket_size_amount),
      sortValue: (r) => r.bucket_size_amount,
      filterNumber: (r) => r.bucket_size_amount,
      sortable: true,
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
  ];

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
        tableId="fingerprints"
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
