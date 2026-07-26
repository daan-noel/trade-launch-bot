import { createSlice, type PayloadAction } from '@reduxjs/toolkit';
import type { OpenStrategyPosition, RecentClosedPosition } from 'types';
import type {
  ArmedChangedEvent,
  ArmedEntry,
  StrategyPositionUpdateEvent,
} from 'lib/strategy/types';

/** Cap on "recent closes" ring (SSE + DB hydrate). */
const MAX_RECENT_CLOSED = 50;

/**
 * Statuses that count as open inventory (still actionable / in flight). A row
 * with SOL possibly still in it (`ExitStuck` / `ExitUnconfirmed`) is OPEN — it
 * must never land in the closed/actionless table. Mirrors the backend partition
 * guard in `strategies/engine/sinks.rs`.
 */
export const OPEN_STATUSES = new Set([
  'BuySubmitted',
  'Holding',
  'ExitPending',
  'ExitStuck',
  'ExitUnconfirmed',
]);

/**
 * The attention lane: stuck/unconfirmed exits always; a `BuySubmitted` joins it
 * only when the backend flags `needs_review` (stale, unresolved buy).
 */
export const ATTENTION_STATUSES = new Set(['ExitStuck', 'ExitUnconfirmed']);

/** Terminal closes: a confirmed exit or a buy that never filled. */
export const TERMINAL_STATUSES = new Set(['End', 'EntryFailed']);

export function armedKey(ruleId: string, mint: string): string {
  return `${ruleId}|${mint}`;
}

export interface LiveArmedRow {
  key: string;
  ruleId: string;
  mint: string;
  ruleName: string | null;
  tradeMode: string | null;
  /** Client wall time when we first saw this arm (session age). */
  armedAt: number;
}

export interface LiveOpenRow {
  positionId: string;
  strategyId: string;
  ruleId: string | null;
  ruleName: string | null;
  mint: string;
  mode: string;
  status: string;
  entryPrice: number | null;
  entrySol: number | null;
  entryTime: string | null;
  exitReason: string | null;
  /** Backend-flagged stale unresolved BuySubmitted (B3) — needs manual Verify. */
  needsReview: boolean;
  /** `bot` | `manual` origin (snapshot-sourced; SSE deltas keep the prior value). */
  origin: string;
  /** ExitStuck redrive state (snapshot-sourced). */
  exitParked: boolean;
  exitRedriveCount: number;
  updatedAt: number;
}

export interface LiveClosedRow {
  positionId: string;
  ruleId: string | null;
  ruleName: string | null;
  mint: string;
  mode: string | null;
  status: string;
  exitReason: string | null;
  entryPrice: number | null;
  exitPrice: number | null;
  entrySol: number | null;
  exitSol: number | null;
  entryTime: string | null;
  exitTime: string | null;
  /** Realized SOL PnL when both legs known; null until hydrate / SSE with amounts. */
  pnlSol: number | null;
  closedAt: number;
}

export interface LiveStatusState {
  armed: Record<string, LiveArmedRow>;
  open: Record<string, LiveOpenRow>;
  recentClosed: LiveClosedRow[];
  /** True after at least one successful REST snapshot apply. */
  hydrated: boolean;
  lastSnapshotAt: number | null;
  /** In-flight snapshot (mount / reconnect / visibility). */
  snapshotLoading: boolean;
}

const initialState: LiveStatusState = {
  armed: {},
  open: {},
  recentClosed: [],
  hydrated: false,
  lastSnapshotAt: null,
  snapshotLoading: false,
};

function openFromPortfolio(
  p: OpenStrategyPosition,
  ruleNames: Record<string, string>,
): LiveOpenRow | null {
  if (!OPEN_STATUSES.has(p.status)) return null;
  const ruleId = p.rule_id;
  return {
    positionId: p.id,
    strategyId: p.strategy_id,
    ruleId,
    ruleName: (ruleId && ruleNames[ruleId]) || null,
    mint: p.mint_address,
    mode: p.mode,
    status: p.status,
    entryPrice: p.entry_price ?? null,
    entrySol: p.entry_sol ?? null,
    entryTime: p.entry_time ?? null,
    exitReason: null,
    needsReview: p.needs_review ?? false,
    origin: p.origin ?? 'bot',
    exitParked: p.exit_parked ?? false,
    exitRedriveCount: p.exit_redrive_count ?? 0,
    updatedAt: Date.now(),
  };
}

