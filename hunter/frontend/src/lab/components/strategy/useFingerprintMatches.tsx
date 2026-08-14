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
// Count fetch is **lazy**: it starts on `ensureCount` (hover the chip) or when
// the matches modal opens — not on every page mount with a persisted seed id.

import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { skipToken } from '@reduxjs/toolkit/query/react';
import { Modal } from 'components/ui/Modal';
import { TokenTable } from 'components/tokens/TokenTable';
import { ALL_TOKEN_INFO_KEYS } from 'components/tokens/sharedTokenColumns';
import { tokenColumns } from 'components/tokens/tokenColumns';
import { inspectFromMint } from 'components/strategy/inspectTarget';
import { LazyLabTokenInspectModal } from '@lab/components/strategy/LazyLabTokenInspectModal';
import { useFlowPatternSource } from 'hooks/useFlowPatternKeys';
import { apiErrorMessage } from 'store/apiSlice';
import { useGetGroupedCreationTokensQuery } from '@lab/store/labEndpoints';
import {
  drillTokenFilters,
  type GroupedCreationTokensArgs,
} from '@lab/components/creation-stats/groupedCreationStats';
import type { TableQuery } from 'components/table/types';
import type { TokenRecord } from 'types';

/** Look-back window (days) the scoped endpoint applies when no `from` is sent —
 *  mirrors the backend `DEFAULT_WINDOW_DAYS` in `creation_stats.rs`. Surfaced so
 *  the count chip / modal can label the window honestly. */
export const FINGERPRINT_MATCH_WINDOW_DAYS = 30;

const INITIAL_QUERY: TableQuery = {
  page: 1,
  pageSize: 25,
  sortKeys: [],
  search: '',
  colFilters: {},
};

/** Stable empty reference so the table doesn't remount while a page loads. */
const EMPTY_ROWS: TokenRecord[] = [];

/** Build the scoped-token args (fingerprint scope ⇒ group_by/group_key ignored
 *  server-side; we still pass the required-but-ignored fields). `from` omitted ⇒
 *  the backend's default {@link FINGERPRINT_MATCH_WINDOW_DAYS}-day window. */
function scopedArgs(fingerprintId: string, query: TableQuery): GroupedCreationTokensArgs {
  return {
    tz: 'UTC',
    segment: 'all',
    groupBy: [],
    groupKey: {},
    fingerprintId,
    page: query.page,
    pageSize: query.pageSize,
    sortKeys: query.sortKeys,
    search: query.search,
    filters: drillTokenFilters(query),
  };
}

interface FingerprintMatchesModalProps {
  fingerprintId: string;
  fingerprintName: string;
  onClose: () => void;
}

/** Modal token table of the fingerprint's matched tokens (paged server-side),
 *  with the same columns + inspect-on-row-click as the Tokens page. */
function FingerprintMatchesModal({
  fingerprintId,
  fingerprintName,
  onClose,
}: FingerprintMatchesModalProps) {
  const [query, setQuery] = useState<TableQuery>(INITIAL_QUERY);
  const columns = useMemo(() => tokenColumns(), []);
  const [inspected, setInspected] = useState<{ mint: string; symbol?: string } | null>(null);
  const flowSource = useFlowPatternSource(fingerprintId);

  const args = useMemo(() => scopedArgs(fingerprintId, query), [fingerprintId, query]);
  const { data, isFetching, isError, error } = useGetGroupedCreationTokensQuery(args);
  // Hold the last successful page so the table doesn't flash empty between pages.
  const itemsRef = useRef<TokenRecord[]>(EMPTY_ROWS);
  if (data?.items) itemsRef.current = data.items;
  const rows = data?.items ?? itemsRef.current;
  const total = data?.total ?? 0;

  return (
    <Modal
      title={`${fingerprintName} — matched tokens (last ${FINGERPRINT_MATCH_WINDOW_DAYS}d)`}
      open
      onClose={onClose}
      size="xxl"
    >
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
