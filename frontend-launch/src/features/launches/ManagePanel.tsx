import { useState } from 'react';
import {
  useManagePreviewMutation,
  useManageExecuteMutation,
  useManageActionsQuery,
} from '@shared/store/endpoints';
import { apiErrorMessage } from '@shared/store/baseApi';
import {
  Banner,
  Button,
  Card,
  Column,
  DataTable,
  Field,
  Input,
  Select,
  StatusPill,
} from '@shared/components/ui';
import { formatAge, quoteToHuman } from '@shared/lib/format';
import type { ActionPlan, ManageAction, PlanLeg, TokenOverview } from '@shared/types';

type Group = 'all' | 'dev' | 'bundler';

/** Post-launch management: preview → (confirm) → execute a sell across a wallet
 *  group, plus the action history. Executing places REAL sells and requires the
 *  backend kill switch (`MANAGE_ENABLED=true`) — a disabled backend surfaces a
 *  clear 503 here rather than failing silently. */
export function ManagePanel({ mint, overview }: { mint: string; overview: TokenOverview | undefined }) {
  const [group, setGroup] = useState<Group>('all');
  const [pct, setPct] = useState(100);
  const [plan, setPlan] = useState<ActionPlan | null>(null);
  const [result, setResult] = useState<ManageAction | null>(null);

  const [preview, previewState] = useManagePreviewMutation();
  const [execute, executeState] = useManageExecuteMutation();
  const { data: actions = [] } = useManageActionsQuery({ mint }, { skip: !mint });

  const td = overview?.decimals ?? 6;
  const qd = overview?.quote_decimals ?? 9;
  const qs = overview?.quote_symbol ?? 'quote';

  const body = () => ({
    kind: 'sell',
    sizing: 'pct_of_holdings',
    size: pct,
    selection: group === 'all' ? {} : { role: group },
  });

  const onPreview = async () => {
    setResult(null);
    try {
      const p = await preview({ mint, body: body() }).unwrap();
      setPlan(p);
    } catch {
      setPlan(null);
    }
  };

  const onExecute = async () => {
    if (!plan) return;
    const groupLabel = group === 'all' ? 'ALL wallets' : `${group} wallets`;
    if (
      !window.confirm(
        `Sell ${pct}% of holdings across ${groupLabel} (${plan.legs.length} wallet(s))?\n\nThis places REAL sells and cannot be undone.`,
      )
    )
      return;
    try {
      const r = await execute({ mint, body: body() }).unwrap();
      setResult(r);
      setPlan(null);
    } catch {
      /* error rendered from executeState below */
    }
  };

  const previewErr = apiErrorMessage(previewState.error);
  const executeErr = apiErrorMessage(executeState.error);

  return (
    <Card title="Manage — sell">
      <div className="space-y-4">
        <div className="flex flex-wrap items-end gap-3">
          <Field label="Wallet group" className="w-40">
            <Select value={group} onChange={(e) => setGroup(e.target.value as Group)}>
              <option value="all">All wallets</option>
              <option value="dev">Dev only</option>
              <option value="bundler">Bundlers only</option>
            </Select>
          </Field>
          <Field label="Sell %" className="w-28">
            <Input
              type="number"
              min={1}
              max={100}
              value={pct}
              onChange={(e) => setPct(Math.max(1, Math.min(100, Number(e.target.value) || 0)))}
            />
          </Field>
          <div className="flex gap-1">
            {[25, 50, 100].map((v) => (
              <Button key={v} size="sm" variant={pct === v ? 'primary' : 'ghost'} onClick={() => setPct(v)}>
                {v}%
              </Button>
            ))}
          </div>
          <Button variant="primary" onClick={onPreview} loading={previewState.isLoading}>
            Preview
          </Button>
        </div>

        {previewErr && <Banner tone="bad">{previewErr}</Banner>}

        {plan && (
          <div className="space-y-3">
            {plan.legs.length === 0 ? (
              <Banner tone="warn">
                No wallets to sell — the selected group holds nothing (or this token isn't one of our
                launches).
              </Banner>
            ) : (
              <>
                <PlanTable legs={plan.legs} td={td} qd={qd} qs={qs} />
                <div className="flex items-center justify-between text-sm">
                  <span className="muted">
                    {plan.legs.length} leg(s) · est. proceeds{' '}
                    <span className="mono">
                      {quoteToHuman(plan.total_est_quote, qd)} {qs}
                    </span>
                  </span>
                  <Button variant="danger" onClick={onExecute} loading={executeState.isLoading}>
                    Execute sell
                  </Button>
                </div>
              </>
            )}
          </div>
        )}

        {executeErr && <Banner tone="bad">{executeErr}</Banner>}

        {result && (
          <Banner tone={result.status === 'completed' ? 'good' : result.status === 'failed' ? 'bad' : 'warn'}>
            Sell {result.status}: {result.legs_confirmed}/{result.legs_total} legs confirmed
            {result.error ? ` — ${result.error}` : ''}
          </Banner>
        )}

        <ActionsHistory actions={actions} />
      </div>
    </Card>
  );
}

function PlanTable({
  legs,
  td,
  qd,
  qs,
}: {
  legs: PlanLeg[];
  td: number;
  qd: number;
  qs: string;
}) {
  const columns: Column<PlanLeg>[] = [
    { header: 'Role', render: (l) => <StatusPill status={l.role} /> },
    { header: 'Wallet', render: (l) => <span className="mono text-xs muted">{l.wallet_id.slice(0, 8)}</span> },
    {
      header: 'Sell (tokens)',
      align: 'right',
      render: (l) => (
        <span className="mono">{(l.amount_base / 10 ** td).toLocaleString(undefined, { maximumFractionDigits: 2 })}</span>
      ),
    },
    {
      header: `Est. ${qs}`,
      align: 'right',
      render: (l) => <span className="mono">{quoteToHuman(l.est_quote, qd)}</span>,
    },
  ];
  return <DataTable columns={columns} rows={legs} rowKey={(l) => l.wallet_id} />;
}

function ActionsHistory({ actions }: { actions: ManageAction[] }) {
  if (actions.length === 0) return null;
  const columns: Column<ManageAction>[] = [
    { header: 'When', render: (a) => <span className="text-xs muted">{formatAge(a.created_at)}</span> },
    { header: 'Action', render: (a) => <span className="text-xs">{a.kind} · {a.sizing}</span> },
    { header: 'Status', render: (a) => <StatusPill status={a.status} /> },
    {
      header: 'Legs',
      align: 'right',
      render: (a) => <span className="mono text-xs">{a.legs_confirmed}/{a.legs_total}</span>,
    },
  ];
  return (
    <div className="pt-2">
      <div className="field-label mb-1">History</div>
      <DataTable columns={columns} rows={actions} rowKey={(a) => a.id} maxHeight={240} />
    </div>
  );
}
