import { useCallback, useEffect, useMemo, useState } from 'react';
import { Link, useSearchParams } from 'react-router-dom';
import { useSelector } from 'react-redux';
import { DataTable } from 'components/table/DataTable';
import type { ColumnDef } from 'components/table/types';
import { Badge, type BadgeVariant } from 'components/ui/Badge';
import { IconButton } from 'components/ui/IconButton';
import { InlineAlert } from 'components/ui/Modal';
import { PageHeader } from 'components/ui/PageHeader';
import { StatTile } from 'components/ui/StatTile';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { LinkIcon, SellIcon, SpinnerIcon } from 'components/ui/icons';
import { useSseStatus } from 'hooks/useSseStatus';
import {
  OPS_PARAMS,
  portfolioHref,
  rulesHref,
  type OpsTab,
} from 'lib/strategy/nav';
import { formatCompact } from 'utils/format';
import {
  formatSigned,
  formatSignedPct,
  pctGradeClass,
  signedToneClass,
} from 'lib/signedTone';
import { resolvePnlPct } from 'lib/pnlPct';
import { apiErrorMessage } from 'store/apiSlice';
import { ArmedHistoryPanel } from '@live/components/strategy/ArmedHistoryPanel';
import { FloorBookStrip } from '@live/components/floor/FloorBookStrip';
import { FloorPositionDetail } from '@live/components/floor/FloorPositionDetail';
import { FloorMintChart } from '@live/components/floor/FloorMintChart';
import {
  useCloseRulePositionMutation,
  useGetPortfolioHoldingsQuery,
} from '@live/store/liveEndpoints';
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
import type { WalletHolding } from 'types';

/** Path segment only validates; armed-history is keyed by `rule_id` in the runtime. */
const ARMED_HISTORY_STRATEGY = 'generic';

const ATTENTION_STATUSES = new Set(['ExitPending', 'ExitFailed', 'ExitUnconfirmed']);

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
 * Floor — live book across rules (Waiting / Open / Needs attention / Recent).
 * Manage inventory + sell; full run history lives on Rules Evidence.
 * Canonical route is `/floor` (`/ops` and legacy paths redirect here).
 */
