import { useCallback, useEffect, useMemo, useState } from 'react';
import { Link, useSearchParams } from 'react-router-dom';
import { useSelector } from 'react-redux';
import { DataTable } from 'components/table/DataTable';
import type { ColumnDef } from 'components/table/types';
import { Badge, type BadgeVariant } from 'components/ui/Badge';
import { IconButton } from 'components/ui/IconButton';
import { InlineAlert } from 'components/ui/Modal';
import { StatTile } from 'components/ui/StatTile';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { LinkIcon, SellIcon, SpinnerIcon } from 'components/ui/icons';
import { useSseStatus } from 'hooks/useSseStatus';
import { OPS_PARAMS, rulesHref, type OpsTab } from 'lib/strategy/nav';
import { formatCompact } from 'utils/format';
import { apiErrorMessage } from 'store/apiSlice';
import { ArmedHistoryPanel } from '@live/components/strategy/ArmedHistoryPanel';
import { useCloseRulePositionMutation } from '@live/store/liveEndpoints';
import {
  selectLiveArmed,
  selectLiveOpen,
  selectLiveRecentClosed,
  selectLiveStatusHydrated,
  type LiveArmedRow,
  type LiveClosedRow,
  type LiveOpenRow,
} from '@live/slices/liveStatusSlice';
import type { RootState } from '@live/store';

/** Path segment only validates; armed-history is keyed by `rule_id` in the runtime. */
const ARMED_HISTORY_STRATEGY = 'generic';

type Tab = OpsTab;
type ModeFilter = 'real' | 'paper' | 'all';

const HOLDING_LABEL: Record<string, string> = {
  Arming: 'Arming',
  Armed: 'Armed',
  Disarmed: 'Disarmed',
  BuySubmitted: 'Buy submitted',
  Holding: 'Holding',
  ExitPending: 'Exit pending',
  ExitUnconfirmed: 'Exit unconfirmed',
  End: 'End',
  ExitFailed: 'Exit failed',
};

function statusVariant(status: string): BadgeVariant {
  switch (status) {
    case 'ExitPending':
    case 'ExitUnconfirmed':
      return 'warning';
    case 'Holding':
      return 'success';
    case 'BuySubmitted':
    case 'Arming':
      return 'info';
    case 'End':
      return 'neutral';
    case 'ExitFailed':
      return 'danger';
    default:
      return 'neutral';
  }
}

