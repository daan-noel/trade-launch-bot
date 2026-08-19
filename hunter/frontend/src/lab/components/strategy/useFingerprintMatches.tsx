// Lab-side companion for the shared `FingerprintScopeControl`: given a selected
// fingerprint, fetches how many tokens it matches (a count chip) and opens a
// paged token table of those matches in a modal. One SSOT so every lab surface
// that scopes by a fingerprint (Sweep config, Flow / Metric discovery, Creation
// Stats) shows a fingerprint's tokens identically.
//
// The match set reuses the existing scoped endpoint
// (`POST /api/tokens/creation-stats/grouped/tokens` with `fingerprint_id`,
// backed by the SQL mirror of `hunter_engine::fingerprint::matches`). That
// endpoint is **window-scoped** — default the last 30 days of token creations —
// so counts/rows are "matched, created in the window", not all-time. Kept in
// `@lab` (not `src/shared`) because the endpoint hook is lab-only and the shared
// control must stay import-boundary clean (`shared ⊬ @lab`).
//
// The modal opens on the fingerprint's CREATION STATS — the day x hour
// seasonality heatmap + calendar trend for exactly the matched corpus, from the
// same scoped grouped endpoint (`GET .../grouped?fingerprint_id=`, a single
// `g = 0` group) — with the token table below it. Clicking a heatmap tile
// narrows the table to that recurring weekly slot.
//
// Count fetch is **lazy**: it starts on `ensureCount` (hover the chip) or when
// the matches modal opens — not on every page mount with a persisted seed id.

import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { skipToken } from '@reduxjs/toolkit/query/react';
import { Modal } from 'components/ui/Modal';
import { Button } from 'components/ui/Button';
import { LoadingState } from 'components/ui/LoadingState';
import { StatCard } from 'components/ui/StatCard';
import { CreationHeatmap } from 'components/creation-stats/CreationHeatmap';
import { CreationWindowPicker } from 'components/creation-stats/CreationWindowPicker';
import { useTimezone } from 'context/TimezoneContext';
import { TokenTable } from 'components/tokens/TokenTable';
import { ALL_TOKEN_INFO_KEYS } from 'components/tokens/sharedTokenColumns';
import { tokenColumns } from 'components/tokens/tokenColumns';
import { inspectFromMint } from 'components/strategy/inspectTarget';
import { LazyLabTokenInspectModal } from '@lab/components/strategy/LazyLabTokenInspectModal';
import { useFlowPatternSource } from 'hooks/useFlowPatternKeys';
import { apiErrorMessage } from 'store/apiSlice';
import {
  useGetGroupedCreationStatsQuery,
  useGetGroupedCreationTokensQuery,
} from '@lab/store/labEndpoints';
import {
  drillTokenFilters,
  toHeatCell,
  weeklySlotLabel,
  type GroupedCreationArgs,
  type GroupedCreationTokensArgs,
} from '@lab/components/creation-stats/groupedCreationStats';
import {
  bucketOptionsForRange,
  clampBucketToRange,
  resolveCreationWindow,
  type CreationBucket,
  type CreationRangePreset,
  type CreationWindow,
} from 'components/creation-stats/creationStats';
import { formatWithCommas } from 'utils/format';
import type { TableQuery } from 'components/table/types';
import type { TokenRecord } from 'types';

/** The trend chart pulls `lightweight-charts` — keep it off the modal's shell. */
const GroupedCreationTrendChart = lazy(() =>
  import('@lab/components/creation-stats/GroupedCreationTrendChart').then((m) => ({
    default: m.GroupedCreationTrendChart,
  })),
);

/** Look-back window (days) the scoped endpoint applies when no `from` is sent —
 *  mirrors the backend `DEFAULT_WINDOW_DAYS` in `creation_stats.rs`. Surfaced so
 *  the count chip / modal can label the window honestly, and the modal's own
 *  window opens on the same span the count chip reports. */
export const FINGERPRINT_MATCH_WINDOW_DAYS = 30;

/** The modal's initial window — the count chip's default span, as a picker value. */
const INITIAL_WINDOW: CreationWindow = {
  preset: String(FINGERPRINT_MATCH_WINDOW_DAYS) as CreationRangePreset,
  from: '',
  to: '',
};

const INITIAL_QUERY: TableQuery = {
  page: 1,
  pageSize: 25,
  sortKeys: [],
  search: '',
  colFilters: {},
};

/** Stable empty reference so the table doesn't remount while a page loads. */
const EMPTY_ROWS: TokenRecord[] = [];

