import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Link, useSearchParams } from 'react-router-dom';
import { useSelector } from 'react-redux';
import { DataTable } from 'components/table/DataTable';
import type { ColumnDef } from 'components/table/types';
import { Badge, type BadgeVariant } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { IconButton } from 'components/ui/IconButton';
import { Input } from 'components/ui/Input';
import { InlineAlert, Modal } from 'components/ui/Modal';
import { PageHeader } from 'components/ui/PageHeader';
import { StatTile } from 'components/ui/StatTile';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { BuyIcon, LinkIcon, SellIcon, SpinnerIcon } from 'components/ui/icons';
import { ModeBadge } from 'components/strategy/ModeBadge';
import { ModeToggle } from 'components/strategy/ModeToggle';
import { useFlowPatternKeysForRule } from 'hooks/useFlowPatternKeys';
import { useSseStatus } from 'hooks/useSseStatus';
import { OPS_PARAMS, portfolioHref, rulesHref } from 'lib/strategy/nav';
import type { ModeFilter } from 'lib/strategy/mode';
import { formatCompact } from 'utils/format';
import {
  formatSigned,
  formatSignedPct,
  pctGradeClass,
  signedToneClass,
} from 'lib/signedTone';
import { apiErrorMessage } from 'store/apiSlice';
import { ConsoleHistorySection } from '@live/components/history/ConsoleHistorySection';
import { FloorBookStrip } from '@live/components/floor/FloorBookStrip';
import { FloorPositionDetailWithFills } from '@live/components/floor/FloorPositionDetailWithFills';
import { FloorMintChart } from '@live/components/floor/FloorMintChart';
import {
  useCloseRulePositionMutation,
  useGetPortfolioHoldingsQuery,
  useManualBuyPositionMutation,
  useSellTokenMutation,
  useSetManualExitConfigMutation,
} from '@live/store/liveEndpoints';
import {
  ATTENTION_STATUSES,
  selectLiveArmed,
  selectLiveOpen,
  selectLiveStatusHydrated,
  type LiveArmedRow,
  type LiveOpenRow,
} from '@live/slices/liveStatusSlice';
import type { RootState } from '@live/store';
import type { WalletHolding } from 'types';

/** Loose base58 mint check — the backend does the real validation. */
const MINT_RE = /^[1-9A-HJ-NP-Za-km-z]{32,44}$/;

/** Stable DataTable row keys — hoisted so live re-renders don't defeat row memo. */
const openPositionRowKey = (r: LiveOpenRow) => r.positionId;
const waitingRowKey = (r: LiveArmedRow) => r.key;

type CloseAction = 'retry' | 'dump' | 'writeoff' | 'verify';

const STATUS_LABEL: Record<string, string> = {
  BuySubmitted: 'Buy submitted',
  Holding: 'Holding',
  ExitPending: 'Exit pending',
  ExitUnconfirmed: 'Exit unconfirmed',
  ExitStuck: 'Exit stuck',
  End: 'End',
  EntryFailed: 'Entry failed',
};

