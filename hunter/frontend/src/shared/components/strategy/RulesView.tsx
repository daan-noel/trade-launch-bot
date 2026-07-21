import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { Link } from 'react-router-dom';

import { DataTable } from 'components/table/DataTable';
import type { ColumnDef } from 'components/table/types';
import { IconButton } from 'components/ui/IconButton';
import { IconButtonGroup } from 'components/ui/IconButtonGroup';
import {
  DisableIcon,
  DuplicateIcon,
  EditIcon,
  EnableIcon,
  LinkIcon,
  PauseIcon,
  PlayIcon,
  PlusIcon,
  SpinnerIcon,
  StopIcon,
  TrashIcon,
} from 'components/ui/icons';
import { Badge } from 'components/ui/Badge';
import { buildCapsColumns } from './capsRuleColumns';
import { buildFingerprintRuleColumns } from './fingerprintRuleColumns';
import { buildRuleParamsColumns } from './ruleParamsColumns';
import { RuleHoverTip } from './RuleHoverTip';
import { useRuleActions } from './useRuleActions';
import type { RuleEditorDraft } from './RuleEditor';
import { useSelectionSearchParam } from 'hooks/useSelectionSearchParam';
import { apiErrorMessage } from 'store/baseApi';
import {
  useGetStrategyRulesQuery,
  useGetFingerprintsQuery,
  useActivateStrategyRuleMutation,
  useEnableStrategyRuleMutation,
  useDisableStrategyRuleMutation,
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
import { computeSameValueCellClasses } from 'lib/sameValueCellColors';
import {
  formatSignedPct,
  pctGradeClass,
  signedToneClass,
  winRateGradeClass,
} from 'lib/signedTone';
import { simulateHref, STRATEGY_PARAMS } from 'lib/strategy/nav';
import {
  disabledRuleRowClass,
  lamportsToSol,
  type StrategyRule,
  type TradeMode,
} from 'lib/strategy/types';

/** Closed trades behind Win% / Avg% / Exp (entered + terminal). */
function closedCount(r: StrategyRule): number {
  return (r.win_count ?? 0) + (r.loss_count ?? 0);
}

export interface RuleLiveCounts {
  open: number;
  pending: number;
}

export interface RulesViewProps {
  /** Lab-only dry-run render-prop forwarded to the editor (FE3). */
  renderDryRun?: (draft: RuleEditorDraft | null, canRun: boolean) => ReactNode;
  /** Lab-only: show a link icon that opens Simulate with this rule selected. */
  linkToSimulate?: boolean;
  /** Live-only: open/pending counts from the Live Status SSOT (not list_rules). */
  ruleLiveCounts?: Record<string, RuleLiveCounts>;
  /** Live-only: show scoreboard columns from rule-list wire (PnL, Avg%, Exp, Win%, W/L, N). */
  showScores?: boolean;
  /** Live-only: Analyze panel under the table for the selected rule (row click). */
  renderAnalyze?: (ctx: {
    ruleId: string;
    rule: StrategyRule;
    clear: () => void;
  }) => ReactNode;
}

/**
 * Rules list + editor, shared by the live and lab apps. One list for all generic
 * rules (mode/status badges) with two row-action columns — **Actions** (edit /
 * duplicate / disable / delete) and **Execute** (Activate → running rule, then
 * Pause / Stop; Enable when Disabled) — plus header **Pause All / Stop All** bulk
 * actions scoped per trade mode. Disabled (`is_enabled=false`) rules are soft-
 * archived: hidden by default, kept in the DB. The editor opens in an xl modal.
 * The lab app injects a dry-run panel via `renderDryRun`.
 */
export function RulesView({
  renderDryRun,
  linkToSimulate,
  ruleLiveCounts,
  showScores,
  renderAnalyze,
}: RulesViewProps) {
  const { data: rules = [], isLoading, refetch } = useGetStrategyRulesQuery();
  const { data: fps = [] } = useGetFingerprintsQuery();
  const [selectedKey, setSelectedKey] = useSelectionSearchParam(STRATEGY_PARAMS.rule);
  const selectedRule = useMemo(
    () => (selectedKey ? rules.find((r) => r.id === selectedKey) : undefined),
    [rules, selectedKey],
  );

  const actions = useRuleActions({ renderDryRun });

  const [activate] = useActivateStrategyRuleMutation();
  const [enable] = useEnableStrategyRuleMutation();
  const [disable] = useDisableStrategyRuleMutation();
  const [pause] = usePauseStrategyRuleMutation();
  const [stop] = useStopStrategyRuleMutation();
  const [pauseAll, pauseAllState] = usePauseAllStrategyRulesMutation();
  const [stopAll, stopAllState] = useStopAllStrategyRulesMutation();

  /** Soft-archived rules are hidden by default — toggle to review them. */
  const [showDisabled, setShowDisabled] = useState(false);
  const [opErr, setOpErr] = useState<string | null>(null);
  /** Rule ids mid optimistic pause (label "Pausing…" until SSE/refetch confirms). */
  const [pausingIds, setPausingIds] = useState<Set<string>>(() => new Set());
  /** Per-rule stop progress keyed by rule_id (from action_progress SSE). */
  const [stopByRule, setStopByRule] = useState<Record<string, ActionProgress>>({});
  /** Bulk stop progress keyed by mode (action frames with no rule_id). */
  const [stopByMode, setStopByMode] = useState<Partial<Record<TradeMode, ActionProgress>>>({});

  const bulkBusy = pauseAllState.isLoading || stopAllState.isLoading;

  const fpById = useMemo(() => new Map(fps.map((f) => [f.id, f])), [fps]);

  const disabledCount = useMemo(() => rules.filter((r) => !r.is_enabled).length, [rules]);
  const visibleRules = useMemo(
    () => (showDisabled ? rules : rules.filter((r) => r.is_enabled)),
    [rules, showDisabled],
  );

  // Tint fingerprint cells when ≥2 rules share the same fingerprint_id.
  const fpTints = useMemo(
    () =>
      computeSameValueCellClasses(visibleRules, (r) => r.id, [
        { key: 'fingerprint', valueOf: (r) => r.fingerprint_id || null },
      ]),
    [visibleRules],
  );

  // Active-rule counts per mode drive whether each mode's bulk actions show.
  const activeByMode = useMemo(() => {
    const acc: Record<TradeMode, number> = { paper: 0, real: 0 };
    for (const r of rules) if (r.is_active && r.is_enabled) acc[r.trade_mode] += 1;
    return acc;
  }, [rules]);

  // SSE: live engine broadcasts rule flips; also tracks stop rollups.
  // Pause success clears `pausingIds` itself (lab has no `tpsl_rules_changed`).
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

  const clearPausing = (ids: Iterable<string>) => {
    setPausingIds((prev) => {
      const next = new Set(prev);
      for (const id of ids) next.delete(id);
      return next;
    });
  };

  const pauseRule = (r: StrategyRule) => {
    setPausingIds((prev) => new Set(prev).add(r.id));
    void run(async () => {
      try {
        await pause(r.id).unwrap();
        // Don't wait for `tpsl_rules_changed` — lab never emits it, and live
        // SSE can lag; mutation success + RTK invalidation already flipped Idle.
        clearPausing([r.id]);
      } catch (e) {
        clearPausing([r.id]);
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
      // Lab (and live with nothing open) returns total=0 + immediate done SSE;
      // don't park the row on a "Stopping…" spinner that has no work to track.
      if (res.action_id && (res.total ?? 0) > 0) {
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
    const ids = rules.filter((r) => r.is_active && r.trade_mode === mode).map((r) => r.id);
    if (ids.length === 0) return;
    setPausingIds((prev) => {
      const next = new Set(prev);
      for (const id of ids) next.add(id);
      return next;
    });
    void run(async () => {
      try {
        await pauseAll(mode).unwrap();
        clearPausing(ids);
      } catch (e) {
        clearPausing(ids);
        throw e;
      }
    }, 'Pause all failed');
  };

  const doStopAll = (mode: TradeMode) => {
    const warn =
      mode === 'real'
        ? `Stop ALL active real rules and close every open position? This sends on-chain sells.`
        : `Stop ALL active paper rules and close every open position?`;
    if (!window.confirm(warn)) return;
    void run(async () => {
      const res = await stopAll(mode).unwrap();
      if (res.action_id && (res.total ?? 0) > 0) {
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
      render: (r) => (
        <RuleHoverTip rule={r} fingerprint={fpById.get(r.fingerprint_id)}>
          <div className="flex items-center justify-center gap-1">
            <span className="cursor-default font-medium text-text">{r.rule_name}</span>
            {linkToSimulate && (
              <Link
                to={simulateHref(r.id)}
                title={`Simulate “${r.rule_name}”`}
                aria-label={`Simulate ${r.rule_name}`}
                className="inline-flex shrink-0 rounded p-0.5 text-accent hover:bg-accent/15 hover:text-primary"
                onClick={(e) => e.stopPropagation()}
              >
                <LinkIcon className="h-3.5 w-3.5" />
              </Link>
            )}
          </div>
        </RuleHoverTip>
      ),
      searchValue: (r) => r.rule_name,
    },
    ...(showScores
      ? ([
          {
            key: 'score_pnl',
            label: 'PnL',
            group: 'score',
            render: (r: StrategyRule) => {
              const v = r.total_pnl_sol ?? 0;
              if ((r.total_positions ?? 0) === 0) {
                return <span className="text-text-dim">—</span>;
              }
              return (
                <span
                  className={`tabular-nums text-xs font-semibold ${signedToneClass(v)}`}
                >
                  {v > 0 ? '+' : ''}
                  {v.toFixed(3)}◎
                </span>
              );
            },
            searchValue: (r: StrategyRule) => String(r.total_pnl_sol ?? 0),
            sortValue: (r: StrategyRule) => r.total_pnl_sol ?? 0,
            sortable: true as const,
          },
          {
            key: 'score_avg',
            label: 'Avg%',
            group: 'score',
            render: (r: StrategyRule) => {
              if (closedCount(r) === 0) {
                return <span className="text-text-dim">—</span>;
              }
              const v = r.avg_pnl_pct ?? 0;
              return (
                <span className={`tabular-nums text-xs ${pctGradeClass(v)}`}>
                  {formatSignedPct(v, 1)}
                </span>
              );
            },
            searchValue: (r: StrategyRule) => String(r.avg_pnl_pct ?? 0),
            sortValue: (r: StrategyRule) => r.avg_pnl_pct ?? 0,
            sortable: true as const,
          },
          {
            key: 'score_exp',
            label: 'Exp',
            group: 'score',
            render: (r: StrategyRule) => {
              const n = closedCount(r);
              if (n === 0) {
                return <span className="text-text-dim">—</span>;
              }
              const v = (r.total_pnl_sol ?? 0) / n;
              return (
                <span className={`tabular-nums text-xs font-semibold ${signedToneClass(v)}`}>
                  {v > 0 ? '+' : ''}
                  {v.toFixed(3)}◎
                </span>
              );
            },
            searchValue: (r: StrategyRule) => {
              const n = closedCount(r);
              return n > 0 ? String((r.total_pnl_sol ?? 0) / n) : '';
            },
            sortValue: (r: StrategyRule) => {
              const n = closedCount(r);
              return n > 0 ? (r.total_pnl_sol ?? 0) / n : 0;
            },
            sortable: true as const,
          },
          {
            key: 'score_win',
            label: 'Win%',
            group: 'score',
            render: (r: StrategyRule) => {
              if (closedCount(r) === 0) {
                return <span className="text-text-dim">—</span>;
              }
              const pct = r.win_rate ?? 0;
              return (
                <span className={`tabular-nums text-xs ${winRateGradeClass(pct / 100)}`}>
                  {Math.round(pct)}%
                </span>
              );
            },
            searchValue: (r: StrategyRule) => String(r.win_rate ?? 0),
            sortValue: (r: StrategyRule) => r.win_rate ?? 0,
            sortable: true as const,
          },
          {
            key: 'score_wl',
            label: 'W/L',
            group: 'score',
            render: (r: StrategyRule) => {
              const w = r.win_count ?? 0;
              const l = r.loss_count ?? 0;
              if (w + l === 0) {
                return <span className="text-text-dim">—</span>;
              }
              return (
                <span className="tabular-nums text-xs">
                  <span className="text-green">{w}</span>
                  <span className="text-text-dim">/</span>
                  <span className="text-red">{l}</span>
                </span>
              );
            },
            searchValue: (r: StrategyRule) =>
              `${r.win_count ?? 0}/${r.loss_count ?? 0}`,
            sortValue: (r: StrategyRule) =>
              (r.win_count ?? 0) * 1_000_000 + (r.loss_count ?? 0),
            sortable: true as const,
          },
          {
            key: 'score_n',
            label: 'N',
            group: 'score',
            render: (r: StrategyRule) => {
              const total = r.total_positions ?? 0;
              const closed = closedCount(r);
              if (total === 0) {
                return <span className="text-text-dim">—</span>;
              }
              // Entered total; when some are still open, show closed beside it.
              return (
                <span
                  className="tabular-nums text-xs text-text-mid"
                  title="Entered positions (closed count when any still open)"
                >
                  {total}
                  {closed < total && (
                    <span className="text-text-dim"> · {closed}c</span>
                  )}
                </span>
              );
            },
            searchValue: (r: StrategyRule) => String(r.total_positions ?? 0),
            sortValue: (r: StrategyRule) => r.total_positions ?? 0,
            sortable: true as const,
          },
        ] satisfies ColumnDef<StrategyRule>[])
      : []),
    {
      key: 'status',
      label: 'Status',
      group: 'status',
      render: (r) => {
        if (!r.is_enabled) {
          return <Badge variant="danger">Disabled</Badge>;
        }
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
      searchValue: (r) => (!r.is_enabled ? 'disabled' : r.is_active ? 'active' : 'idle'),
      // Active > Idle > Disabled on desc (the natural first click).
      sortValue: (r) => (!r.is_enabled ? 0 : r.is_active ? 2 : 1),
    },
    ...(ruleLiveCounts
      ? ([
          {
            key: 'live_pos',
            label: 'Live',
            group: 'status',
            render: (r: StrategyRule) => {
              const c = ruleLiveCounts[r.id];
              const open = c?.open ?? 0;
              const pending = c?.pending ?? 0;
              if (open === 0 && pending === 0) {
                return <span className="text-text-dim">—</span>;
              }
              return (
                <span className="tabular-nums text-xs">
                  <span className="text-primary">{open} open</span>
                  {pending > 0 && (
                    <span className="text-text-dim"> · {pending} pend</span>
                  )}
                </span>
              );
            },
            searchValue: (r: StrategyRule) => {
              const c = ruleLiveCounts[r.id];
              return `${c?.open ?? 0} ${c?.pending ?? 0}`;
            },
            sortValue: (r: StrategyRule) =>
              (ruleLiveCounts[r.id]?.open ?? 0) * 1000 + (ruleLiveCounts[r.id]?.pending ?? 0),
            sortable: true as const,
          },
        ] satisfies ColumnDef<StrategyRule>[])
      : []),
    {
      key: 'mode',
      label: 'Mode',
      group: 'status',
      render: (r) => (
        <Badge variant={r.trade_mode === 'real' ? 'warning' : 'info'}>{r.trade_mode}</Badge>
      ),
      searchValue: (r) => r.trade_mode,
      sortValue: (r) => r.trade_mode,
    },
    ...buildFingerprintRuleColumns(fpById, {
      cellClassName: (r) => fpTints.get(`${r.id}\0fingerprint`),
    }),
    {
      key: 'buy',
      label: 'Buy',
      render: (r) => <span className="tabular-nums">{lamportsToSol(r.buy_amount_lamports)}◎</span>,
      searchValue: (r) => String(lamportsToSol(r.buy_amount_lamports)),
      sortValue: (r) => r.buy_amount_lamports,
      sortable: true,
    },
    ...buildCapsColumns(),
    ...buildRuleParamsColumns(),
    {
      key: 'execute',
      label: 'Execute',
      render: (r) => {
        if (!r.is_enabled) {
          return (
            <IconButton
              variant="ghost"
              size="md"
              onClick={() => void run(() => enable(r.id).unwrap(), 'Enable failed')}
              title="Enable — restore this rule to the active list"
              aria-label="Enable — restore this rule to the active list"
            >
              <EnableIcon />
            </IconButton>
          );
        }
        const stopProg = stopByRule[r.id];
        const stopping = !!stopProg && stopProg.status === 'running';
        const pausing = pausingIds.has(r.id);
        // Once inactive (optimistic or server), show Activate — do not keep the
        // Pause/Stop cluster solely because `pausingIds` hasn't cleared yet.
        if (r.is_active || stopping) {
          return (
            <IconButtonGroup>
              <IconButton
                variant="ghost"
                size="md"
                disabled={pausing || stopping}
                onClick={() => pauseRule(r)}
                title={pausing ? 'Pausing…' : 'Pause — stop new entries, let open positions drain'}
                aria-label={pausing ? 'Pausing…' : 'Pause — stop new entries, let open positions drain'}
              >
                {pausing ? <SpinnerIcon /> : <PauseIcon />}
              </IconButton>
              <IconButton
                variant="danger"
                size="md"
                disabled={stopping}
                onClick={() => stopRule(r)}
                title={
                  stopping
                    ? `Stopping ${stopProg!.done}/${stopProg!.total}`
                    : 'Stop — deactivate and force-close open positions now'
                }
                aria-label={
                  stopping
                    ? `Stopping ${stopProg!.done}/${stopProg!.total}`
                    : 'Stop — deactivate and force-close open positions now'
                }
              >
                {stopping ? <SpinnerIcon /> : <StopIcon />}
              </IconButton>
            </IconButtonGroup>
          );
        }
        return (
          <IconButton
            variant="primary"
            size="md"
            onClick={() => void run(() => activate(r.id).unwrap(), 'Activate failed')}
            title="Activate — arm this rule to fire on matching tokens"
            aria-label="Activate — arm this rule to fire on matching tokens"
          >
            <PlayIcon />
          </IconButton>
        );
      },
      searchValue: (r) => (!r.is_enabled ? 'enable' : r.is_active ? 'pause stop' : 'activate'),
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
                <IconButtonGroup>
                  <IconButton
                    variant="ghost"
                    size="md"
                    disabled={bulkBusy || stopping}
                    onClick={() => doPauseAll(mode)}
                    title={`Pause All (${activeByMode[mode]})`}
                    aria-label={`Pause All (${activeByMode[mode]})`}
                  >
                    <PauseIcon />
                  </IconButton>
                  <IconButton
                    variant="danger"
                    size="md"
                    disabled={bulkBusy || stopping}
                    onClick={() => doStopAll(mode)}
                    title={
                      stopping
                        ? `Stopping ${bulkStop!.done}/${bulkStop!.total}`
                        : `Stop All (${activeByMode[mode]})`
                    }
                    aria-label={
                      stopping
                        ? `Stopping ${bulkStop!.done}/${bulkStop!.total}`
                        : `Stop All (${activeByMode[mode]})`
                    }
                  >
                    {stopping ? <SpinnerIcon /> : <StopIcon />}
                  </IconButton>
                </IconButtonGroup>
              </div>
            );
          })}
          {disabledCount > 0 && (
            <label className="flex cursor-pointer items-center gap-1.5 text-[12px] text-text-dim">
              <input
                type="checkbox"
                checked={showDisabled}
                onChange={(e) => setShowDisabled(e.target.checked)}
                className="accent-accent"
              />
              Show disabled ({disabledCount})
            </label>
          )}
          <IconButton
            variant="success"
            size="lg"
            label="New rule"
            title="New rule"
            onClick={actions.openNew}
          >
            <PlusIcon />
          </IconButton>
        </div>
      </div>
      {err && <p className="text-[12px] text-red">{err}</p>}
      <DataTable
        columns={columns}
        rows={visibleRules}
        rowKey={(r) => r.id}
        loading={isLoading}
        searchable
        colFilters
        tableId="strategy-rules"
        emptyMessage="No rules yet — create one from a fingerprint."
        selectedKey={selectedKey}
        onSelect={setSelectedKey}
        rowClassName={disabledRuleRowClass}
        rowActions={(r) => (
          <IconButtonGroup>
            <IconButton
              variant="accent"
              size="md"
              title="Edit"
              aria-label="Edit"
              onClick={() => actions.edit(r)}
            >
              <EditIcon />
            </IconButton>
            <IconButton
              variant="ghost"
              size="md"
              title="Duplicate"
              aria-label="Duplicate"
              onClick={() => actions.duplicate(r)}
            >
              <DuplicateIcon />
            </IconButton>
            {r.is_enabled ? (
              <IconButton
                variant="ghost"
                size="md"
                title="Disable — keep this rule but hide it from the default list"
                aria-label="Disable — keep this rule but hide it from the default list"
                onClick={() => void run(() => disable(r.id).unwrap(), 'Disable failed')}
              >
                <DisableIcon />
              </IconButton>
            ) : (
              <IconButton
                variant="ghost"
                size="md"
                title="Enable — restore this rule to the active list"
                aria-label="Enable — restore this rule to the active list"
                onClick={() => void run(() => enable(r.id).unwrap(), 'Enable failed')}
              >
                <EnableIcon />
              </IconButton>
            )}
            <IconButton
              variant="danger"
              size="md"
              title="Delete"
              aria-label="Delete"
              onClick={() => void actions.remove(r)}
            >
              <TrashIcon />
            </IconButton>
          </IconButtonGroup>
        )}
      />
      {renderAnalyze && selectedKey && selectedRule
        ? renderAnalyze({
            ruleId: selectedKey,
            rule: selectedRule,
            clear: () => setSelectedKey(null),
          })
        : null}
      {actions.editorNode}
    </div>
  );
}