/** Window/tile scope shared by the modal's charts and its token table, so both
 *  read the SAME corpus. `from` omitted ⇒ the backend's default
 *  {@link FINGERPRINT_MATCH_WINDOW_DAYS}-day window; `cell` set ⇒ narrowed to
 *  that recurring weekly day×hour slot (a heatmap-tile click). */
interface MatchScope {
  tz: string;
  from?: string;
  /** RFC3339 upper bound; omitted for an open window (`-> now`). */
  to?: string;
  cell?: { dow: number; hour: number } | null;
}

/** The whole-window, UTC scope — what the lazy count chip asks for (no tile, no
 *  look-back override), and the modal's initial state. */
const DEFAULT_SCOPE: MatchScope = { tz: 'UTC' };

/** Build the scoped-token args (fingerprint scope ⇒ group_by/group_key ignored
 *  server-side; we still pass the required-but-ignored fields). */
function scopedArgs(
  fingerprintId: string,
  query: TableQuery,
  scope: MatchScope = DEFAULT_SCOPE,
): GroupedCreationTokensArgs {
  return {
    tz: scope.tz,
    from: scope.from,
    to: scope.to,
    segment: 'all',
    groupBy: [],
    groupKey: {},
    fingerprintId,
    ...(scope.cell ? { dow: scope.cell.dow, hour: scope.cell.hour } : {}),
    page: query.page,
    pageSize: query.pageSize,
    sortKeys: query.sortKeys,
    search: query.search,
    filters: drillTokenFilters(query),
  };
}

/** Build the scoped creation-stats args — the single `g = 0` group the backend
 *  returns under a fingerprint scope (`groupBy`/`top` are ignored there, but the
 *  arg type requires them). The heatmap folds the WHOLE window regardless of the
 *  selected tile: the tile narrows the table, not the chart that picks it. */
function statsArgs(
  fingerprintId: string,
  scope: MatchScope,
  bucket: CreationBucket,
): GroupedCreationArgs {
  return {
    bucket,
    tz: scope.tz,
    from: scope.from,
    to: scope.to,
    segment: 'all',
    groupBy: [],
    top: 1,
    fingerprintId,
  };
}

interface FingerprintMatchesModalProps {
  fingerprintId: string;
  fingerprintName: string;
  onClose: () => void;
}

/**
 * Modal creation-stats dashboard for one fingerprint's matched tokens: the
 * day x hour seasonality heatmap + calendar trend of the matched corpus, over
 * the paged token table (same columns + inspect-on-row-click as the Tokens
 * page).
 *
 * Charts and table read the same scope - one window picker drives both - and
 * clicking a heatmap tile narrows the table to that recurring weekly slot.
 */
