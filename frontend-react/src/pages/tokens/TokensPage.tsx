import { useCallback, useEffect, useMemo, useState } from 'react';
import { DataTable } from '../../components/table/DataTable';
import { FilterPanel } from '../../components/tokens/FilterPanel';
import { TokenDetailPanel } from '../../components/tokens/TokenDetailPanel';
import { tokenColumns } from '../../components/tokens/tokenColumns';
import {
  activeFilterCount,
  defaultFilters,
  filtersEmpty,
  loadStoredTokenFilters,
  saveStoredTokenFilters,
  tokenPassesFilters,
  type TokenFilters,
} from '../../components/tokens/filters';
import { Button } from '../../components/ui/Button';
import { StatusButton } from '../../components/ui/StatusButton';
import { fetchTokenDetail, fetchTokens } from '../../services/api';
import { POLL_INTERVAL_MS } from '../../services/config';
import type { TokenDetailRecord, TokenRecord } from '../../types';
import { usePriceDisplay } from '../../hooks/usePriceDisplay';
import { cn } from '../../lib/cn';

const LS_LIVE_KEY = 'tokens_live';

function loadLive(): boolean {
  try {
    return localStorage.getItem(LS_LIVE_KEY) === 'true';
  } catch {
    return false;
  }
}

export function TokensPage() {
  const price = usePriceDisplay();
  const columns = useMemo(() => tokenColumns(price), [price]);

  const [tokens, setTokens] = useState<TokenRecord[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [live, setLive] = useState(loadLive);
  const [showFilters, setShowFilters] = useState(false);
  const [filters, setFilters] = useState<TokenFilters>(loadStoredTokenFilters);
  const [selectedMint, setSelectedMint] = useState<string | null>(null);
  const [detail, setDetail] = useState<TokenDetailRecord | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailError, setDetailError] = useState<string | null>(null);

  const loadTokens = useCallback(async (silent = false) => {
    if (!silent) setLoading(true);
    try {
      const result = await fetchTokens('', 5000, 0);
      setTokens(result.items);
      setTotal(result.total);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load tokens');
    } finally {
      if (!silent) setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadTokens();
  }, [loadTokens]);

  useEffect(() => {
    if (!live) return;
    const id = setInterval(() => loadTokens(true), POLL_INTERVAL_MS);
    return () => clearInterval(id);
  }, [live, loadTokens]);

  useEffect(() => {
    try {
      localStorage.setItem(LS_LIVE_KEY, live ? 'true' : 'false');
    } catch {
      /* ignore */
    }
  }, [live]);

  useEffect(() => {
    if (!selectedMint) {
      setDetail(null);
      setDetailError(null);
      setDetailLoading(false);
      return;
    }
    setDetailLoading(true);
    setDetailError(null);
    setDetail(null);
    fetchTokenDetail(selectedMint)
      .then(setDetail)
      .catch((e) => setDetailError(e instanceof Error ? e.message : 'Failed to load detail'))
      .finally(() => setDetailLoading(false));
  }, [selectedMint]);

  useEffect(() => {
    if (!selectedMint) return;
    const t = setTimeout(() => {
      document.getElementById(`detail-${selectedMint}`)?.scrollIntoView({
        behavior: 'smooth',
        block: 'nearest',
      });
    }, 300);
    return () => clearTimeout(t);
  }, [selectedMint, detail]);

  const displayed = useMemo(() => {
    if (filtersEmpty(filters)) return tokens;
    return tokens.filter((t) => tokenPassesFilters(filters, t));
  }, [tokens, filters]);

  const filterCount = activeFilterCount(filters);

  return (
    <div>
      <div className="mb-3.5 flex flex-wrap items-center gap-3">
        <h2 className="text-lg font-extrabold text-text">Tokens</h2>
        <span className="rounded-md border border-primary bg-primary/15 px-2.5 py-0.5 font-mono text-[11px] font-bold tracking-wide text-primary">
          {total} tracked
        </span>
        <StatusButton
          state={live ? 'live' : 'dead'}
          label={live ? 'ACTIVE' : 'PAUSED'}
          onClick={() => setLive((v) => !v)}
          className={cn(
            'rounded px-2 py-0.5 text-[10px]',
            live && 'animate-pulse',
          )}
        />
      </div>

      <div className="mb-1.5 flex gap-1.5">
        <Button
          variant="subtle"
          size="sm"
          active={showFilters || filterCount > 0}
          onClick={() => setShowFilters((v) => !v)}
        >
          {filterCount > 0 ? `Global Filters (${filterCount})` : 'Global Filters'}
        </Button>
      </div>

      {showFilters && (
        <FilterPanel
          filters={filters}
          onApply={(next) => {
            setFilters(next);
            saveStoredTokenFilters(next);
          }}
          onClear={() => {
            const empty = defaultFilters();
            setFilters(empty);
            saveStoredTokenFilters(empty);
          }}
        />
      )}

      {loading && <p className="text-text-dim">Loading tokens…</p>}
      {error && !loading && <p className="text-red">{error}</p>}
      {!loading && !error && (
        <DataTable
          columns={columns}
          rows={displayed}
          rowKey={(r) => r.mint_address}
          selectedKey={selectedMint}
          onSelect={setSelectedMint}
          rowDetail={() => (
            <TokenDetailPanel detail={detail} loading={detailLoading} error={detailError} />
          )}
          searchable
          colFilters
          colToggle
          hoverable
          storageKey="tokens_visible_cols"
          emptyMessage="No tokens found"
        />
      )}
    </div>
  );
}
