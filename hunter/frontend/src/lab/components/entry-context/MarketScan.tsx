import { useCallback, useEffect, useMemo, useState } from 'react';
import type { SerializedError } from '@reduxjs/toolkit';
import type { FetchBaseQueryError } from '@reduxjs/toolkit/query';
import { skipToken } from '@reduxjs/toolkit/query/react';
import type { ColumnDef } from 'components/table/types';
import type { ChartEventMarker } from 'components/token-price-chart';
import { tokenColumns } from 'components/tokens/tokenColumns';
import { TokenTable } from 'components/tokens/TokenTable';
import type { ChartOverlayHook } from 'components/tokens/TokenChartsGrid';
import { ALL_TOKEN_INFO_KEYS } from 'components/tokens/sharedTokenColumns';
import { Accordion } from 'components/ui/Accordion';
import { Button } from 'components/ui/Button';
import { inspectFromMint } from 'components/strategy/inspectTarget';
import { ACCORDION_IDS } from 'lib/storage';
import { apiErrorMessage } from 'store/apiSlice';
import { useLazyGetTokensBatchQuery } from 'store/sharedEndpoints';
import type { TokenRecord } from 'types';
import { shortAddr } from '@lab/components/analysis/traderQuery';
import { LazyLabTokenInspectModal } from '@lab/components/strategy/LazyLabTokenInspectModal';
import type { EntryLogic } from '@lab/lib/entryContext/logic';
import type { EntryScanRequest, ScanMoment } from '@lab/lib/entryContext/types';
import { useGetEntryScanQuery } from '@lab/store/labEndpoints';

/** `POST /api/tokens/batch` cap. */
const BATCH = 500;

type MarketRow = TokenRecord & { points: number };

function asApiError(e: unknown): FetchBaseQueryError | SerializedError | undefined {
  if (typeof e === 'object' && e != null && ('status' in e || 'message' in e)) {
    return e as FetchBaseQueryError | SerializedError;
  }
  return undefined;
}

const ideaOf = (logic: EntryLogic) =>
  logic.conditions.filter((c) => c.kind === 'signal' && !c.key.startsWith('pe_'));

function markersOf(moments: readonly ScanMoment[]): ChartEventMarker[] {
  const out: ChartEventMarker[] = [];
  for (const m of moments) {
    if (m.price == null || !(m.price > 0)) continue;
    out.push({
      kind: 'entry',
      role: 'signal',
      time: m.at,
      priceInSol: m.price,
      label: 'Target',
    });
  }
  return out;
}

