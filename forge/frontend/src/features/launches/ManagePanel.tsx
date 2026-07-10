import { useState } from 'react';
import {
  useManagePreviewMutation,
  useManageExecuteMutation,
  useManageActionsQuery,
} from '@shared/store/endpoints';
import { apiErrorMessage } from '@shared/store/baseApi';
import {
  AddressDisplay,
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
import type { ActionPlan, ManageAction, ManageRequest, PlanLeg, TokenOverview } from '@shared/types';

type Kind = 'sell' | 'buy' | 'consolidate';
type Group = 'all' | 'dev' | 'bundler';

const SIZING: Record<Kind, string> = {
  sell: 'pct_of_holdings',
  buy: 'fixed_sol',
  consolidate: 'sweep',
};

/** Post-launch management: preview → (confirm) → execute a sell / buy / SOL
 *  consolidate across a wallet group, plus the action history. Executing places
 *  REAL trades/transfers and requires the backend kill switch
 *  (`MANAGE_ENABLED=true`) — a disabled backend surfaces a clear 503 here. */
export function ManagePanel({ mint, overview }: { mint: string; overview: TokenOverview | undefined }) {
  const [kind, setKind] = useState<Kind>('sell');
  const [group, setGroup] = useState<Group>('all');
  const [pct, setPct] = useState(100);
  const [sol, setSol] = useState(0.05);
  const [plan, setPlan] = useState<ActionPlan | null>(null);
  const [result, setResult] = useState<ManageAction | null>(null);

  const [preview, previewState] = useManagePreviewMutation();
  const [execute, executeState] = useManageExecuteMutation();
  const { data: actions = [] } = useManageActionsQuery({ mint }, { skip: !mint });

  const qd = overview?.quote_decimals ?? 9;
  const qs = overview?.quote_symbol ?? 'SOL';

  // Buy/consolidate resolve managed wallets by role — no natural "all", so those
  // kinds restrict the group to dev/bundler.
  const groupOptions: Group[] = kind === 'sell' ? ['all', 'dev', 'bundler'] : ['dev', 'bundler'];

  const onKind = (k: Kind) => {
    setKind(k);
    setPlan(null);
    setResult(null);
    if (k !== 'sell' && group === 'all') setGroup('dev');
  };

  const body = (): ManageRequest => ({
    kind,
    sizing: SIZING[kind],
    size: kind === 'sell' ? pct : kind === 'buy' ? sol : 0,
    selection: group === 'all' ? {} : { role: group },
  });

  const onPreview = async () => {
    setResult(null);
    try {
      setPlan(await preview({ mint, body: body() }).unwrap());
    } catch {
      setPlan(null);
    }
  };

  const onExecute = async () => {
    if (!plan) return;
    const groupLabel = group === 'all' ? 'ALL wallets' : `${group} wallets`;
    const verb =
      kind === 'sell'
        ? `Sell ${pct}% of holdings`
        : kind === 'buy'
          ? `Buy ${sol} ${qs} each`
          : 'Sweep all SOL to treasury';
    if (
      !window.confirm(
        `${verb} across ${groupLabel} (${plan.legs.length} wallet(s))?\n\nThis places REAL on-chain transactions and cannot be undone.`,
      )
    )
      return;
    try {
      setResult(await execute({ mint, body: body() }).unwrap());
      setPlan(null);
    } catch {
      /* rendered from executeState below */
    }
  };

  const previewErr = apiErrorMessage(previewState.error);
  const executeErr = apiErrorMessage(executeState.error);

  return (
    <Card title="Manage">
      <div className="space-y-4">
        <div className="flex flex-wrap items-end gap-3">
          <Field label="Action" className="w-36">
            <Select value={kind} onChange={(e) => onKind(e.target.value as Kind)}>
              <option value="sell">Sell</option>
              <option value="buy">Buy</option>
              <option value="consolidate">Consolidate SOL</option>
            </Select>
          </Field>

          <Field label="Wallet group" className="w-40">
            <Select value={group} onChange={(e) => setGroup(e.target.value as Group)}>
              {groupOptions.map((g) => (
                <option key={g} value={g}>
                  {g === 'all' ? 'All wallets' : g === 'dev' ? 'Dev only' : 'Bundlers only'}
                </option>
              ))}
            </Select>
          </Field>

          {kind === 'sell' && (
            <>
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
            </>
          )}

          {kind === 'buy' && (
            <Field label={`${qs} per wallet`} className="w-32">
              <Input
                type="number"
                min={0}
                step={0.01}
                value={sol}
                onChange={(e) => setSol(Math.max(0, Number(e.target.value) || 0))}
              />
            </Field>
          )}

          <Button variant="primary" onClick={onPreview} loading={previewState.isLoading}>
            Preview
          </Button>
        </div>

        {previewErr && <Banner tone="bad">{previewErr}</Banner>}

        {plan && (
          <div className="space-y-3">
            {plan.legs.length === 0 ? (
              <Banner tone="warn">No wallets matched — nothing to {kind}.</Banner>
            ) : (
              <>
                <PlanTable plan={plan} td={overview?.decimals ?? 6} qd={qd} qs={qs} />
                <div className="flex items-center justify-between text-sm">
                  <span className="muted">{plan.legs.length} leg(s)</span>
                  <Button
                    variant="danger"
                    onClick={onExecute}
                    loading={executeState.isLoading}
                  >
                    Execute {kind}
                  </Button>
                </div>
              </>
            )}
          </div>
        )}

        {executeErr && <Banner tone="bad">{executeErr}</Banner>}

        {result && (
          <Banner tone={result.status === 'completed' ? 'good' : result.status === 'failed' ? 'bad' : 'warn'}>
            {result.kind} {result.status}: {result.legs_confirmed}/{result.legs_total} legs confirmed
            {result.error ? ` — ${result.error}` : ''}
          </Banner>
        )}

        <ActionsHistory actions={actions} />
      </div>
    </Card>
  );
}

function PlanTable({
  plan,
  td,
  qd,
  qs,
}: {
  plan: ActionPlan;
  td: number;
  qd: number;
  qs: string;
}) {
  const base: Column<PlanLeg>[] = [
    { header: 'Role', render: (l) => <StatusPill status={l.role} /> },
    { header: 'Wallet', render: (l) => <AddressDisplay value={l.wallet_address} /> },
  ];
  const amountCols: Column<PlanLeg>[] =
    plan.kind === 'sell'
      ? [
          {
            header: 'Sell (tokens)',
            align: 'right',
            render: (l) => (
              <span className="mono">
                {(l.amount_base / 10 ** td).toLocaleString(undefined, { maximumFractionDigits: 2 })}
              </span>
            ),
          },
          {
            header: `Est. ${qs}`,
            align: 'right',
            render: (l) => <span className="mono">{quoteToHuman(l.est_quote, qd)}</span>,
          },
        ]
      : [
          {
            header: plan.kind === 'buy' ? `Spend ${qs}` : `Sweep ${qs} (est)`,
            align: 'right',
            render: (l) => <span className="mono">{quoteToHuman(l.spend_quote, qd)}</span>,
          },
        ];
  return <DataTable columns={[...base, ...amountCols]} rows={plan.legs} rowKey={(l) => l.managed_wallet_id} />;
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
