import { useEffect, useMemo, useState, type ReactNode } from 'react';

import { DataTable } from 'components/table/DataTable';
import type { ColumnDef } from 'components/table/types';
import { Button } from 'components/ui/Button';
import { Badge } from 'components/ui/Badge';
import { ruleParamsCell, ruleParamsSearchText } from './RuleParamsSummary';
import {
  fingerprintParamsCell,
  fingerprintParamsSearchText,
} from './FingerprintParamsSummary';
import { useRuleActions } from './useRuleActions';
import type { RuleEditorDraft } from './RuleEditor';
import { apiErrorMessage } from 'store/baseApi';
import {
  useGetStrategyRulesQuery,
  useGetFingerprintsQuery,
  useActivateStrategyRuleMutation,
  usePauseStrategyRuleMutation,
  useStopStrategyRuleMutation,
  usePauseAllStrategyRulesMutation,
  useStopAllStrategyRulesMutation,
} from 'store/sharedEndpoints';
import {
  connectActionProgressStream,
  connectTpslRulesChanged,
  type ActionProgress,
} from 'services/sse';
import { lamportsToSol, type StrategyRule, type TradeMode } from 'lib/strategy/types';

export interface RulesViewProps {
  /** Lab-only dry-run render-prop forwarded to the editor (FE3). */
  renderDryRun?: (draft: RuleEditorDraft | null, canRun: boolean) => ReactNode;
}

/**
 * Rules list + editor, shared by the live and lab apps. One list for all generic
 * rules (mode/status badges) with two row-action columns — **Actions** (edit /
 * duplicate / delete) and **Execute** (Activate → running rule, then Pause /
 * Stop) — plus header **Pause All / Stop All** bulk actions scoped per trade mode.
 * The editor opens in an xl modal. The lab app injects a dry-run panel via
 * `renderDryRun`.
 */