function closedFromRecent(
  p: RecentClosedPosition,
  ruleNames: Record<string, string>,
): LiveClosedRow | null {
  if (!TERMINAL_STATUSES.has(p.status)) return null;
  const ruleId = p.rule_id;
  const closedAt = p.exit_time
    ? Date.parse(p.exit_time)
    : p.updated_at
      ? Date.parse(p.updated_at)
      : Date.now();
  const entrySol = p.entry_sol ?? null;
  const exitSol = p.exit_sol ?? null;
  const pnlSol =
    entrySol != null && exitSol != null && Number.isFinite(entrySol) && Number.isFinite(exitSol)
      ? exitSol - entrySol
      : null;
  return {
    positionId: p.id,
    ruleId,
    ruleName: (ruleId && ruleNames[ruleId]) || null,
    mint: p.mint_address,
    mode: p.mode,
    status: p.status,
    exitReason: p.exit_reason ?? null,
    entryPrice: p.entry_price ?? null,
    exitPrice: p.exit_price ?? null,
    entrySol,
    exitSol,
    entryTime: p.entry_time ?? null,
    exitTime: p.exit_time ?? null,
    pnlSol,
    closedAt: Number.isFinite(closedAt) ? closedAt : Date.now(),
  };
}

const liveStatusSlice = createSlice({
  name: 'liveStatus',
  initialState,
  reducers: {
    snapshotStart(state) {
      state.snapshotLoading = true;
    },
    /**
     * Replace armed + open from REST; merge DB recent closes (newest-first).
     * Drops armed rows that collide with an open (rule, mint) — Waiting must
     * never sit next to a buy-confirmed Open after snapshot.
     */
    applySnapshot(
      state,
      action: PayloadAction<{
        armed: ArmedEntry[];
        positions: OpenStrategyPosition[];
        recentClosed?: RecentClosedPosition[];
        ruleNames: Record<string, string>;
      }>,
    ) {
      const { armed, positions, recentClosed, ruleNames } = action.payload;
      const nextArmed: Record<string, LiveArmedRow> = {};
      const now = Date.now();
      for (const e of armed) {
        const key = armedKey(e.rule_id, e.mint_address);
        const prev = state.armed[key];
        nextArmed[key] = {
          key,
          ruleId: e.rule_id,
          mint: e.mint_address,
          ruleName: ruleNames[e.rule_id] ?? prev?.ruleName ?? null,
          tradeMode: prev?.tradeMode ?? null,
          armedAt: prev?.armedAt ?? now,
        };
      }
      const nextOpen: Record<string, LiveOpenRow> = {};
      const openKeys = new Set<string>();
      for (const p of positions) {
        const row = openFromPortfolio(p, ruleNames);
        if (!row) continue;
        const prev = state.open[row.positionId];
        if (prev?.ruleName && !row.ruleName) row.ruleName = prev.ruleName;
        if (prev?.entrySol != null && row.entrySol == null) row.entrySol = prev.entrySol;
        nextOpen[row.positionId] = row;
        if (row.ruleId) openKeys.add(armedKey(row.ruleId, row.mint));
      }
      // Snapshot must not resurrect Waiting for mints already Open.
      for (const key of openKeys) {
        delete nextArmed[key];
      }
      state.armed = nextArmed;
      state.open = nextOpen;

      if (recentClosed) {
        const fromDb: LiveClosedRow[] = [];
        for (const p of recentClosed) {
          const row = closedFromRecent(p, ruleNames);
          if (row) fromDb.push(row);
        }
        // Prefer DB order; keep any newer session-only closes not yet in DB page.
        const dbIds = new Set(fromDb.map((r) => r.positionId));
        const sessionExtra = state.recentClosed.filter((r) => !dbIds.has(r.positionId));
        state.recentClosed = [...fromDb, ...sessionExtra]
          .sort((a, b) => b.closedAt - a.closedAt)
          .slice(0, MAX_RECENT_CLOSED);
      }

      state.hydrated = true;
      state.lastSnapshotAt = now;
      state.snapshotLoading = false;
    },
    snapshotFailed(state) {
      state.snapshotLoading = false;
    },
    applyArmedDelta(state, action: PayloadAction<ArmedChangedEvent>) {
      const d = action.payload;
      const key = armedKey(d.rule_id, d.mint_address);
      if (d.state === 'armed') {
        // Ignore arm if this mint is already an open position for the rule.
        const occupied = Object.values(state.open).some(
          (o) => o.ruleId === d.rule_id && o.mint === d.mint_address,
        );
        if (occupied) return;
        const prev = state.armed[key];
        state.armed[key] = {
          key,
          ruleId: d.rule_id,
          mint: d.mint_address,
          ruleName: d.rule_name ?? prev?.ruleName ?? null,
          tradeMode: d.trade_mode ?? prev?.tradeMode ?? null,
          armedAt: prev?.armedAt ?? Date.now(),
        };
      } else {
        delete state.armed[key];
      }
    },
    applyPositionDelta(state, action: PayloadAction<StrategyPositionUpdateEvent>) {
      const d = action.payload;
      // Nil / empty ids cannot key the open map (stale terminal frames).
      if (!d.position_id || d.position_id === '00000000-0000-0000-0000-000000000000') {
        return;
      }
      const now = Date.now();
      const prev = state.open[d.position_id];

      if (OPEN_STATUSES.has(d.status)) {
        state.open[d.position_id] = {
          positionId: d.position_id,
          strategyId: prev?.strategyId ?? 'generic',
          ruleId: d.rule_id,
          ruleName: d.rule_name ?? prev?.ruleName ?? null,
          mint: d.mint_address,
          mode: d.trade_mode ?? prev?.mode ?? 'real',
          status: d.status,
          entryPrice: d.entry_price ?? prev?.entryPrice ?? null,
          entrySol: prev?.entrySol ?? null,
          entryTime: prev?.entryTime ?? null,
          exitReason: d.exit_reason ?? null,
          needsReview: d.needs_review ?? prev?.needsReview ?? false,
          origin: prev?.origin ?? 'bot',
          exitParked: prev?.exitParked ?? false,
          exitRedriveCount: prev?.exitRedriveCount ?? 0,
          updatedAt: now,
        };
        // Entered a position → drop matching armed waiting row.
        if (d.status === 'BuySubmitted' || d.status === 'Holding') {
          delete state.armed[armedKey(d.rule_id, d.mint_address)];
        }
        return;
      }

      delete state.open[d.position_id];
      if (TERMINAL_STATUSES.has(d.status)) {
        const closed: LiveClosedRow = {
          positionId: d.position_id,
          ruleId: d.rule_id,
          ruleName: d.rule_name ?? prev?.ruleName ?? null,
          mint: d.mint_address,
          mode: d.trade_mode ?? prev?.mode ?? null,
          status: d.status,
          exitReason: d.exit_reason ?? null,
          entryPrice: d.entry_price ?? prev?.entryPrice ?? null,
          exitPrice: d.exit_price ?? null,
          entrySol: prev?.entrySol ?? null,
          exitSol: null,
          entryTime: prev?.entryTime ?? null,
          exitTime: null,
          pnlSol: null,
          closedAt: now,
        };
        state.recentClosed = [
          closed,
          ...state.recentClosed.filter((r) => r.positionId !== d.position_id),
        ].slice(0, MAX_RECENT_CLOSED);
      }
    },
  },
});