function statusVariant(status: string): BadgeVariant {
  switch (status) {
    case 'ExitPending':
    case 'ExitUnconfirmed':
      return 'warning';
    case 'Holding':
      return 'success';
    case 'BuySubmitted':
      return 'info';
    case 'ExitStuck':
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

/** The attention lane: stuck/unconfirmed exits + backend-flagged stale buys.
 *  Status truth only — the page never infers from fills or timestamps. */
function isAttention(r: LiveOpenRow): boolean {
  return ATTENTION_STATUSES.has(r.status) || (r.status === 'BuySubmitted' && r.needsReview);
}

interface LogEntry {
  key: string;
  at: number;
  action: string;
  mint: string;
  note: string;
  ok: boolean;
}

/** Persistent manual-trade log (defect #10: the old page-local array died on
 *  reload). localStorage-backed; the durable source of truth for manual
 *  activity remains the positions table itself (origin='manual' rows). */
const TRADE_LOG_KEY = 'mt:console-trade-log';

function loadTradeLog(): LogEntry[] {
  try {
    const raw = localStorage.getItem(TRADE_LOG_KEY);
    if (!raw) return [];
    const arr = JSON.parse(raw) as LogEntry[];
    return Array.isArray(arr) ? arr.slice(0, 50) : [];
  } catch {
    return [];
  }
}

/**
 * The unified real-trade Console (`/console`) — replaces Floor + Trade.
 * Lanes, top to bottom: ATTENTION (always first — every row has an action),
 * OPEN beside the MANUAL TRADE panel, WAITING (collapsible), then HISTORY.
 *
 * The three lanes above History are the **cockpit**: live, SSE-driven, scoped to
 * what is still actionable. History is the **review** surface — server-paged over
 * the whole `strategy_positions` population with its own cohort filter and charts
 * deck, so it owns closed rows entirely (there is no session-local closes buffer).
 */
export function ConsolePage() {
  const [params, setParams] = useSearchParams();
  const modeFilter = (params.get(OPS_PARAMS.mode) as ModeFilter | null) ?? 'real';
  const statusFilter = params.get(OPS_PARAMS.status);
  const mintParam = params.get(OPS_PARAMS.mint);
  const ruleParam = params.get(OPS_PARAMS.rule);
  const positionParam = params.get(OPS_PARAMS.position);

  const setMode = (m: ModeFilter) => {
    const next = new URLSearchParams(params);
    next.set(OPS_PARAMS.mode, m);
    setParams(next, { replace: true });
  };
  const clearDeepLink = () => {
    const next = new URLSearchParams();
    next.set(OPS_PARAMS.mode, modeFilter);
    setParams(next, { replace: true });
  };

  const sse = useSseStatus();
  const sseLive = sse === 'open';
  const hydrated = useSelector(selectLiveStatusHydrated);
  const armedMap = useSelector(selectLiveArmed);
  const openMap = useSelector(selectLiveOpen);
  const snapshotLoading = useSelector((s: RootState) => s.liveStatus.snapshotLoading);

  const [closePosition] = useCloseRulePositionMutation();
  const [manualBuy] = useManualBuyPositionMutation();
  const [setManualExit] = useSetManualExitConfigMutation();
  const [sellToken] = useSellTokenMutation();

  const [busyId, setBusyId] = useState<string | null>(null);
  const [actionErr, setActionErr] = useState<string | null>(null);
  const [actionNotice, setActionNotice] = useState<string | null>(null);

  // Manual-trade log — survives reloads (localStorage).
  const [log, setLog] = useState<LogEntry[]>(loadTradeLog);
  const logSeq = useRef(Date.now());
  const pushLog = useCallback((action: string, mint: string, note: string, ok: boolean) => {
    const key = `${logSeq.current++}`;
    setLog((prev) => {
      const next = [{ key, at: Date.now(), action, mint, note, ok }, ...prev].slice(0, 50);
      try {
        localStorage.setItem(TRADE_LOG_KEY, JSON.stringify(next));
      } catch {
        /* quota/private mode — session-only fallback */
      }
      return next;
    });
  }, []);

  // ── Bag marks (SSOT unrealized PnL) — join by mint for real open MTM ────────
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

  // ── Lane partitioning (status truth from the slice; zero inference) ─────────
  const openAll = useMemo(
    () => Object.values(openMap).filter((r) => modeOk(r.mode, modeFilter)),
    [openMap, modeFilter],
  );
  const attentionRows = useMemo(() => openAll.filter(isAttention), [openAll]);
  const openRows = useMemo(() => {
    let rows = openAll.filter((r) => !isAttention(r));
    if (ruleParam) rows = rows.filter((r) => r.ruleId === ruleParam);
    if (statusFilter) rows = rows.filter((r) => r.status === statusFilter);
    return rows;
  }, [openAll, ruleParam, statusFilter]);
  const waitingRows = useMemo(() => {
    const rows = Object.values(armedMap).filter((r) => modeOk(r.tradeMode, modeFilter));
    return ruleParam ? rows.filter((r) => r.ruleId === ruleParam) : rows;
  }, [armedMap, modeFilter, ruleParam]);
  const totalDeployed = openAll.reduce((s, r) => s + (r.entrySol ?? 0), 0);

  const openMtmByRule = useMemo(() => {
    const acc = new Map<string, { label: string; value: number }>();
    for (const r of openAll) {
      const { mtmSol } = openMark(r);
      if (mtmSol == null || !Number.isFinite(mtmSol)) continue;
      const key = r.ruleId ?? r.positionId;
      const label =
        r.origin === 'manual'
          ? 'manual'
          : (r.ruleName ?? (r.ruleId ? r.ruleId.slice(0, 8) : r.mint.slice(0, 8)));
      const prev = acc.get(key);
      acc.set(key, { label, value: (prev?.value ?? 0) + mtmSol });
    }
    return [...acc.entries()]
      .map(([key, v]) => ({ key, label: v.label, value: v.value }))
      .sort((a, b) => b.value - a.value);
  }, [openAll, openMark]);

  // ── Row actions (legality mirrors the backend close-action matrix) ──────────
  const onAction = useCallback(
    async (row: LiveOpenRow, action: CloseAction, sellBps?: number) => {
      const mint8 = `${row.mint.slice(0, 8)}…`;
      const pctLabel = sellBps != null ? `${(sellBps / 100).toFixed(sellBps % 100 === 0 ? 0 : 1)}%` : null;
      const prompt =
        action === 'dump'
          ? `Force-DUMP ${mint8}? Sells with NO slippage floor — accepts whatever the (near-drained) pool gives. REAL on-chain sell.`
          : action === 'writeoff'
            ? `WRITE OFF ${mint8}? Books this position closed at a FULL LOSS with no sell — for a pool with no sellable liquidity. Cannot be undone.`
            : action === 'verify'
              ? null // verify is read-only + heal — no confirm needed
              : row.status === 'ExitUnconfirmed'
                ? `Re-sell ${mint8}? Safe: if the original sell landed, this books the row cleared instead of double-selling.`
                : pctLabel
                  ? `Sell ${pctLabel} of initial bag (${mint8})? REAL mode sends an on-chain partial. Position stays open.`
                  : `Sell ALL of this position (${mint8})? REAL mode sends an on-chain sell.`;
      if (prompt && !window.confirm(prompt)) return;
      setActionErr(null);
      setActionNotice(null);
      setBusyId(row.positionId);
      try {
        const res = await closePosition({
          strategy: 'generic',
          positionId: row.positionId,
          action,
          sellBps,
        }).unwrap();
        if (action === 'verify') {
          const note = res.cleared
            ? 'verified — sell HAD landed, booked closed'
            : res.still_held
              ? 'verified — bag still held (re-sell / dump / write off)'
              : res.adopted
                ? 'verified — buy fill adopted, now Holding'
                : res.dropped
                  ? 'verified — all buy attempts reverted, row dropped'
                  : 'verified — still ambiguous, leave it to the reaper';
          setActionNotice(`${mint8}: ${note}`);
          pushLog('Verify', row.mint, note, true);
          setBusyId(null);
        } else if (res.closed || res.written_off) {
          pushLog(action === 'writeoff' ? 'Write off' : 'Sell', row.mint, 'booked closed', true);
          setBusyId(null);
        } else {
          pushLog(
            action === 'dump' ? 'Dump' : pctLabel ? `Sell ${pctLabel}` : 'Sell',
            row.mint,
            'sell submitted',
            true,
          );
        }
      } catch (e) {
        setBusyId(null);
        const msg = apiErrorMessage(e as never) ?? 'action failed';
        setActionErr(`${mint8}: ${msg}`);
        pushLog(action, row.mint, msg, false);
      }
    },
    [closePosition, pushLog],
  );

  // Clear the per-row busy marker once SSE moves the row on.
  useEffect(() => {
    if (!busyId) return;
    const row = openMap[busyId];
    if (!row || row.status !== 'Holding') setBusyId(null);
  }, [busyId, openMap]);

  // ── TP/SL editor (manual Holding rows) ──────────────────────────────────────
  const [tpslFor, setTpslFor] = useState<LiveOpenRow | null>(null);
  const [tpInput, setTpInput] = useState('');
  const [slInput, setSlInput] = useState('');
  const submitTpsl = useCallback(async () => {
    if (!tpslFor) return;
    const tp = tpInput.trim() === '' ? undefined : Number(tpInput);
    const sl = slInput.trim() === '' ? undefined : Number(slInput);
    for (const [name, v] of [
      ['TP', tp],
      ['SL', sl],
    ] as const) {
      if (v !== undefined && (!Number.isFinite(v) || v <= 0)) {
        setActionErr(`${name} must be a positive percentage`);
        return;
      }
    }
    try {
      await setManualExit({ positionId: tpslFor.positionId, tp_pct: tp, sl_pct: sl }).unwrap();
      pushLog(
        'TP/SL',
        tpslFor.mint,
        tp === undefined && sl === undefined
          ? 'cleared — tracked-only again'
          : `set ${tp !== undefined ? `TP ${tp}%` : ''} ${sl !== undefined ? `SL ${sl}%` : ''}`.trim(),
        true,
      );
      setTpslFor(null);
    } catch (e) {
      setActionErr(apiErrorMessage(e as never) ?? 'TP/SL update failed');
    }
  }, [tpslFor, tpInput, slInput, setManualExit, pushLog]);

  // ── Manual trade panel state ────────────────────────────────────────────────
  const [tradeMint, setTradeMint] = useState(mintParam ?? '');
  const [tradeSol, setTradeSol] = useState('0.05');
  const [tradeTp, setTradeTp] = useState('');
  const [tradeSl, setTradeSl] = useState('');
  const [buying, setBuying] = useState(false);
  const [sellingMint, setSellingMint] = useState(false);
  useEffect(() => {
    if (mintParam && MINT_RE.test(mintParam)) setTradeMint(mintParam);
  }, [mintParam]);
  const tradeMintValid = MINT_RE.test(tradeMint.trim());

  const handleManualBuy = useCallback(async () => {
    const mint = tradeMint.trim();
    if (!MINT_RE.test(mint)) {
      setActionErr('Enter a valid token mint address');
      return;
    }
    const sol = Number(tradeSol.trim());
    if (!Number.isFinite(sol) || sol <= 0) {
      setActionErr('Enter a valid SOL amount > 0');
      return;
    }
    const tp = tradeTp.trim() === '' ? undefined : Number(tradeTp);
    const sl = tradeSl.trim() === '' ? undefined : Number(tradeSl);
    for (const [name, v] of [
      ['TP', tp],
      ['SL', sl],
    ] as const) {
      if (v !== undefined && (!Number.isFinite(v) || v <= 0)) {
        setActionErr(`${name} must be a positive percentage`);
        return;
      }
    }
    setActionErr(null);
    setBuying(true);
    try {
      const res = await manualBuy({
        mint_address: mint,
        amount_sol: sol,
        ...(tp !== undefined ? { tp_pct: tp } : {}),
        ...(sl !== undefined ? { sl_pct: sl } : {}),
      }).unwrap();
      pushLog('Buy', mint, `◎${sol} submitted → position ${res.position_id.slice(0, 8)}…`, true);
    } catch (e) {
      const msg = apiErrorMessage(e as never) ?? 'unknown error';
      setActionErr(`Buy failed: ${msg}`);
      pushLog('Buy', mint, msg, false);
    } finally {
      setBuying(false);
    }
  }, [tradeMint, tradeSol, tradeTp, tradeSl, manualBuy, pushLog]);

  const handleSellAllByMint = useCallback(async () => {
    const mint = tradeMint.trim();
    if (!MINT_RE.test(mint)) {
      setActionErr('Enter a valid token mint address');
      return;
    }
    // A tracked row should be closed through ITS actions (position-aware,
    // engine-coordinated) — the wallet sweep is for external/Phantom bags.
    const tracked = openAll.find((r) => r.mint === mint && r.mode === 'real');
    const prompt = tracked
      ? `A tracked position exists for ${mint.slice(0, 8)}… (${tracked.status}). Prefer its row's Sell. Sweep the WALLET balance anyway?`
      : `Sell the wallet's ENTIRE balance of ${mint.slice(0, 8)}…? For external bags with no tracked row.`;
    if (!window.confirm(prompt)) return;
    setActionErr(null);
    setSellingMint(true);
    try {
      await sellToken({ mint_address: mint }).unwrap();
      pushLog('Sell all', mint, 'wallet balance swept', true);
    } catch (e) {
      const msg = apiErrorMessage(e as never) ?? 'unknown error';
      setActionErr(`Sell failed: ${msg}`);
      pushLog('Sell all', mint, msg, false);
    } finally {
      setSellingMint(false);
    }
  }, [tradeMint, openAll, sellToken, pushLog]);

  // ── Shared cells ────────────────────────────────────────────────────────────
  const ruleLink = (ruleId: string | null, name: string | null, origin?: string) => {
    if (origin === 'manual' || (!ruleId && name === 'manual')) {
      return <span className="text-text-dim">manual</span>;
    }
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

  const originDot = (r: LiveOpenRow) => (
    <span
      className={r.origin === 'manual' ? 'text-accent' : 'text-primary'}
      title={r.origin === 'manual' ? 'manual position' : 'bot position'}
    >
      {r.origin === 'manual' ? '○' : '●'}
    </span>
  );

  const tokenCell = (r: LiveOpenRow) => (
    <span className="inline-flex items-center gap-1.5">
      {originDot(r)}
      <AddressDisplay address={r.mint} kind="token" />
    </span>
  );

  const statusCell = (r: LiveOpenRow) => (
    <span className="inline-flex flex-wrap items-center gap-1">
      <Badge variant={statusVariant(r.status)}>{STATUS_LABEL[r.status] ?? r.status}</Badge>
      {r.soldBps > 0 && (
        <Badge variant="accent" title={`Scale stage ${r.scaleStage}`}>
          {Math.round(r.soldBps / 100)}% banked
        </Badge>
      )}
      {r.status === 'ExitStuck' &&
        (r.exitParked ? (
          <Badge variant="danger">PARKED</Badge>
        ) : (
          <span className="text-[10px] text-text-dim">retry {Math.min(r.exitRedriveCount, 2)}/2</span>
        ))}
      {r.status === 'BuySubmitted' && r.needsReview && (
        <Badge variant="warning">stale — verify</Badge>
      )}
      {/* Deadness chip (defect #9): a rugged/drained pool must not read as
          "no data" — the shared is_dead verdict rides on the holdings join. */}
      {holdingByMint.get(r.mint)?.is_dead && (
        <span title="Dead pool — liquidity gone; sells may only clear via Dump / Write off">
          <Badge variant="danger">❗ dead</Badge>
        </span>
      )}
      {r.mode === 'paper' && <ModeBadge mode="paper" />}
      {r.mode === 'real' && modeFilter === 'all' && <ModeBadge mode="real" />}
    </span>
  );

  // Per-row stale cue (defect #10): with a dead feed the Age keeps ticking but
  // the STATUS may be old — mark it instead of letting it read as live.
  const ageCell = (r: LiveOpenRow) => {
    const t = r.entryTime ? Date.parse(r.entryTime) : r.updatedAt;
    const age = Number.isFinite(t) ? fmtAge(Date.now() - t) : '—';
    return sseLive ? age : `${age} ⚠`;
  };
  const ageSecs = (r: LiveOpenRow) => {
    const t = r.entryTime ? Date.parse(r.entryTime) : r.updatedAt;
    return Number.isFinite(t) ? Math.max(0, Math.floor((Date.now() - t) / 1000)) : null;
  };

  /** Per-status action cell — mirrors the backend legality matrix 1:1. */
  const actionsCell = (r: LiveOpenRow) => {
    const busy = busyId === r.positionId;
    const actionable = sseLive && r.mode === 'real';
    if (!actionable) {
      return (
        <span className="text-text-dim" title={sseLive ? 'paper position' : 'SSE not live — status may be stale'}>
          —
        </span>
      );
    }
    return (
      <div className="flex items-center gap-1.5" onClick={(e) => e.stopPropagation()}>
        {r.status === 'Holding' && (
          <>
            <IconButton
              variant="danger"
              size="md"
              label="Sell"
              disabled={busy}
              onClick={() => void onAction(r, 'retry')}
              title="Sell ALL — force-close this position"
              aria-label="Sell ALL"
            >
              {busy ? <SpinnerIcon /> : <SellIcon />}
            </IconButton>
            <Button
              variant="link"
              size="xs"
              disabled={busy}
              className="text-warning"
              title="Sell 25% of the initial bag (partial)"
              onClick={() => void onAction(r, 'retry', 2500)}
            >
              25%
            </Button>
            <Button
              variant="link"
              size="xs"
              disabled={busy}
              className="text-warning"
              title="Sell 50% of the initial bag (partial)"
              onClick={() => void onAction(r, 'retry', 5000)}
            >
              50%
            </Button>
            {r.origin === 'manual' && (
              <Button
                variant="link"
                size="xs"
                disabled={busy}
                className="text-accent"
                title="Set / change this manual position's TP/SL (empty = tracked-only)"
                onClick={() => {
                  setTpslFor(r);
                  setTpInput('');
                  setSlInput('');
                }}
              >
                +TP/SL
              </Button>
            )}
          </>
        )}
        {r.status === 'ExitPending' && <span className="text-text-dim">selling…</span>}
        {r.status === 'BuySubmitted' &&
          (r.needsReview ? (
            <Button
              variant="link"
              size="xs"
              disabled={busy}
              className="text-accent"
              title="Resolve on demand: adopt an indexed fill or drop a proven revert"
              onClick={() => void onAction(r, 'verify')}
            >
              Verify
            </Button>
          ) : (
            <span className="text-text-dim">buying…</span>
          ))}
        {r.status === 'ExitStuck' && (
          <>
            <Button
              variant="link"
              size="xs"
              disabled={busy}
              className="text-accent"
              title="Retry the sell (un-parks; fresh auto-redrive budget)"
              onClick={() => void onAction(r, 'retry')}
            >
              Retry
            </Button>
            <Button
              variant="link"
              size="xs"
              disabled={busy}
              className="text-warning"
              title="Force-dump — NO slippage floor (accept dust)"
              onClick={() => void onAction(r, 'dump')}
            >
              Dump
            </Button>
            <Button
              variant="link"
              size="xs"
              disabled={busy}
              className="text-text-dim hover:text-red"
              title="Book closed at a full loss with no sell"
              onClick={() => void onAction(r, 'writeoff')}
            >
              Write off
            </Button>
          </>
        )}
        {r.status === 'ExitUnconfirmed' && (
          <>
            <Button
              variant="link"
              size="xs"
              disabled={busy}
              className="text-accent"
              title="Check the wallet net on-chain history: bag gone ⇒ book closed"
              onClick={() => void onAction(r, 'verify')}
            >
              Verify
            </Button>
            <Button
              variant="link"
              size="xs"
              disabled={busy}
              className="text-warning"
              title="Safe re-sell: a landed original books the row cleared"
              onClick={() => void onAction(r, 'retry')}
            >
              Re-sell
            </Button>
            <Button
              variant="link"
              size="xs"
              disabled={busy}
              className="text-warning"
              title="Force-dump — NO slippage floor (accept dust)"
              onClick={() => void onAction(r, 'dump')}
            >
              Dump
            </Button>
            <Button
              variant="link"
              size="xs"
              disabled={busy}
              className="text-text-dim hover:text-red"
              title="Book closed at a full loss with no sell"
              onClick={() => void onAction(r, 'writeoff')}
            >
              Write off
            </Button>
          </>
        )}
      </div>
    );
  };

  // ── Columns (memoized — Console re-renders on SSE ticks; stable defs keep
  // DataTable row memo effective when only unrelated chrome changed).
  const openColsBase: ColumnDef<LiveOpenRow>[] = useMemo(
    () => [
      {
        key: 'mint',
        label: 'Token',
        render: tokenCell,
        searchValue: (r) => r.mint,
      },
      {
        key: 'rule',
        label: 'Rule',
        render: (r) => ruleLink(r.ruleId, r.ruleName, r.origin),
        searchValue: (r) => (r.origin === 'manual' ? 'manual' : (r.ruleName ?? r.ruleId ?? '')),
      },
      {
        key: 'status',
        label: 'Status',
        sortable: true,
        render: statusCell,
        sortValue: (r) => r.status,
        searchValue: (r) => STATUS_LABEL[r.status] ?? r.status,
      },
      {
        key: 'age',
        label: 'Age',
        sortable: true,
        render: (r) => (
          <span
            className={`tabular-nums ${sseLive ? 'text-text-dim' : 'text-warning'}`}
            title={sseLive ? undefined : 'SSE stale — status may be out of date'}
          >
            {ageCell(r)}
          </span>
        ),
        sortValue: (r) => ageSecs(r) ?? -1,
        searchValue: () => '',
        filterNumber: ageSecs,
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
        filterNumber: (r) => r.entrySol ?? null,
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
        filterNumber: (r) => openMark(r).mtmSol,
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
        filterNumber: (r) => openMark(r).mtmPct,
      },
      {
        key: 'actions',
        label: '',
        width: '250px',
        render: actionsCell,
        searchValue: () => '',
      },
    ],
    // tokenCell/actionsCell/ageCell close over live book + busy/SSE — rebuild when those move.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [sseLive, openMark, busyId, modeFilter, holdingByMint],
  );

  const waitingCols: ColumnDef<LiveArmedRow>[] = useMemo(
    () => [
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
        key: 'age',
        label: 'Age',
        sortable: true,
        render: (r) => (
          <span className="tabular-nums text-text-dim">{fmtAge(Date.now() - r.armedAt)}</span>
        ),
        sortValue: (r) => r.armedAt,
        searchValue: () => '',
        filterNumber: (r) => Math.max(0, Math.floor((Date.now() - r.armedAt) / 1000)),
      },
      {
        key: 'trade',
        label: '',
        width: '64px',
        render: (r) => (
          <button
            type="button"
            className="text-[11px] font-semibold text-accent hover:text-primary hover:underline"
            onClick={(e) => {
              e.stopPropagation();
              setTradeMint(r.mint);
            }}
            title="Prefill the manual-trade panel with this mint"
          >
            Trade
          </button>
        ),
        searchValue: () => '',
      },
    ],
    [],
  );

  // ── Deep-link detail modals (row click → position param) ────────────────────
  const selectedKey = positionParam;
  const inspectOpen =
    [...attentionRows, ...openRows].find((r) => r.positionId === selectedKey) ?? null;
  const openFlowPatternKeys = useFlowPatternKeysForRule(inspectOpen?.ruleId);
  // A closed row's detail modal belongs to the History section — it holds the
  // full DB record, so it can open a position from any date, not just one still
  // in the session's open lane.
  const inspectWaiting = inspectOpen
    ? null
    : (waitingRows.find((r) => r.key === selectedKey) ?? null);
  const waitingFlowPatternKeys = useFlowPatternKeysForRule(inspectWaiting?.ruleId);

  const selectRow = (key: string | null, mint?: string) => {
    if (!key) {
      clearDeepLink();
      return;
    }
    const next = new URLSearchParams(params);
    next.set(OPS_PARAMS.position, key);
    if (mint) next.set(OPS_PARAMS.mint, mint);
    setParams(next, { replace: true });
  };

  const openDetail = (r: LiveOpenRow) => {
    const { mtmSol, mtmPct } = openMark(r);
    const entryMs = r.entryTime ? Date.parse(r.entryTime) : NaN;
    return (
      <FloorPositionDetailWithFills
        positionId={r.positionId}
        facts={{
          mint: r.mint,
          ruleId: r.ruleId,
          ruleName: r.origin === 'manual' ? 'manual' : r.ruleName,
          mode: r.mode,
          status: STATUS_LABEL[r.status] ?? r.status,
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
          flowPatternKeys: openFlowPatternKeys,
        }}
        chartHeight={420}
      />
    );
  };

  const [waitingOpen, setWaitingOpen] = useState(false);

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Console"
        description={
          <>
            Real-trade book · every SOL-holding row lives here ·{' '}
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
          <>
            {/* SSE feed state — deliberately separate from the engine LIVE/DEAD
                kill switch in the app header (two different facts). */}
            <span
              className={`inline-flex items-center gap-1 text-[11px] font-semibold ${
                sseLive ? 'text-green' : 'text-warning'
              }`}
              title={sseLive ? 'SSE stream live' : 'SSE stream stale — rows may lag'}
            >
              {sseLive ? '● live' : '○ stale'}
            </span>

            <div className="grow" />

            <ModeToggle
              layout="ops"
              size="sm"
              value={modeFilter}
              onChange={setMode}
            />
          </>
        }
      />

      <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
        <StatTile label="Attention" value={String(attentionRows.length)} />
        <StatTile label="Open" value={String(openRows.length)} />
        <StatTile label="Deployed ◎" value={formatCompact(totalDeployed, 3)} />
        <StatTile label="Waiting" value={String(waitingRows.length)} />
      </div>

      {!sseLive && (
        <InlineAlert variant="warning">
          SSE not live — status may be stale; sells are disabled until the stream recovers.
        </InlineAlert>
      )}
      {!hydrated && snapshotLoading && (
        <p className="text-xs text-text-dim">Loading live book…</p>
      )}
      {actionErr && <InlineAlert variant="error">{actionErr}</InlineAlert>}
      {actionNotice && <InlineAlert variant="success">{actionNotice}</InlineAlert>}

      {/* ⚠ ATTENTION — always on top; every row here has at least one action. */}
      <section>
        <h2 className="mb-1.5 text-xs font-bold uppercase tracking-wider text-warning">
          ⚠ Attention ({attentionRows.length})
        </h2>
        {attentionRows.length === 0 ? (
          <p className="text-xs text-text-dim">
            Nothing needs eyes — no stuck, unconfirmed, or stale-buy rows.
          </p>
        ) : (
          <DataTable
            columns={openColsBase}
            rows={attentionRows}
            rowKey={openPositionRowKey}
            tableId="console-attention"
            emptyMessage="Nothing needs attention."
            selectedKey={selectedKey}
            onSelect={(key) => {
              const row = attentionRows.find((r) => r.positionId === key);
              selectRow(key, row?.mint);
            }}
          />
        )}
      </section>

      {/* OPEN + MANUAL TRADE — co-equal panels. */}
      <div className="grid grid-cols-1 gap-4 xl:grid-cols-[minmax(0,1fr)_380px]">
        <section className="min-w-0">
          <h2 className="mb-1.5 text-xs font-bold uppercase tracking-wider text-text-dim">
            Open ({openRows.length})
          </h2>
          {openMtmByRule.length > 0 && (
            <FloorBookStrip
              title="Open MTM by rule"
              rows={openMtmByRule}
              empty="No bag marks yet."
            />
          )}
          <DataTable
            columns={openColsBase}
            rows={openRows}
            rowKey={openPositionRowKey}
            searchable
            colFilters
            tableId="console-open"
            emptyMessage="No open positions."
            selectedKey={selectedKey}
            onSelect={(key) => {
              const row = openRows.find((r) => r.positionId === key);
              selectRow(key, row?.mint);
            }}
          />
        </section>

        <section className="flex min-w-0 flex-col gap-3">
          <h2 className="text-xs font-bold uppercase tracking-wider text-text-dim">
            Manual trade
          </h2>
          <div className="flex flex-col gap-2.5 rounded-lg border border-white/6 bg-bg-panel p-3">
            <label className="flex flex-col gap-1">
              <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
                Mint address
              </span>
              <Input
                type="text"
                fieldSize="md"
                placeholder="Token mint address"
                value={tradeMint}
                onChange={(e) => setTradeMint(e.target.value)}
                className="font-mono"
              />
            </label>
            <div className="flex flex-wrap items-end gap-2">
              <label className="flex flex-col gap-1">
                <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
                  Amount ◎
                </span>
                <Input
                  type="number"
                  fieldSize="md"
                  min={0.001}
                  step={0.001}
                  value={tradeSol}
                  onChange={(e) => setTradeSol(e.target.value)}
                  className="w-24"
                />
              </label>
              <label className="flex flex-col gap-1">
                <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
                  TP %
                </span>
                <Input
                  type="number"
                  fieldSize="md"
                  min={0}
                  step={5}
                  placeholder="off"
                  value={tradeTp}
                  onChange={(e) => setTradeTp(e.target.value)}
                  className="w-20"
                />
              </label>
              <label className="flex flex-col gap-1">
                <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
                  SL %
                </span>
                <Input
                  type="number"
                  fieldSize="md"
                  min={0}
                  step={5}
                  placeholder="off"
                  value={tradeSl}
                  onChange={(e) => setTradeSl(e.target.value)}
                  className="w-20"
                />
              </label>
            </div>
            <p className="text-[10px] leading-snug text-text-dim">
              Buys become full tracked positions (row + MTM + row actions). TP/SL opts into
              the engine's auto-exit stack (incl. dead-pool exit); empty = tracked-only.
            </p>
            <div className="flex items-center gap-2">
              <IconButton
                variant="primary"
                size="lg"
                onClick={() => void handleManualBuy()}
                disabled={buying || !tradeMintValid}
                label={buying ? 'Buying…' : 'Buy'}
                title="Submit a manual buy (202 → row appears as Buy submitted)"
              >
                {buying ? <SpinnerIcon /> : <BuyIcon />}
              </IconButton>
              <IconButton
                variant="danger"
                size="lg"
                onClick={() => void handleSellAllByMint()}
                disabled={sellingMint || !tradeMintValid}
                label={sellingMint ? 'Selling…' : 'Sell all by mint'}
                title="Sweep the WALLET's entire balance of this mint (external bags)"
              >
                {sellingMint ? <SpinnerIcon /> : <SellIcon />}
              </IconButton>
            </div>
          </div>

          {tradeMintValid && (
            <FloorMintChart mint={tradeMint.trim()} tableId="console-trade-chart" height={260} />
          )}

          <div className="rounded-lg border border-white/6 bg-bg-panel p-3">
            <h3 className="mb-1.5 text-[10px] font-bold uppercase tracking-wider text-text-dim">
              Trade log (session)
            </h3>
            {log.length === 0 ? (
              <p className="text-xs text-text-dim">No manual actions yet.</p>
            ) : (
              <ul className="flex max-h-56 flex-col gap-1 overflow-y-auto">
                {log.map((e) => (
                  <li key={e.key} className="flex items-center gap-2 text-xs">
                    <span className="tabular-nums text-text-dim">
                      {new Date(e.at).toLocaleTimeString()}
                    </span>
                    <Badge variant={e.ok ? (e.action === 'Buy' ? 'buy' : 'neutral') : 'danger'}>
                      {e.action}
                    </Badge>
                    <span className="font-mono text-text-mid">{e.mint.slice(0, 8)}…</span>
                    <span className={e.ok ? 'text-text-mid' : 'text-red'}>{e.note}</span>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </section>
      </div>

      {/* WAITING — armed rules, collapsible. */}
      <section>
        <button
          type="button"
          className="mb-1.5 text-xs font-bold uppercase tracking-wider text-text-dim hover:text-text"
          onClick={() => setWaitingOpen((v) => !v)}
        >
          {waitingOpen ? '▾' : '▸'} Waiting ({waitingRows.length})
        </button>
        {waitingOpen && (
          <>
            <DataTable
              columns={waitingCols}
              rows={waitingRows}
              rowKey={waitingRowKey}
              searchable
              tableId="console-waiting"
              emptyMessage="No armed (waiting) rules."
              selectedKey={selectedKey}
              onSelect={(key) => {
                const row = waitingRows.find((r) => r.key === key);
                selectRow(key, row?.mint);
              }}
            />
          </>
        )}
      </section>

      {/* HISTORY — every position across every rule and run, server-paged, with
          the charts deck above it. Supersedes the old 50-row Recent-closed lane
          (which could only show the session's SSE tail). */}
      <ConsoleHistorySection selectedKey={selectedKey} onSelect={selectRow} />

      {/* Detail modals (deep-link + row click). */}
      {inspectOpen && (
        <Modal
          title={`${inspectOpen.mint.slice(0, 8)}… — Open position`}
          open
          onClose={clearDeepLink}
          size="xxl"
        >
          {openDetail(inspectOpen)}
        </Modal>
      )}
      {inspectWaiting && (
        <Modal
          title={`${inspectWaiting.mint.slice(0, 8)}… — Waiting`}
          open
          onClose={clearDeepLink}
          size="xxl"
        >
          <div className="flex flex-col gap-3">
            <div className="flex flex-wrap items-center gap-3 text-[11px]">
              <span className="text-text-dim">
                Waiting for entry · armed{' '}
                <span className="tabular-nums text-text">
                  {fmtAge(Date.now() - inspectWaiting.armedAt)}
                </span>
              </span>
              {ruleLink(inspectWaiting.ruleId, inspectWaiting.ruleName)}
            </div>
            <FloorMintChart
              mint={inspectWaiting.mint}
              tableId="console-waiting-chart"
              height={420}
              flowPatternKeys={waitingFlowPatternKeys}
            />
          </div>
        </Modal>
      )}

      {/* Manual TP/SL editor. */}
      {tpslFor && (
        <Modal
          title={`${tpslFor.mint.slice(0, 8)}… — TP/SL`}
          open
          onClose={() => setTpslFor(null)}
        >
          <p className="mb-3 text-xs text-text-mid">
            Set take-profit / stop-loss for this manual position. Leaving both empty clears
            the config (tracked-only — no auto-exit of any kind).
          </p>
          <div className="flex flex-wrap items-end gap-2">
            <label className="flex flex-col gap-1">
              <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
                TP %
              </span>
              <Input
                type="number"
                fieldSize="md"
                min={0}
                step={5}
                placeholder="off"
                value={tpInput}
                onChange={(e) => setTpInput(e.target.value)}
                className="w-24"
              />
            </label>
            <label className="flex flex-col gap-1">
              <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
                SL %
              </span>
              <Input
                type="number"
                fieldSize="md"
                min={0}
                step={5}
                placeholder="off"
                value={slInput}
                onChange={(e) => setSlInput(e.target.value)}
                className="w-24"
              />
            </label>
          </div>
          <div className="mt-4 flex items-center justify-end gap-2.5">
            <Button variant="ghost" onClick={() => setTpslFor(null)}>
              Cancel
            </Button>
            <Button variant="primary" onClick={() => void submitTpsl()}>
              Apply
            </Button>
          </div>
        </Modal>
      )}
    </div>
  );
}