function FingerprintMatchesModal({
  fingerprintId,
  fingerprintName,
  onClose,
}: FingerprintMatchesModalProps) {
  const { timezone } = useTimezone();
  const [query, setQuery] = useState<TableQuery>(INITIAL_QUERY);
  const columns = useMemo(() => tokenColumns(), []);
  const [inspected, setInspected] = useState<{ mint: string; symbol?: string } | null>(null);
  const flowSource = useFlowPatternSource(fingerprintId);

  const [win, setWin] = useState<CreationWindow>(INITIAL_WINDOW);
  const [bucket, setBucket] = useState<CreationBucket>('day');
  const [cell, setCell] = useState<{ dow: number; hour: number } | null>(null);

  // Same window vocabulary (civil-day shortcuts, rolling look-backs, custom
  // bounds) and the same lowering as every other creation-stats surface.
  const { from, to, spanDays } = useMemo(
    () => resolveCreationWindow(win, timezone),
    [win, timezone],
  );

  // Span-gated trend granularities, clamped so a window change never leaves an
  // out-of-range bucket selected (same contract as the Creation Stats page).
  const bucketOpts = useMemo(() => bucketOptionsForRange(spanDays), [spanDays]);
  const effBucket = clampBucketToRange(bucket, spanDays);

  const scope = useMemo<MatchScope>(
    () => ({ tz: timezone, from, to, cell }),
    [timezone, from, to, cell],
  );

  const statsQueryArgs = useMemo(
    () => statsArgs(fingerprintId, scope, effBucket),
    [fingerprintId, scope, effBucket],
  );
  const stats = useGetGroupedCreationStatsQuery(statsQueryArgs);
  const group = stats.data?.groups[0] ?? null;
  const heatCells = useMemo(
    () => (stats.data?.cells ?? []).map(toHeatCell),
    [stats.data?.cells],
  );

  const args = useMemo(
    () => scopedArgs(fingerprintId, query, scope),
    [fingerprintId, query, scope],
  );
  // Snaps the table to page 1 when the corpus changes under it (a new window
  // or a different weekly slot) - page 7 of the old result set is not a page of
  // the new one.
  const tableResetKey = `${from}:${to ?? ''}:dow=${cell?.dow ?? ''}:hour=${cell?.hour ?? ''}`;
  const { data, isFetching, isError, error } = useGetGroupedCreationTokensQuery(args);
  // Hold the last successful page so the table doesn't flash empty between pages.
  const itemsRef = useRef<TokenRecord[]>(EMPTY_ROWS);
  if (data?.items) itemsRef.current = data.items;
  const rows = data?.items ?? itemsRef.current;
  const total = data?.total ?? 0;

  return (
    <Modal
      title={`${fingerprintName} — matched tokens`}
      open
      onClose={onClose}
      size="xxl"
    >
      <div className="flex flex-col gap-3">
        <div className="flex flex-wrap items-center gap-2">
          <CreationWindowPicker
            aria-label="Creation window"
            value={win}
            onChange={setWin}
            timezone={timezone}
          />
          <span className="text-[11px] text-text-dim">
            matched tokens created in the window · {timezone}
          </span>
          {cell && (
            <Button
              size="sm"
              variant="subtle"
              active
              title="Clear the weekly-slot filter"
              onClick={() => setCell(null)}
            >
              {weeklySlotLabel(cell.dow, cell.hour)} (every week) ✕
            </Button>
          )}
        </div>

        <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
          <StatCard
            label="Matched tokens"
            value={group ? formatWithCommas(group.total) : '—'}
          />
          <StatCard
            label="Trades"
            value={group ? formatWithCommas(group.trades) : '—'}
          />
          <StatCard
            label="Trades per token"
            value={group ? group.trades_avg.toFixed(1) : '—'}
          />
        </div>

        {stats.isError && (
          <p className="text-red">
            {apiErrorMessage(stats.error, 'Failed to load creation stats')}
          </p>
        )}

        {/* Day x hour seasonality of the matched corpus - click a tile to narrow
            the table below to that recurring weekly slot. */}
        <section className="rounded-lg border border-white/8 bg-white/2 p-3">
          <div className="mb-2 flex items-center justify-between">
            <h3 className="text-sm font-semibold text-text">
              Creation seasonality — day × hour
            </h3>
            <span className="text-[10px] text-text-dim">
              shade = share of max · click a tile to filter the table
            </span>
          </div>
          {stats.isLoading ? (
            <LoadingState variant="inline" label="Loading heatmap…" />
          ) : heatCells.length > 0 ? (
            <CreationHeatmap
              cells={heatCells}
              metric="count"
              total={group?.total ?? 0}
              onCellClick={(dow, hour) =>
                setCell((prev) =>
                  prev && prev.dow === dow && prev.hour === hour ? null : { dow, hour },
                )
              }
              selectedCell={cell}
            />
          ) : (
            <p className="text-text-dim">No matched tokens in this window.</p>
          )}
        </section>

        {/* Calendar trend of the same corpus (one series - the scoped group). */}
        <section className="rounded-lg border border-white/8 bg-white/2 p-3">
          <div className="mb-2 flex items-center justify-between">
            <h3 className="text-sm font-semibold text-text">Creation trend</h3>
            <div className="flex items-center gap-1">
              {bucketOpts.map((o) => (
                <Button
                  key={o.value}
                  size="sm"
                  variant="subtle"
                  active={effBucket === o.value}
                  onClick={() => setBucket(o.value)}
                >
                  {o.label}
                </Button>
              ))}
            </div>
          </div>
          {stats.isLoading ? (
            <LoadingState variant="inline" label="Loading chart…" />
          ) : stats.data && stats.data.points.length > 0 ? (
            <Suspense fallback={<LoadingState variant="inline" label="Loading chart…" />}>
              <GroupedCreationTrendChart
                points={stats.data.points}
                groups={stats.data.groups}
              />
            </Suspense>
          ) : (
            <p className="text-text-dim">No matched tokens in this window.</p>
          )}
        </section>

        {isError ? (
          <p className="text-red">{apiErrorMessage(error, 'Failed to load matched tokens')}</p>
        ) : (
          <TokenTable
            columns={columns}
            rows={rows}
            existingKeys={ALL_TOKEN_INFO_KEYS}
            serverSide
            serverTotal={total}
            onQueryChange={setQuery}
            loading={isFetching}
            resetKey={tableResetKey}
            charts
            chartsDefaultOn
            flowPatternKeys={flowSource.keys}
            flowFingerprintId={flowSource.fingerprintId}
            searchable
            colToggle
            hoverable
            tableId="fingerprint_matched_tokens"
            emptyMessage="No tokens match this fingerprint in the window"
            selectedKey={inspected?.mint ?? null}
            onSelect={(mint) => {
              const row = mint ? rows.find((r) => r.mint_address === mint) : null;
              setInspected(mint ? { mint, symbol: row?.symbol } : null);
            }}
          />
        )}
      </div>
      {inspected && (
        <LazyLabTokenInspectModal
          target={inspectFromMint(inspected.mint, inspected.symbol)}
          titleSuffix="Token inspect"
          flowPatternKeys={flowSource.keys}
          onClose={() => setInspected(null)}
        />
      )}
    </Modal>
  );
}