/** Token records for a mint list, in batches of {@link BATCH}. */
function useMintRecords(mints: readonly string[]) {
  const [load] = useLazyGetTokensBatchQuery();
  const [rows, setRows] = useState<TokenRecord[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const key = mints.join(',');
  useEffect(() => {
    if (mints.length === 0) {
      setRows([]);
      setError(null);
      return;
    }
    let cancel = false;
    setRows(null);
    setError(null);
    (async () => {
      try {
        const out: TokenRecord[] = [];
        for (let i = 0; i < mints.length; i += BATCH) {
          const part = await load(mints.slice(i, i + BATCH), true).unwrap();
          out.push(...part);
        }
        if (!cancel) setRows(out);
      } catch (e) {
        if (!cancel) {
          setError(apiErrorMessage(asApiError(e), 'Failed to load market tokens') ?? 'Failed to load market tokens');
        }
      }
    })();
    return () => {
      cancel = true;
    };
    // `key` is the mint list. `load` is the lazy trigger.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, load]);
  return { rows, error, loading: mints.length > 0 && rows == null && error == null };
}

/**
 * The market, apart from his token table. Scan finds every buy of the selected
 * structure in the range. A token is listed when the analysis window before that
 * buy passes the buys table's filters. Its chart marks those buys, and his.
 */
export function MarketScan({
  scanRequest,
  logic,
  wallet,
  flowPatternKeys,
  hasTarget,
}: {
  scanRequest: EntryScanRequest | null;
  logic: EntryLogic;
  wallet: string;
  flowPatternKeys: ReadonlySet<string> | null;
  hasTarget: boolean;
}) {
  const key = JSON.stringify(scanRequest);
  const [armedFor, setArmedFor] = useState<string | null>(null);
  const armed = hasTarget && scanRequest != null && armedFor === key;
  const scan = useGetEntryScanQuery(armed ? scanRequest : skipToken);
  const market = armed && !scan.isFetching ? scan.data : undefined;

  const matched = useMemo(
    () => (market?.moments ?? []).filter((m) => !m.unknown_reason && logic.idea(m)),
    [market, logic],
  );
  const byMint = useMemo(() => {
    const map = new Map<string, ScanMoment[]>();
    for (const m of matched) {
      const list = map.get(m.mint_address);
      if (list) list.push(m);
      else map.set(m.mint_address, [m]);
    }
    return map;
  }, [matched]);
  const mints = useMemo(() => [...byMint.keys()], [byMint]);
  const records = useMintRecords(market ? mints : []);

  const markers = useMemo(() => {
    const map = new Map<string, ChartEventMarker[]>();
    for (const [mint, moments] of byMint) map.set(mint, markersOf(moments));
    return map;
  }, [byMint]);
  const useOverlay = useCallback<ChartOverlayHook<MarketRow>>(
    (row) => ({ eventMarkers: markers.get(row.mint_address) ?? [] }),
    [markers],
  );

  const rows = useMemo<MarketRow[]>(() => {
    if (!records.rows) return [];
    const have = new Map(records.rows.map((r) => [r.mint_address, r]));
    const out: MarketRow[] = [];
    for (const mint of mints) {
      const rec = have.get(mint);
      const points = byMint.get(mint)?.length ?? 0;
      if (rec) out.push({ ...rec, points });
    }
    return out;
  }, [records.rows, mints, byMint]);

  const columns = useMemo(() => {
    const base = tokenColumns() as unknown as ColumnDef<MarketRow>[];
    let lastIdentity = -1;
    base.forEach((c, i) => {
      if (c.group === 'identity') lastIdentity = i;
    });
    const points: ColumnDef<MarketRow> = {
      key: 'mkt_points',
      label: 'Points',
      group: 'market',
      tooltip: 'Buys of the selected structure in the range whose analysis window passes the filters.',
      render: (r) => r.points.toLocaleString(),
      sortValue: (r) => r.points,
      searchValue: () => '',
      filterNumber: (r) => r.points,
    };
    return [...base.slice(0, lastIdentity + 1), points, ...base.slice(lastIdentity + 1)];
  }, []);

  const [inspected, setInspected] = useState<{ mint: string; symbol?: string } | null>(null);
  const idea = ideaOf(logic);
  const missing = market ? mints.length - rows.length : 0;

  return (
    <div className="mb-4 flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-3 text-xs">
        <Button
          size="xs"
          variant="ghost"
          active={armed}
          disabled={!hasTarget || scan.isFetching}
          onClick={() => setArmedFor(key)}
        >
          {market ? 'Market scanned' : 'Scan market'}
        </Button>
        <span className="text-text-dim">
          {!hasTarget
            ? 'Pick a pattern set above. A point is a buy of that structure.'
            : !armed
              ? 'Finds every buy of the selected structure in the range. A token is listed when the window before that buy passes the filters.'
              : scan.isFetching
                ? 'Scanning the market…'
                : scan.error
                  ? apiErrorMessage(scan.error, 'The market scan failed')
                  : market
                    ? `${matched.length.toLocaleString()} points on ${mints.length.toLocaleString()} tokens${market.truncated ? ' · cut at the cap, most recent only' : ''}${missing > 0 && !records.loading ? ` · ${missing.toLocaleString()} have no token record` : ''}`
                    : null}
        </span>
      </div>
      {market && (
        <p className="text-[11px] text-text-dim">
          {idea.length > 0
            ? `Filters: ${idea.map((c) => `${c.label} ${c.text}`).join(', ')}.`
            : 'No window filter is set, so every buy of the structure is a point.'}{' '}
          Charts mark those buys. His buys stay marked.
        </p>
      )}
      {records.error != null && <p className="text-[11px] text-red">{records.error}</p>}
      {market && mints.length > 0 && (
        <Accordion
          title={`Market (${rows.length.toLocaleString()})`}
          padding="sm"
          bordered={false}
          storageKey={ACCORDION_IDS.entryContextMarket}
        >
          <TokenTable
            columns={columns}
            rows={rows}
            existingKeys={ALL_TOKEN_INFO_KEYS}
            charts
            chartsDefaultOn
            searchable
            colFilters
            colToggle
            hoverable
            loading={records.loading}
            groupLabels={{ market: 'Market' }}
            tableId="entry_context_market"
            defaultSort={{ col: 'mkt_points', dir: 'desc' }}
            resetKey={`${mints.length}|${idea.map((c) => c.text).join(',')}`}
            highlightWallet={wallet}
            useRowOverlay={useOverlay}
            titleOf={(r) => r.symbol || r.name || shortAddr(r.mint_address)}
            selectedKey={inspected?.mint ?? null}
            onSelect={(mint) => {
              const row = mint ? rows.find((r) => r.mint_address === mint) : null;
              setInspected(mint ? { mint, symbol: row?.symbol } : null);
            }}
            emptyMessage="No token in the market passes the filters"
            flowPatternKeys={flowPatternKeys}
          />
        </Accordion>
      )}
      {inspected && (
        <LazyLabTokenInspectModal
          target={inspectFromMint(inspected.mint, inspected.symbol)}
          titleSuffix="Market"
          flowPatternKeys={flowPatternKeys}
          eventMarkers={markers.get(inspected.mint) ?? []}
          onClose={() => setInspected(null)}
        />
      )}
    </div>
  );
}
