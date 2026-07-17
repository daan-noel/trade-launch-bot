import { useMemo, useState, type ReactNode } from 'react';

import { DataTable } from 'components/table/DataTable';
import type { ColumnDef } from 'components/table/types';
import { Button } from 'components/ui/Button';
import { Badge } from 'components/ui/Badge';
import { Modal } from 'components/ui/Modal';
import { RuleEditor, type RuleEditorDraft } from './RuleEditor';
import { ruleParamsCell, ruleParamsSearchText } from './RuleParamsSummary';
import { apiErrorMessage } from 'store/baseApi';
import {
  useGetStrategyRulesQuery,
  useGetFingerprintsQuery,
  useCreateStrategyRuleMutation,
  useUpdateStrategyRuleMutation,
  useDeleteStrategyRuleMutation,
  useActivateStrategyRuleMutation,
  usePauseStrategyRuleMutation,
} from 'store/sharedEndpoints';
import { lamportsToSol, type StrategyRule } from 'lib/strategy/types';

export interface RulesViewProps {
  /** Lab-only dry-run render-prop forwarded to the editor (FE3). */
  renderDryRun?: (draft: RuleEditorDraft | null, canRun: boolean) => ReactNode;
}

/**
 * Rules list + editor, shared by the live and lab apps. One list for all generic
 * rules (mode/status badges) with edit / duplicate / activate-pause / delete row
 * actions; the RuleEditor opens in an xl modal. The lab app injects a dry-run
 * panel via `renderDryRun`.
 */
export function RulesView({ renderDryRun }: RulesViewProps) {
  const { data: rules = [], isLoading } = useGetStrategyRulesQuery();
  const { data: fps = [] } = useGetFingerprintsQuery();
  const [createRule, { isLoading: creating }] = useCreateStrategyRuleMutation();
  const [updateRule, { isLoading: updating }] = useUpdateStrategyRuleMutation();
  const [deleteRule] = useDeleteStrategyRuleMutation();
  const [activate] = useActivateStrategyRuleMutation();
  const [pause] = usePauseStrategyRuleMutation();

  const [editing, setEditing] = useState<StrategyRule | 'new' | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const fpName = useMemo(() => new Map(fps.map((f) => [f.id, f.name || f.id.slice(0, 8)])), [fps]);

  const submit = async (draft: RuleEditorDraft) => {
    setErr(null);
    try {
      if (editing && editing !== 'new' && editing.id) {
        await updateRule({
          id: editing.id,
          body: {
            rule_name: draft.rule_name,
            trade_mode: draft.trade_mode,
            buy_amount_lamports: draft.buy_amount_lamports,
            max_concurrent_tokens: draft.max_concurrent_tokens,
            max_total_tokens: draft.max_total_tokens,
            params: draft.params,
          },
        }).unwrap();
      } else {
        await createRule(draft).unwrap();
      }
      setEditing(null);
    } catch (e) {
      setErr(apiErrorMessage(e as never) ?? 'Save failed');
    }
  };

  const toggleActive = async (r: StrategyRule) => {
    try {
      if (r.is_active) await pause(r.id).unwrap();
      else await activate(r.id).unwrap();
    } catch (e) {
      setErr(apiErrorMessage(e as never) ?? 'Toggle failed');
    }
  };

  const remove = async (r: StrategyRule) => {
    if (!window.confirm(`Delete rule "${r.rule_name}"?`)) return;
    try {
      await deleteRule(r.id).unwrap();
    } catch (e) {
      setErr(apiErrorMessage(e as never) ?? 'Delete failed');
    }
  };

  const duplicate = (r: StrategyRule) =>
    setEditing({ ...r, id: '', rule_name: `${r.rule_name} copy`, is_active: false });

  const columns: ColumnDef<StrategyRule>[] = [
    {
      key: 'rule_name',
      label: 'Name',
      render: (r) => <span className="font-medium text-text">{r.rule_name}</span>,
      searchValue: (r) => r.rule_name,
    },
    {
      key: 'status',
      label: 'Status',
      render: (r) => (
        <Badge variant={r.is_active ? 'success' : 'neutral'}>{r.is_active ? 'Active' : 'Idle'}</Badge>
      ),
      searchValue: (r) => (r.is_active ? 'active' : 'idle'),
    },
    {
      key: 'mode',
      label: 'Mode',
      render: (r) => (
        <Badge variant={r.trade_mode === 'real' ? 'warning' : 'info'}>{r.trade_mode}</Badge>
      ),
      searchValue: (r) => r.trade_mode,
    },
    {
      key: 'fingerprint',
      label: 'Fingerprint',
      render: (r) => (
        <span className="font-mono text-[12px] text-text-dim">
          {fpName.get(r.fingerprint_id) ?? r.fingerprint_id.slice(0, 8)}
        </span>
      ),
      searchValue: (r) => fpName.get(r.fingerprint_id) ?? r.fingerprint_id,
    },
    {
      key: 'buy',
      label: 'Buy',
      render: (r) => <span className="tabular-nums">{lamportsToSol(r.buy_amount_lamports)}◎</span>,
      searchValue: (r) => String(lamportsToSol(r.buy_amount_lamports)),
      sortValue: (r) => r.buy_amount_lamports,
      sortable: true,
    },
    {
      key: 'caps',
      label: 'Caps',
      render: (r) => (
        <span className="tabular-nums text-text-dim">
          {r.max_concurrent_tokens}/{r.max_total_tokens || '∞'}
        </span>
      ),
      searchValue: (r) => `${r.max_concurrent_tokens}/${r.max_total_tokens}`,
    },
    {
      key: 'params',
      label: 'Params',
      render: (r) => ruleParamsCell(r.params),
      searchValue: (r) => ruleParamsSearchText(r.params),
    },
  ];

  return (
    <div className="flex flex-col gap-3 p-4">
      <div className="flex items-center justify-between">
        <h1 className="text-lg font-semibold text-text">Rules</h1>
        <Button variant="primary" size="sm" onClick={() => setEditing('new')}>
          + New rule
        </Button>
      </div>
      {err && <p className="text-[12px] text-red">{err}</p>}
      <DataTable
        columns={columns}
        rows={rules}
        rowKey={(r) => r.id}
        loading={isLoading}
        searchable
        colFilters
        tableId="strategy-rules"
        emptyMessage="No rules yet — create one from a fingerprint."
        rowActions={(r) => (
          <div className="flex gap-1">
            <Button variant="ghost" size="xs" onClick={() => setEditing(r)}>
              Edit
            </Button>
            <Button variant="ghost" size="xs" onClick={() => duplicate(r)}>
              Duplicate
            </Button>
            <Button
              variant={r.is_active ? 'subtle' : 'primary'}
              size="xs"
              onClick={() => toggleActive(r)}
            >
              {r.is_active ? 'Pause' : 'Activate'}
            </Button>
            <Button variant="ghost" size="xs" onClick={() => remove(r)}>
              Delete
            </Button>
          </div>
        )}
      />
      <Modal
        title={editing && editing !== 'new' && editing.id ? 'Edit rule' : 'New rule'}
        open={editing !== null}
        onClose={() => setEditing(null)}
        size="xl"
      >
        {editing !== null && (
          <RuleEditor
            initial={editing === 'new' ? undefined : editing.id ? editing : { ...editing }}
            onSubmit={submit}
            onCancel={() => setEditing(null)}
            submitting={creating || updating}
            error={err}
            renderDryRun={renderDryRun}
          />
        )}
      </Modal>
    </div>
  );
}