export const {
  snapshotStart,
  applySnapshot,
  snapshotFailed,
  applyArmedDelta,
  applyPositionDelta,
} = liveStatusSlice.actions;

export default liveStatusSlice.reducer;

/** Selectors — pages must read these, never a private Map. */
export const selectLiveStatus = (s: { liveStatus: LiveStatusState }) => s.liveStatus;
export const selectLiveArmed = (s: { liveStatus: LiveStatusState }) => s.liveStatus.armed;
export const selectLiveOpen = (s: { liveStatus: LiveStatusState }) => s.liveStatus.open;
export const selectLiveRecentClosed = (s: { liveStatus: LiveStatusState }) => s.liveStatus.recentClosed;
export const selectLiveStatusHydrated = (s: { liveStatus: LiveStatusState }) => s.liveStatus.hydrated;

export function selectOpenByRule(ruleId: string) {
  return (s: { liveStatus: LiveStatusState }): LiveOpenRow[] =>
    Object.values(s.liveStatus.open).filter((r) => r.ruleId === ruleId);
}

export function selectRuleOpenCounts(s: { liveStatus: LiveStatusState }): Record<
  string,
  { open: number; pending: number }
> {
  const out: Record<string, { open: number; pending: number }> = {};
  for (const r of Object.values(s.liveStatus.open)) {
    if (!r.ruleId) continue;
    const row = out[r.ruleId] ?? { open: 0, pending: 0 };
    if (r.status === 'Holding' || r.status === 'ExitPending') row.open += 1;
    else if (r.status === 'BuySubmitted') row.pending += 1;
    else if (r.status === 'ExitUnconfirmed' || r.status === 'ExitStuck') row.open += 1;
    out[r.ruleId] = row;
  }
  return out;
}