export interface UseFingerprintMatches {
  /** Number of tokens the fingerprint matches in the window, or `null` when no
   *  fingerprint is selected / the count hasn't been requested yet. */
  count: number | null;
  countLoading: boolean;
  /** Start the cheap `pageSize:1` count fetch (idempotent). */
  ensureCount: () => void;
  /** Open the matched-tokens modal (also starts the count fetch). */
  openMatches: () => void;
  /** The modal element (rendered only while open) — drop into the page's JSX. */
  matchesModal: ReactNode;
}

/**
 * Wire a selected fingerprint to a matched-token count + an on-demand modal
 * table. Pass the result into `FingerprintScopeControl`'s `matchedCount` /
 * `matchedCountLoading` / `onViewMatches` / `onRequestMatchCount` props, and
 * render `matchesModal`.
 *
 * The count loads lazily (`ensureCount` / open modal) — a cheap `pageSize:1`
 * fetch reading `total`. The full table only fetches once the modal is opened.
 */
export function useFingerprintMatches(
  fingerprintId: string | null,
  fingerprintName?: string,
): UseFingerprintMatches {
  const [open, setOpen] = useState(false);
  const [countEnabled, setCountEnabled] = useState(false);

  // Count = the `total` of a 1-row page; no per-column filters (the raw match
  // count for the whole window). Reuses the modal's cache-adjacent query shape.
  const countArgs =
    fingerprintId && countEnabled
      ? scopedArgs(fingerprintId, { ...INITIAL_QUERY, page: 1, pageSize: 1 })
      : skipToken;
  const { data: countData, isFetching: countLoading } =
    useGetGroupedCreationTokensQuery(countArgs);
  const count = fingerprintId && countEnabled ? countData?.total ?? null : null;

  // Clearing / switching the fingerprint closes any open modal and resets the
  // lazy count gate so a newly picked id doesn't flash the previous total.
  useEffect(() => {
    if (!fingerprintId) setOpen(false);
    setCountEnabled(false);
  }, [fingerprintId]);

  const ensureCount = useCallback(() => {
    if (fingerprintId) setCountEnabled(true);
  }, [fingerprintId]);

  const openMatches = useCallback(() => {
    if (!fingerprintId) return;
    setCountEnabled(true);
    setOpen(true);
  }, [fingerprintId]);

  const matchesModal =
    open && fingerprintId ? (
      <FingerprintMatchesModal
        key={fingerprintId}
        fingerprintId={fingerprintId}
        fingerprintName={fingerprintName || fingerprintId.slice(0, 8)}
        onClose={() => setOpen(false)}
      />
    ) : null;

  return {
    count,
    countLoading: fingerprintId && countEnabled ? countLoading : false,
    ensureCount,
    openMatches,
    matchesModal,
  };
}

export interface UseFingerprintMatchesFor {
  /** Open the matches dashboard for this fingerprint (row button click). */
  open: (fingerprint: { id: string; name?: string | null }) => void;
  /** The modal element (rendered only while open) — drop into the page's JSX. */
  modal: ReactNode;
}

/**
 * Row-driven variant of {@link useFingerprintMatches}: instead of following one
 * pre-selected fingerprint, the caller passes the fingerprint at click time —
 * what a table of fingerprints needs, where every row can open its own matches.
 * Same modal (creation-stats heatmap + trend + token table), no count chip.
 */
export function useFingerprintMatchesFor(): UseFingerprintMatchesFor {
  const [target, setTarget] = useState<{ id: string; name: string } | null>(null);

  const open = useCallback((fingerprint: { id: string; name?: string | null }) => {
    setTarget({
      id: fingerprint.id,
      name: fingerprint.name || fingerprint.id.slice(0, 8),
    });
  }, []);

  const modal = target ? (
    <FingerprintMatchesModal
      key={target.id}
      fingerprintId={target.id}
      fingerprintName={target.name}
      onClose={() => setTarget(null)}
    />
  ) : null;

  return { open, modal };
}