export function RulesView({ renderDryRun }: RulesViewProps) {
  const { data: rules = [], isLoading, refetch } = useGetStrategyRulesQuery();
  const { data: fps = [] } = useGetFingerprintsQuery();

  const actions = useRuleActions({ renderDryRun });

  const [activate] = useActivateStrategyRuleMutation();
  const [pause] = usePauseStrategyRuleMutation();
  const [stop] = useStopStrategyRuleMutation();
  const [pauseAll, pauseAllState] = usePauseAllStrategyRulesMutation();
  const [stopAll, stopAllState] = useStopAllStrategyRulesMutation();

  const [opErr, setOpErr] = useState<string | null>(null);
  /** Rule ids mid optimistic pause (label "Pausing…" until SSE/refetch confirms). */
  const [pausingIds, setPausingIds] = useState<Set<string>>(() => new Set());
  /** Per-rule stop progress keyed by rule_id (from action_progress SSE). */
  const [stopByRule, setStopByRule] = useState<Record<string, ActionProgress>>({});
  /** Bulk stop progress keyed by mode (action frames with no rule_id). */
  const [stopByMode, setStopByMode] = useState<Partial<Record<TradeMode, ActionProgress>>>({});

  const bulkBusy = pauseAllState.isLoading || stopAllState.isLoading;

  const fpById = useMemo(() => new Map(fps.map((f) => [f.id, f])), [fps]);

  // Active-rule counts per mode drive whether each mode's bulk actions show.
  const activeByMode = useMemo(() => {
    const acc: Record<TradeMode, number> = { paper: 0, real: 0 };
    for (const r of rules) if (r.is_active) acc[r.trade_mode] += 1;
    return acc;
  }, [rules]);

  // SSE: confirm pause flips + track stop rollups.
  useEffect(() => {
    const rulesH = connectTpslRulesChanged(() => {
      setPausingIds(new Set());
      void refetch();
    });
    const progressH = connectActionProgressStream((p) => {
      if (p.kind !== 'stop') return;
      if (p.rule_id) {
        setStopByRule((prev) => {
          const next = { ...prev };
          if (p.status === 'done' || p.status === 'partial' || p.status === 'failed') {
            delete next[p.rule_id!];
          } else {
            next[p.rule_id!] = p;
          }
          return next;
        });
      } else {
        // Bulk Stop All — match the action_id seeded by doStopAll.
        setStopByMode((prev) => {
          const next = { ...prev };
          for (const mode of ['paper', 'real'] as TradeMode[]) {
            if (prev[mode]?.action_id !== p.action_id) continue;
            if (p.status === 'done' || p.status === 'partial' || p.status === 'failed') {
              delete next[mode];
            } else {
              next[mode] = p;
            }
          }
          return next;
        });
      }
    });
    return () => {
      rulesH.close();
      progressH.close();
    };
  }, [refetch]);

  const run = async (fn: () => Promise<unknown>, fail: string) => {
    setOpErr(null);
    try {
      await fn();
    } catch (e) {
      setOpErr(apiErrorMessage(e as never) ?? fail);
    }
  };

  const pauseRule = (r: StrategyRule) => {
    setPausingIds((prev) => new Set(prev).add(r.id));
    void run(async () => {
      try {
        await pause(r.id).unwrap();
      } catch (e) {
        setPausingIds((prev) => {
          const next = new Set(prev);
          next.delete(r.id);
          return next;
        });
        throw e;
      }
    }, 'Pause failed');
  };

  const stopRule = (r: StrategyRule) => {
    if (
      r.trade_mode === 'real' &&
      !window.confirm(`Stop "${r.rule_name}" and close its open positions? REAL mode sends on-chain sells.`)
    )
      return;
    void run(async () => {
      const res = await stop(r.id).unwrap();
      if (res.action_id) {
        setStopByRule((prev) => ({
          ...prev,
          [r.id]: {
            action_id: res.action_id,
            mint_address: null,
            rule_id: r.id,
            kind: 'stop',
            status: 'running',
            done: 0,
            total: res.total ?? 0,
          },
        }));
      }
    }, 'Stop failed');
  };

  const doPauseAll = (mode: TradeMode) => {
    for (const r of rules) {
      if (r.is_active && r.trade_mode === mode) {
        setPausingIds((prev) => new Set(prev).add(r.id));
      }
    }
    void run(() => pauseAll(mode).unwrap(), 'Pause all failed');
  };

  const doStopAll = (mode: TradeMode) => {
    const warn =
      mode === 'real'
        ? `Stop ALL active real rules and close every open position? This sends on-chain sells.`
        : `Stop ALL active paper rules and close every open position?`;
    if (!window.confirm(warn)) return;
    void run(async () => {
      const res = await stopAll(mode).unwrap();
      if (res.action_id) {
        setStopByMode((prev) => ({
          ...prev,
          [mode]: {
            action_id: res.action_id,
            mint_address: null,
            kind: 'stop',
            status: 'running',
            done: 0,
            total: res.total ?? 0,
          },
        }));
      }
    }, 'Stop all failed');
  };

  const columns: ColumnDef<StrategyRule>[] = [
    {
      key: 'rule_name',
      label: 'Name',
      group: 'name',
      render: (r) => <span className="font-medium text-text">{r.rule_name}</span>,
      searchValue: (r) => r.rule_name,
    },
    {
      key: 'status',
      label: 'Status',
      group: 'status',
      render: (r) => {
        if (pausingIds.has(r.id) && r.is_active) {
          return <Badge variant="warning">Pausing…</Badge>;
        }
        const stopProg = stopByRule[r.id];
        if (stopProg && stopProg.status === 'running') {
          return (
            <Badge variant="warning">
              Stopping {stopProg.done}/{stopProg.total}
            </Badge>
          );
        }
        return (
          <Badge variant={r.is_active ? 'success' : 'neutral'}>{r.is_active ? 'Active' : 'Idle'}</Badge>
        );
      },
      searchValue: (r) => (r.is_active ? 'active' : 'idle'),
    },
    {
      key: 'mode',
      label: 'Mode',
      group: 'status',
      render: (r) => (
        <Badge variant={r.trade_mode === 'real' ? 'warning' : 'info'}>{r.trade_mode}</Badge>
      ),
      searchValue: (r) => r.trade_mode,
    },
    {
      key: 'fingerprint',
      label: 'Fingerprint',
      group: 'fingerprint',
      render: (r) => {
        const fp = fpById.get(r.fingerprint_id);
        return (
          <div className="flex min-w-48 flex-col gap-1">
            <span className="font-mono text-[12px] text-text-dim">
              {fp?.name || r.fingerprint_id.slice(0, 8)}
            </span>
            {fp ? fingerprintParamsCell(fp) : null}
          </div>
        );
      },
      searchValue: (r) => fingerprintParamsSearchText(fpById.get(r.fingerprint_id), r.fingerprint_id),
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
      group: 'params',
      render: (r) => ruleParamsCell(r.params),
      searchValue: (r) => ruleParamsSearchText(r.params),
    },
    {
      key: 'execute',
      label: 'Execute',
      render: (r) => {
        const stopProg = stopByRule[r.id];
        const stopping = !!stopProg && stopProg.status === 'running';
        const pausing = pausingIds.has(r.id);
        if (r.is_active || stopping || pausing) {
          return (
            <div className="flex gap-1">
              <Button
                variant="subtle"
                size="xs"
                disabled={pausing || stopping}
                onClick={() => pauseRule(r)}
                title="Pause — stop new entries, let open positions drain"
              >
                {pausing ? 'Pausing…' : 'Pause'}
              </Button>
              <Button
                variant="danger"
                size="xs"
                disabled={stopping}
                onClick={() => stopRule(r)}
                title="Stop — deactivate and force-close open positions now"
              >
                {stopping ? `Stopping ${stopProg!.done}/${stopProg!.total}` : 'Stop'}
              </Button>
            </div>
          );
        }
        return (
          <Button
            variant="primary"
            size="xs"
            onClick={() => void run(() => activate(r.id).unwrap(), 'Activate failed')}
            title="Activate — arm this rule to fire on matching tokens"
          >
            Activate
          </Button>
        );
      },
      searchValue: (r) => (r.is_active ? 'pause stop' : 'activate'),
    },
  ];

  const err = actions.err || opErr;

  return (
    <div className="flex flex-col gap-3 p-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h1 className="text-lg font-semibold text-text">Rules</h1>
        <div className="flex flex-wrap items-center gap-1.5">
          {(['paper', 'real'] as TradeMode[]).map((mode) => {
            const bulkStop = stopByMode[mode];
            const stopping = !!bulkStop && bulkStop.status === 'running';
            const show = activeByMode[mode] > 0 || stopping;
            if (!show) return null;
            return (
              <div key={mode} className="flex items-center gap-1 rounded-md border border-white/8 px-1.5 py-1">
                <span className="text-[11px] uppercase tracking-wide text-text-dim">{mode}</span>
                <Button
                  variant="ghost"
                  size="xs"
                  disabled={bulkBusy || stopping}
                  onClick={() => doPauseAll(mode)}
                >
                  ⏸ Pause All ({activeByMode[mode]})
                </Button>
                <Button
                  variant="danger"
                  size="xs"
                  disabled={bulkBusy || stopping}
                  onClick={() => doStopAll(mode)}
                >
                  {stopping
                    ? `■ Stopping ${bulkStop!.done}/${bulkStop!.total}`
                    : `■ Stop All (${activeByMode[mode]})`}
                </Button>
              </div>
            );
          })}
          <Button variant="primary" size="sm" onClick={actions.openNew}>
            + New rule
          </Button>
        </div>
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
            <Button variant="ghost" size="xs" onClick={() => actions.edit(r)}>
              Edit
            </Button>
            <Button variant="ghost" size="xs" onClick={() => actions.duplicate(r)}>
              Duplicate
            </Button>
            <Button variant="ghost" size="xs" onClick={() => void actions.remove(r)}>
              Delete
            </Button>
          </div>
        )}
      />
      {actions.editorNode}
    </div>
  );
}