export function OpsPage() {
  const [params, setParams] = useSearchParams();
  const rawTab = params.get(OPS_PARAMS.tab) as Tab | null;
  const tab: Tab =
    rawTab === 'waiting' ||
    rawTab === 'open' ||
    rawTab === 'attention' ||
    rawTab === 'recent'
      ? rawTab
      : 'open';
  const modeFilter = (params.get(OPS_PARAMS.mode) as ModeFilter | null) ?? 'real';
  const statusFilter = params.get(OPS_PARAMS.status);
  const mintParam = params.get(OPS_PARAMS.mint);
  const ruleParam = params.get(OPS_PARAMS.rule);
  const positionParam = params.get(OPS_PARAMS.position);

  const setTab = (t: Tab) => {
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

  // Bag marks (SSOT unrealized_pnl) — join by mint for real open MTM / PnL%.
  const { data: holdings = [] } = useGetPortfolioHoldingsQuery();
  const holdingByMint = useMemo(() => {
    const m = new Map<string, WalletHolding>();
    for (const h of holdings) {
      if (h.asset_kind === 'cash' || h.asset_kind === 'wrapped_sol') continue;
      m.set(h.mint_address, h);
    }
    return m;
  }, [holdings]);

  const openMark = useCallback(
    (row: LiveOpenRow): { mtmSol: number | null; mtmPct: number | null } => {
      if (row.mode !== 'real') return { mtmSol: null, mtmPct: null };
      const h = holdingByMint.get(row.mint);
      if (!h) return { mtmSol: null, mtmPct: null };
      return {
        mtmSol: h.unrealized_pnl_sol ?? null,
        mtmPct: h.unrealized_pnl_pct ?? null,
      };
    },
    [holdingByMint],
  );

  const armedRows = useMemo(
    () => Object.values(armedMap).filter((r) => modeOk(r.tradeMode, modeFilter)),
    [armedMap, modeFilter],
  );
  const openRows = useMemo(
    () => Object.values(openMap).filter((r) => modeOk(r.mode, modeFilter)),
    [openMap, modeFilter],
  );
  const attentionRows = useMemo(
    () => openRows.filter((r) => ATTENTION_STATUSES.has(r.status)),
    [openRows],
  );
  const holdingOpenRows = useMemo(
    () => openRows.filter((r) => !ATTENTION_STATUSES.has(r.status)),
    [openRows],
  );
  const recentRows = useMemo(
    () => recent.filter((r) => modeOk(r.mode, modeFilter)),
    [recent, modeFilter],
  );

  const openTableSource = tab === 'attention' ? attentionRows : holdingOpenRows;
  const openTableRows = useMemo(() => {
    let rows = openTableSource;
    if (ruleParam) rows = rows.filter((r) => r.ruleId === ruleParam);
    if (statusFilter) rows = rows.filter((r) => r.status === statusFilter);
    return rows;
  }, [openTableSource, statusFilter, ruleParam]);
  const recentTableRows = useMemo(() => {
    let rows = recentRows;
    if (ruleParam) rows = rows.filter((r) => r.ruleId === ruleParam);
    if (statusFilter) rows = rows.filter((r) => r.status === statusFilter);
    return rows;
  }, [recentRows, statusFilter, ruleParam]);
  const waitingTableRows = useMemo(() => {
    if (!ruleParam) return armedRows;
    return armedRows.filter((r) => r.ruleId === ruleParam);
  }, [armedRows, ruleParam]);

  const selectedKey = positionParam;

  // Armed-history context follows the selected waiting row, independent of the
  // `rule` deep-link filter (which must stay notification-only — see onSelect
  // below). Cheap lookup over the small in-memory armed set.
  const selectedArmedRuleId = useMemo(
    () => armedRows.find((r) => r.key === positionParam)?.ruleId ?? null,
    [armedRows, positionParam],
  );

  const deepLinkActive = Boolean(statusFilter || mintParam || positionParam || ruleParam);

  const exitPending = attentionRows.length;
  const totalDeployed = openRows.reduce((s, r) => s + (r.entrySol ?? 0), 0);

  const openMtmByRule = useMemo(() => {
    const acc = new Map<string, { label: string; value: number }>();
    for (const r of holdingOpenRows) {
      const { mtmSol } = openMark(r);
      if (mtmSol == null || !Number.isFinite(mtmSol)) continue;
      const key = r.ruleId ?? r.positionId;
      const label = r.ruleName ?? (r.ruleId ? r.ruleId.slice(0, 8) : r.mint.slice(0, 8));
      const prev = acc.get(key);
      acc.set(key, { label, value: (prev?.value ?? 0) + mtmSol });
    }
    return [...acc.entries()]
      .map(([key, v]) => ({ key, label: v.label, value: v.value }))
      .sort((a, b) => b.value - a.value);
  }, [holdingOpenRows, openMark]);

  const recentPnlByRule = useMemo(() => {
    const acc = new Map<string, { label: string; value: number }>();
    for (const r of recentRows) {
      if (r.pnlSol == null || !Number.isFinite(r.pnlSol)) continue;
      const key = r.ruleId ?? r.positionId;
      const label = r.ruleName ?? (r.ruleId ? r.ruleId.slice(0, 8) : r.mint.slice(0, 8));
      const prev = acc.get(key);
      acc.set(key, { label, value: (prev?.value ?? 0) + r.pnlSol });
    }
    return [...acc.entries()]
      .map(([key, v]) => ({ key, label: v.label, value: v.value }))
      .sort((a, b) => b.value - a.value);
  }, [recentRows]);

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
        const res = await closePosition({
          strategy: row.strategyId || 'generic',
          positionId: row.positionId,
        }).unwrap();
        if (res && 'closed' in res && res.closed) {
          setSellingId(null);
        }
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
    if (
      !row ||
      row.status === 'ExitPending' ||
      row.status === 'End' ||
      row.status === 'ExitFailed' ||
      row.status === 'ExitUnconfirmed'
    ) {
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
        title="Open Rules Evidence"
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

  const waitingDetail = (r: LiveArmedRow) => (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-3 text-[11px]">
        <span className="text-text-dim">
          Waiting for entry · armed{' '}
          <span className="tabular-nums text-text">{fmtAge(Date.now() - r.armedAt)}</span>
        </span>
        {ruleLink(r.ruleId, r.ruleName)}
        <Link
          to={`/trade?mint=${encodeURIComponent(r.mint)}`}
          className="font-semibold text-accent hover:underline"
          onClick={(e) => e.stopPropagation()}
        >
          Trade
        </Link>
      </div>
      <FloorMintChart mint={r.mint} tableId="floor-waiting" />
    </div>
  );

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
      key: 'age',
      label: 'Age',
      sortable: true,
      render: (r) => {
        const t = r.entryTime ? Date.parse(r.entryTime) : r.updatedAt;
        return (
          <span className="tabular-nums text-text-dim">
            {Number.isFinite(t) ? fmtAge(Date.now() - t) : '—'}
          </span>
        );
      },
      sortValue: (r) => (r.entryTime ? Date.parse(r.entryTime) : r.updatedAt),
      searchValue: () => '',
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
      key: 'mtm',
      label: 'MTM ◎',
      sortable: true,
      render: (r) => {
        const { mtmSol } = openMark(r);
        return mtmSol != null ? (
          <span className={`tabular-nums text-xs font-semibold ${signedToneClass(mtmSol)}`}>
            {formatSigned(mtmSol, 3)}
          </span>
        ) : (
          <span className="text-text-dim">—</span>
        );
      },
      sortValue: (r) => openMark(r).mtmSol ?? 0,
      searchValue: (r) => String(openMark(r).mtmSol ?? ''),
    },
    {
      key: 'pnl_pct',
      label: 'PnL%',
      sortable: true,
      render: (r) => {
        const { mtmPct } = openMark(r);
        return mtmPct != null ? (
          <span className={`tabular-nums text-xs ${pctGradeClass(mtmPct)}`}>
            {formatSignedPct(mtmPct, 1)}
          </span>
        ) : (
          <span className="text-text-dim">—</span>
        );
      },
      sortValue: (r) => openMark(r).mtmPct ?? 0,
      searchValue: (r) => String(openMark(r).mtmPct ?? ''),
    },
    {
      key: 'actions',
      label: '',
      width: '140px',
      render: (r) => {
        const busy =
          sellingId === r.positionId || r.status === 'ExitPending' || r.status === 'BuySubmitted';
        const canSell =
          sseLive &&
          r.mode === 'real' &&
          (r.status === 'Holding' || r.status === 'ExitFailed');
        return (
          <div className="flex items-center gap-2" onClick={(e) => e.stopPropagation()}>
            <IconButton
              variant="danger"
              size="md"
              label="Sell"
              disabled={!canSell || busy}
              onClick={() => void onSell(r)}
              title={
                !sseLive
                  ? 'SSE not live — status may be stale'
                  : r.status === 'ExitFailed'
                    ? 'Retry sell — prior exit failed; bag may still be in wallet'
                    : canSell
                      ? 'Sell ALL — force-close this strategy position'
                      : 'Sell only when Holding or ExitFailed (real)'
              }
              aria-label="Sell ALL"
            >
              {busy && r.status !== 'Holding' && r.status !== 'ExitFailed' ? (
                <SpinnerIcon />
              ) : (
                <SellIcon />
              )}
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

  const openDetail = (r: LiveOpenRow) => {
    const { mtmSol, mtmPct } = openMark(r);
    const entryMs = r.entryTime ? Date.parse(r.entryTime) : NaN;
    return (
      <FloorPositionDetail
        facts={{
          mint: r.mint,
          ruleId: r.ruleId,
          ruleName: r.ruleName,
          mode: r.mode,
          status: HOLDING_LABEL[r.status] ?? r.status,
          entrySol: r.entrySol,
          entryPrice: r.entryPrice,
          holdLabel: Number.isFinite(entryMs) ? fmtAge(Date.now() - entryMs) : null,
          mtmSol,
          pnlPct: mtmPct,
          inspect: {
            mint_address: r.mint,
            entryTime: r.entryTime,
            entryPrice: r.entryPrice,
            exitTime: null,
            exitPrice: null,
          },
        }}
      />
    );
  };

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
      key: 'pnl',
      label: 'PnL',
      sortable: true,
      render: (r) =>
        r.pnlSol != null ? (
          <span className={`tabular-nums text-xs font-semibold ${signedToneClass(r.pnlSol)}`}>
            {formatSigned(r.pnlSol, 3)}◎
          </span>
        ) : (
          <span className="text-text-dim">—</span>
        ),
      sortValue: (r) => r.pnlSol ?? 0,
      searchValue: (r) => String(r.pnlSol ?? ''),
    },
    {
      key: 'pnl_pct',
      label: 'PnL%',
      sortable: true,
      render: (r) => {
        const pct = resolvePnlPct({
          pnlSol: r.pnlSol,
          entrySol: r.entrySol,
          entryPrice: r.entryPrice,
          exitPrice: r.exitPrice,
        });
        return pct != null ? (
          <span className={`tabular-nums text-xs ${pctGradeClass(pct)}`}>
            {formatSignedPct(pct, 1)}
          </span>
        ) : (
          <span className="text-text-dim">—</span>
        );
      },
      sortValue: (r) =>
        resolvePnlPct({
          pnlSol: r.pnlSol,
          entrySol: r.entrySol,
          entryPrice: r.entryPrice,
          exitPrice: r.exitPrice,
        }) ?? 0,
      searchValue: (r) =>
        String(
          resolvePnlPct({
            pnlSol: r.pnlSol,
            entrySol: r.entrySol,
            entryPrice: r.entryPrice,
            exitPrice: r.exitPrice,
          }) ?? '',
        ),
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
      key: 'hold',
      label: 'Hold',
      sortable: true,
      render: (r) => {
        const entryMs = r.entryTime ? Date.parse(r.entryTime) : NaN;
        if (!Number.isFinite(entryMs)) return <span className="text-text-dim">—</span>;
        return (
          <span className="tabular-nums text-text-dim">{fmtAge(r.closedAt - entryMs)}</span>
        );
      },
      sortValue: (r) => {
        const entryMs = r.entryTime ? Date.parse(r.entryTime) : NaN;
        return Number.isFinite(entryMs) ? r.closedAt - entryMs : 0;
      },
      searchValue: () => '',
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

  const recentDetail = (r: LiveClosedRow) => {
    const pct = resolvePnlPct({
      pnlSol: r.pnlSol,
      entrySol: r.entrySol,
      entryPrice: r.entryPrice,
      exitPrice: r.exitPrice,
    });
    const entryMs = r.entryTime ? Date.parse(r.entryTime) : NaN;
    return (
      <FloorPositionDetail
        facts={{
          mint: r.mint,
          ruleId: r.ruleId,
          ruleName: r.ruleName,
          mode: r.mode,
          status: r.status,
          entrySol: r.entrySol,
          entryPrice: r.entryPrice,
          exitPrice: r.exitPrice,
          holdLabel: Number.isFinite(entryMs) ? fmtAge(r.closedAt - entryMs) : null,
          pnlSol: r.pnlSol,
          pnlPct: pct,
          inspect: {
            mint_address: r.mint,
            entryTime: r.entryTime,
            entryPrice: r.entryPrice,
            exitTime: r.exitTime ?? new Date(r.closedAt).toISOString(),
            exitPrice: r.exitPrice,
            exitLabel: r.exitReason,
          },
        }}
      />
    );
  };

  const tabs: { id: Tab; label: string; count: number }[] = [
    { id: 'open', label: 'Open', count: holdingOpenRows.length },
    { id: 'waiting', label: 'Waiting', count: armedRows.length },
    { id: 'attention', label: 'Needs attention', count: attentionRows.length },
    { id: 'recent', label: 'Recent', count: recentRows.length },
  ];

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Floor"
        description={
          <>
            Live book · sell / triage ·{' '}
            <Link to={rulesHref()} className="text-accent hover:underline">
              Rules Evidence
            </Link>
            {' · '}
            <Link to={portfolioHref('today')} className="text-accent hover:underline">
              Portfolio
            </Link>
          </>
        }
        actions={
          <div className="flex flex-wrap items-center gap-2">
            <Badge variant={sseLive ? 'success' : sse === 'connecting' ? 'info' : 'warning'}>
              SSE {sse === 'open' ? 'Live' : sse === 'connecting' ? 'Connecting…' : 'Reconnecting…'}
            </Badge>
            {!sseLive && (
              <span className="text-xs text-warning">Status may be stale — sells disabled</span>
            )}
          </div>
        }
      />

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
          value={holdingOpenRows.length}
          sub={`◎${formatCompact(totalDeployed, 2)} deployed`}
          tone="green"
        />
        <StatTile
          label="Needs attention"
          value={exitPending}
          tone={exitPending > 0 ? 'red' : 'muted'}
        />
        <StatTile
          label="Recent"
          value={recentRows.length}
          sub="last closes · not full history"
          tone="muted"
        />
      </div>

      <div className="flex flex-wrap gap-1 border-b border-white/8">
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
                · status{' '}
                <span className="font-semibold">{HOLDING_LABEL[statusFilter] ?? statusFilter}</span>
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
            rows={waitingTableRows}
            rowKey={(r) => r.key}
            searchable
            defaultSort={{ col: 'age', dir: 'desc' }}
            tableId="floor-waiting"
            emptyMessage="Nothing waiting on entry."
            selectedKey={selectedKey}
            rowDetail={waitingDetail}
            onSelect={(key) => {
              if (!key) {
                clearDeepLink();
                return;
              }
              const row = waitingTableRows.find((r) => r.key === key);
              if (!row) return;
              const next = new URLSearchParams(params);
              next.set(OPS_PARAMS.position, row.key);
              next.set(OPS_PARAMS.mint, row.mint);
              setParams(next, { replace: true });
            }}
          />
          <ArmedHistoryPanel
            strategy={ARMED_HISTORY_STRATEGY}
            selectedRuleId={selectedArmedRuleId}
          />
        </>
      ) : tab === 'recent' ? (
        <>
          <FloorBookStrip
            title="Recent PnL by rule"
            rows={recentPnlByRule}
            empty="No recent closes with PnL yet."
          />
          <DataTable
            columns={recentCols}
            rows={recentTableRows}
            rowKey={(r) => r.positionId}
            searchable
            defaultSort={{ col: 'when', dir: 'desc' }}
            tableId="floor-recent"
            emptyMessage="No recent closes — full history is on Rules Evidence."
            selectedKey={selectedKey}
            rowDetail={recentDetail}
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
              setParams(next, { replace: true });
            }}
          />
        </>
      ) : (
        <>
          {tab === 'open' && (
            <FloorBookStrip
              title="Open MTM by rule"
              rows={openMtmByRule}
              empty="No bag marks yet — real holdings need a wallet refresh."
            />
          )}
          <DataTable
            columns={openCols}
            rows={openTableRows}
            rowKey={(r) => r.positionId}
            searchable
            colFilters
            tableId={tab === 'attention' ? 'floor-attention' : 'floor-open'}
            emptyMessage={
              tab === 'attention'
                ? 'Nothing stuck — no ExitPending / ExitFailed / ExitUnconfirmed.'
                : 'No open positions.'
            }
            selectedKey={selectedKey}
            rowDetail={openDetail}
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
              setParams(next, { replace: true });
            }}
          />
        </>
      )}
    </div>
  );
}