function fmtAge(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${s % 60}s`;
  return `${Math.floor(m / 60)}h ${m % 60}m`;
}

function modeOk(mode: string | null | undefined, filter: ModeFilter): boolean {
  if (filter === 'all') return true;
  return (mode ?? 'real') === filter;
}

/**
 * Live Ops — the real-time manage cockpit (Waiting / Open / Recent).
 * Reads the Live Status SSOT only; never owns a parallel armed/open Map.
 */
export function OpsPage() {
  const [params, setParams] = useSearchParams();
  const tab = (params.get(OPS_PARAMS.tab) as Tab | null) ?? 'open';
  const modeFilter = (params.get(OPS_PARAMS.mode) as ModeFilter | null) ?? 'real';
  const statusFilter = params.get(OPS_PARAMS.status);
  const mintParam = params.get(OPS_PARAMS.mint);
  const ruleParam = params.get(OPS_PARAMS.rule);
  const positionParam = params.get(OPS_PARAMS.position);

  const setTab = (t: Tab) => {
    // Manual tab change drops notification deep-link selection/filters.
    const next = new URLSearchParams();
    next.set(OPS_PARAMS.tab, t);
    next.set(OPS_PARAMS.mode, modeFilter);
    setParams(next, { replace: true });
  };
  const setMode = (m: ModeFilter) => {
    const next = new URLSearchParams(params);
    next.set(OPS_PARAMS.mode, m);
    setParams(next, { replace: true });
  };
  const clearDeepLink = () => {
    const next = new URLSearchParams();
    next.set(OPS_PARAMS.tab, tab);
    next.set(OPS_PARAMS.mode, modeFilter);
    setParams(next, { replace: true });
  };

  const sse = useSseStatus();
  const sseLive = sse === 'open';
  const hydrated = useSelector(selectLiveStatusHydrated);
  const armedMap = useSelector(selectLiveArmed);
  const openMap = useSelector(selectLiveOpen);
  const recent = useSelector(selectLiveRecentClosed);
  const snapshotLoading = useSelector((s: RootState) => s.liveStatus.snapshotLoading);

  const [closePosition] = useCloseRulePositionMutation();
  const [sellingId, setSellingId] = useState<string | null>(null);
  const [sellErr, setSellErr] = useState<string | null>(null);

  const armedRows = useMemo(
    () => Object.values(armedMap).filter((r) => modeOk(r.tradeMode, modeFilter)),
    [armedMap, modeFilter],
  );
  const openRows = useMemo(
    () => Object.values(openMap).filter((r) => modeOk(r.mode, modeFilter)),
    [openMap, modeFilter],
  );
  const recentRows = useMemo(
    () => recent.filter((r) => modeOk(r.mode, modeFilter)),
    [recent, modeFilter],
  );
  // Notification deep-link may narrow the table by status without changing KPI counts.
  const openTableRows = useMemo(
    () => (statusFilter ? openRows.filter((r) => r.status === statusFilter) : openRows),
    [openRows, statusFilter],
  );
  const recentTableRows = useMemo(
    () => (statusFilter ? recentRows.filter((r) => r.status === statusFilter) : recentRows),
    [recentRows, statusFilter],
  );

  const selectedKey =
    tab === 'waiting'
      ? ruleParam && mintParam
        ? `${ruleParam}|${mintParam}`
        : null
      : positionParam;

  const deepLinkActive = Boolean(statusFilter || mintParam || positionParam || ruleParam);

  const exitPending = openRows.filter((r) => r.status === 'ExitPending').length;
  const totalDeployed = openRows.reduce((s, r) => s + (r.entrySol ?? 0), 0);

  const onSell = useCallback(
    async (row: LiveOpenRow) => {
      if (
        !window.confirm(
          `Sell ALL of this position (${row.mint.slice(0, 8)}…)? REAL mode sends an on-chain sell.`,
        )
      )
        return;
      setSellErr(null);
      setSellingId(row.positionId);
      try {
        await closePosition({
          strategy: row.strategyId || 'generic',
          positionId: row.positionId,
        }).unwrap();
      } catch (e) {
        setSellingId(null);
        setSellErr(apiErrorMessage(e as never) ?? 'Sell failed');
      }
    },
    [closePosition],
  );

  useEffect(() => {
    if (!sellingId) return;
    const row = openMap[sellingId];
    if (!row || row.status === 'ExitPending' || row.status === 'End') {
      setSellingId(null);
    }
  }, [sellingId, openMap]);

  const ruleLink = (ruleId: string | null, name: string | null) => {
    if (!ruleId) return <span className="text-text-dim">—</span>;
    const label = name ?? ruleId.slice(0, 8);
    return (
      <Link
        to={rulesHref(ruleId)}
        className="inline-flex items-center gap-0.5 text-accent hover:text-primary hover:underline"
        onClick={(e) => e.stopPropagation()}
      >
        <span>{label}</span>
        <LinkIcon className="h-3.5 w-3.5 shrink-0" />
      </Link>
    );
  };

  const waitingCols: ColumnDef<LiveArmedRow>[] = [
    {
      key: 'mint',
      label: 'Token',
      render: (r) => <AddressDisplay address={r.mint} kind="token" />,
      searchValue: (r) => r.mint,
    },
    {
      key: 'rule',
      label: 'Rule',
      render: (r) => ruleLink(r.ruleId, r.ruleName),
      searchValue: (r) => r.ruleName ?? r.ruleId,
    },
    {
      key: 'status',
      label: 'Status',
      render: () => <Badge variant="info">Waiting for entry</Badge>,
      searchValue: () => 'waiting',
    },
    {
      key: 'age',
      label: 'Age',
      sortable: true,
      render: (r) => (
        <span className="tabular-nums text-text-dim">{fmtAge(Date.now() - r.armedAt)}</span>
      ),
      sortValue: (r) => r.armedAt,
      searchValue: () => '',
    },
    {
      key: 'trade',
      label: '',
      width: '64px',
      render: (r) => (
        <Link
          to={`/trade?mint=${encodeURIComponent(r.mint)}`}
          className="text-[11px] font-semibold text-accent hover:text-primary hover:underline"
          onClick={(e) => e.stopPropagation()}
        >
          Trade
        </Link>
      ),
      searchValue: () => '',
    },
  ];

  const openCols: ColumnDef<LiveOpenRow>[] = [
    {
      key: 'mint',
      label: 'Token',
      render: (r) => <AddressDisplay address={r.mint} kind="token" />,
      searchValue: (r) => r.mint,
    },
    {
      key: 'rule',
      label: 'Rule',
      render: (r) => ruleLink(r.ruleId, r.ruleName),
      searchValue: (r) => r.ruleName ?? r.ruleId ?? '',
    },
    {
      key: 'status',
      label: 'Status',
      sortable: true,
      render: (r) => {
        const selling = sellingId === r.positionId || r.status === 'ExitPending';
        return (
          <Badge variant={selling ? 'warning' : statusVariant(r.status)}>
            {selling && r.status !== 'ExitPending'
              ? 'Selling…'
              : (HOLDING_LABEL[r.status] ?? r.status)}
          </Badge>
        );
      },
      sortValue: (r) => r.status,
      searchValue: (r) => HOLDING_LABEL[r.status] ?? r.status,
    },
    {
      key: 'entry',
      label: 'Entry ◎',
      sortable: true,
      render: (r) => (
        <span className="tabular-nums">
          {r.entrySol != null ? formatCompact(r.entrySol, 3) : '—'}
        </span>
      ),
      sortValue: (r) => r.entrySol ?? -1,
      searchValue: (r) => String(r.entrySol ?? ''),
    },
    {
      key: 'price',
      label: 'Entry price',
      render: (r) => <span className="tabular-nums">{r.entryPrice ?? '—'}</span>,
      searchValue: (r) => String(r.entryPrice ?? ''),
    },
    {
      key: 'actions',
      label: '',
      width: '140px',
      render: (r) => {
        const busy =
          sellingId === r.positionId || r.status === 'ExitPending' || r.status === 'BuySubmitted';
        const canSell = sseLive && r.status === 'Holding' && r.mode === 'real';
        return (
          <div className="flex items-center gap-2" onClick={(e) => e.stopPropagation()}>
            <IconButton
              variant="danger"
              size="md"
              disabled={!canSell || busy}
              onClick={() => void onSell(r)}
              title={
                !sseLive
                  ? 'SSE not live — status may be stale'
                  : canSell
                    ? 'Sell ALL — force-close this strategy position'
                    : 'Sell only when Holding (real)'
              }
              aria-label="Sell ALL"
            >
              {busy && r.status !== 'Holding' ? <SpinnerIcon /> : <SellIcon />}
            </IconButton>
            <Link
              to={`/trade?mint=${encodeURIComponent(r.mint)}`}
              className="text-[11px] font-semibold text-accent hover:text-primary hover:underline"
            >
              Trade
            </Link>
          </div>
        );
      },
      searchValue: () => '',
    },
  ];

  const recentCols: ColumnDef<LiveClosedRow>[] = [
    {
      key: 'mint',
      label: 'Token',
      render: (r) => <AddressDisplay address={r.mint} kind="token" />,
      searchValue: (r) => r.mint,
    },
    {
      key: 'rule',
      label: 'Rule',
      render: (r) => ruleLink(r.ruleId, r.ruleName),
      searchValue: (r) => r.ruleName ?? r.ruleId ?? '',
    },
    {
      key: 'status',
      label: 'Status',
      render: (r) => <Badge variant={statusVariant(r.status)}>{r.status}</Badge>,
      searchValue: (r) => r.status,
    },
    {
      key: 'exit',
      label: 'Exit reason',
      render: (r) => <span className="text-text-dim">{r.exitReason ?? '—'}</span>,
      searchValue: (r) => r.exitReason ?? '',
    },
    {
      key: 'when',
      label: 'Closed',
      sortable: true,
      render: (r) => (
        <span className="tabular-nums text-text-dim">{fmtAge(Date.now() - r.closedAt)} ago</span>
      ),
      sortValue: (r) => r.closedAt,
      searchValue: () => '',
    },
  ];

  const tabs: { id: Tab; label: string; count: number }[] = [
    { id: 'waiting', label: 'Waiting', count: armedRows.length },
    { id: 'open', label: 'Open', count: openRows.length },
    { id: 'recent', label: 'Recent', count: recentRows.length },
  ];

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-baseline justify-between gap-3">
        <div className="flex flex-wrap items-baseline gap-3">
          <h1 className="text-lg font-extrabold text-text">Ops</h1>
          <span className="text-sm text-text-mid">
            Live manage · Waiting + Open + session closes (Analyze = per-rule history)
          </span>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Badge variant={sseLive ? 'success' : sse === 'connecting' ? 'info' : 'warning'}>
            SSE {sse === 'open' ? 'Live' : sse === 'connecting' ? 'Connecting…' : 'Reconnecting…'}
          </Badge>
          {!sseLive && (
            <span className="text-[11px] text-warning">Status may be stale — sells disabled</span>
          )}
        </div>
      </div>

      {sellErr && <InlineAlert variant="error">{sellErr}</InlineAlert>}

      <div className="flex flex-wrap items-center gap-2">
        {(['real', 'paper', 'all'] as ModeFilter[]).map((m) => (
          <button
            key={m}
            type="button"
            onClick={() => setMode(m)}
            className={`rounded-md px-2.5 py-1 text-xs font-semibold capitalize ${
              modeFilter === m
                ? 'bg-primary/20 text-primary'
                : 'bg-white/5 text-text-dim hover:bg-white/8'
            }`}
          >
            {m}
          </button>
        ))}
      </div>

      <div className="grid grid-cols-2 gap-2.5 sm:grid-cols-4">
        <StatTile label="Waiting" value={armedRows.length} tone="primary" />
        <StatTile
          label="Open"
          value={openRows.length}
          sub={`◎${formatCompact(totalDeployed, 2)} deployed`}
          tone="green"
        />
        <StatTile
          label="Exit pending"
          value={exitPending}
          tone={exitPending > 0 ? 'red' : 'muted'}
        />
        <StatTile
          label="Recent closes"
          value={recentRows.length}
          sub="this session"
          tone="muted"
        />
      </div>

      <div className="flex gap-1 border-b border-white/8">
        {tabs.map((t) => (
          <button
            key={t.id}
            type="button"
            onClick={() => setTab(t.id)}
            className={`-mb-px border-b-2 px-3 py-2 text-xs font-semibold ${
              tab === t.id
                ? 'border-primary text-primary'
                : 'border-transparent text-text-dim hover:text-text'
            }`}
          >
            {t.label} ({t.count})
          </button>
        ))}
      </div>

      {deepLinkActive && (
        <div className="flex flex-wrap items-center gap-2 rounded-lg border border-accent/25 bg-accent/8 px-3 py-2 text-xs text-text">
          <span>
            From notification
            {statusFilter ? (
              <>
                {' '}
                · status <span className="font-semibold">{HOLDING_LABEL[statusFilter] ?? statusFilter}</span>
              </>
            ) : null}
            {mintParam ? (
              <>
                {' '}
                · mint <span className="font-mono">{mintParam.slice(0, 8)}…</span>
              </>
            ) : null}
          </span>
          <button
            type="button"
            onClick={clearDeepLink}
            className="ml-auto text-[11px] font-semibold text-accent underline underline-offset-2 hover:text-primary"
          >
            Clear
          </button>
        </div>
      )}

      {!hydrated && snapshotLoading ? (
        <p className="py-10 text-center text-text-dim">Loading live status…</p>
      ) : tab === 'waiting' ? (
        <>
          <DataTable
            columns={waitingCols}
            rows={armedRows}
            rowKey={(r) => r.key}
            searchable
            defaultSort={{ col: 'age', dir: 'desc' }}
            tableId="ops-waiting"
            emptyMessage="Nothing waiting on entry."
            selectedKey={selectedKey}
            onSelect={(key) => {
              if (!key) {
                clearDeepLink();
                return;
              }
              const row = armedRows.find((r) => r.key === key);
              if (!row) return;
              const next = new URLSearchParams(params);
              next.set(OPS_PARAMS.mint, row.mint);
              next.set(OPS_PARAMS.rule, row.ruleId);
              setParams(next, { replace: true });
            }}
          />
          <ArmedHistoryPanel
            strategy={ARMED_HISTORY_STRATEGY}
            selectedRuleId={ruleParam}
          />
        </>
      ) : tab === 'open' ? (
        <DataTable
          columns={openCols}
          rows={openTableRows}
          rowKey={(r) => r.positionId}
          searchable
          colFilters
          tableId="ops-open"
          emptyMessage="No open positions."
          selectedKey={selectedKey}
          onSelect={(key) => {
            if (!key) {
              clearDeepLink();
              return;
            }
            const row = openTableRows.find((r) => r.positionId === key);
            if (!row) return;
            const next = new URLSearchParams(params);
            next.set(OPS_PARAMS.position, row.positionId);
            next.set(OPS_PARAMS.mint, row.mint);
            if (row.ruleId) next.set(OPS_PARAMS.rule, row.ruleId);
            setParams(next, { replace: true });
          }}
        />
      ) : (
        <DataTable
          columns={recentCols}
          rows={recentTableRows}
          rowKey={(r) => r.positionId}
          searchable
          defaultSort={{ col: 'when', dir: 'desc' }}
          tableId="ops-recent"
          emptyMessage="No closes this session yet."
          selectedKey={selectedKey}
          onSelect={(key) => {
            if (!key) {
              clearDeepLink();
              return;
            }
            const row = recentTableRows.find((r) => r.positionId === key);
            if (!row) return;
            const next = new URLSearchParams(params);
            next.set(OPS_PARAMS.position, row.positionId);
            next.set(OPS_PARAMS.mint, row.mint);
            if (row.ruleId) next.set(OPS_PARAMS.rule, row.ruleId);
            setParams(next, { replace: true });
          }}
        />
      )}
    </div>
  );
}
