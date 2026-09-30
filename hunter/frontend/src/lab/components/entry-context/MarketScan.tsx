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
import { Checkbox } from 'components/ui/Checkbox';
import { Input } from 'components/ui/Input';
import { inspectFromMint } from 'components/strategy/inspectTarget';
import { RelativeTimeCell } from 'components/table/RelativeTimeCell';
import { useTimezone } from 'context/TimezoneContext';
import { useLocalStorage } from 'hooks/useLocalStorage';
import { ACCORDION_IDS, STORAGE_KEYS } from 'lib/storage';
import { apiErrorMessage } from 'store/apiSlice';
import { useLazyGetTokensBatchQuery } from 'store/sharedEndpoints';
import { formatIso } from 'utils/date';
import type { TokenRecord } from 'types';
import { shortAddr } from '@lab/components/analysis/traderQuery';
import { LazyLabTokenInspectModal } from '@lab/components/strategy/LazyLabTokenInspectModal';
import { type Chance, findChances, gapStats, PAUSE_STEPS, pointPauses, suggestPause } from '@lab/lib/entryContext/chances';
import type { EntryLogic } from '@lab/lib/entryContext/logic';
import { type AreaKey, type HisToken, overlap } from '@lab/lib/entryContext/overlap';
import type { EntryScanRequest, ScanMoment } from '@lab/lib/entryContext/types';
import { useGetEntryScanQuery, useLazyGetEntryScanQuery } from '@lab/store/labEndpoints';
import { PassTable } from './StatTable';
import { HelpTip, Terms } from './HelpText';
import { areaSentence, OverlapDiagram } from './OverlapDiagram';
import { CHANCE_HELP, MARKET_HELP } from './summaryHelp';
import { type TokenView, TokenViewToggle } from './TokenViewToggle';

/** `POST /api/tokens/batch` cap. */
const BATCH = 500;

type MarketRow = TokenRecord & { chances: number; points: number };

/** Max pause used when the chance count never settles, seconds. */
const FALLBACK_PAUSE_SECS = 30;

function asApiError(e: unknown): FetchBaseQueryError | SerializedError | undefined {
  if (typeof e === 'object' && e != null && ('status' in e || 'message' in e)) {
    return e as FetchBaseQueryError | SerializedError;
  }
  return undefined;
}

const ideaOf = (logic: EntryLogic) =>
  logic.conditions.filter((c) => c.kind === 'signal' && !c.key.startsWith('pe_'));

/** One marker per chance, at its first point; with `showPoints`, each later point too. */
function markersOf(chances: readonly Chance[], showPoints: boolean): ChartEventMarker[] {
  const out: ChartEventMarker[] = [];
  const push = (m: ScanMoment, label: string) => {
    if (m.price == null || !(m.price > 0)) return;
    out.push({ kind: 'entry', role: 'signal', time: m.at, priceInSol: m.price, label });
  };
  for (const c of chances) {
    push(c.points[0], c.points.length > 1 ? `Chance · ${c.points.length} pts` : 'Chance');
    if (showPoints) for (const m of c.points.slice(1)) push(m, '·');
  }
  return out;
}

const count = (v: number) => v.toLocaleString();

const fmtSecs =(s: number) => (s < 10 ? s.toFixed(1).replace(/\.0$/, '') : Math.round(s).toLocaleString());

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
 * structure in the range (the pool). The summary counts those buys and their tokens
 * before and after the filters, and every token traded in the range. The token
 * table lists the pool's tokens, or the ones with a buy that passes the filters.
 * Its chart marks those buys, and his.
 *
 * The request Scan was pressed for is saved, and the backend stores each scan's
 * result, so the same request shows its result again without a scan. Re-scan
 * reads the market again for that request.
 *
 * The diagram sets his tokens against the market's (`overlap`); a count in it
 * lists its tokens in the token table.
 */
export function MarketScan({
  scanRequest,
  logic,
  wallet,
  flowPatternKeys,
  hasTarget,
  pool,
  his,
}: {
  scanRequest: EntryScanRequest | null;
  logic: EntryLogic;
  wallet: string;
  flowPatternKeys: ReadonlySet<string> | null;
  hasTarget: boolean;
  /** Which IXs are selected, in words: `the pattern set "6Vo3"`. */
  pool: string;
  /** Each token he bought in the range, with its class. */
  his: ReadonlyMap<string, HisToken>;
}) {
  const key = JSON.stringify(scanRequest);
  const { timezone } = useTimezone();
  const [armedFor, setArmedFor] = useLocalStorage<string | null>(STORAGE_KEYS.entryContextScan, null);
  const armed = hasTarget && scanRequest != null && armedFor === key;
  const scan = useGetEntryScanQuery(armed ? scanRequest : skipToken);
  const [rescan] = useLazyGetEntryScanQuery();
  // The result of this request; a re-scan keeps it on screen until the new one lands.
  const market = armed ? scan.currentData : undefined;

  const moments = useMemo(() => market?.moments ?? [], [market]);
  const isPoint = useCallback((m: ScanMoment) => logic.idea(m), [logic]);
  // Off = a failing buy is passed over; only the pause ends a chance.
  const [failEnds, setFailEnds] = useState(true);
  const stats = useMemo(() => gapStats(pointPauses(moments, isPoint, failEnds)), [moments, isPoint, failEnds]);
  const sensitivity = useMemo(
    () => PAUSE_STEPS.map((p) => [p, findChances(moments, isPoint, p, failEnds).length] as const),
    [moments, isPoint, failEnds],
  );
  const suggested = useMemo(() => suggestPause(sensitivity), [sensitivity]);
  // `null` = follow the suggestion.
  const [pauseInput, setPauseInput] = useState<number | null>(null);
  const maxPause = pauseInput ?? suggested ?? FALLBACK_PAUSE_SECS;
  const [showPoints, setShowPoints] = useState(false);

  const chances = useMemo(
    () => findChances(moments, isPoint, maxPause, failEnds),
    [moments, isPoint, maxPause, failEnds],
  );
  const points = useMemo(() => chances.reduce((n, c) => n + c.points.length, 0), [chances]);
  const byMint = useMemo(() => {
    const map = new Map<string, Chance[]>();
    for (const c of chances) {
      const list = map.get(c.mint_address);
      if (list) list.push(c);
      else map.set(c.mint_address, [c]);
    }
    return map;
  }, [chances]);
  const mints = useMemo(() => [...byMint.keys()], [byMint]);
  // The pool: tokens with a buy the scan could read.
  const poolMints = useMemo(
    () => [...new Set(moments.filter((m) => !m.unknown_reason).map((m) => m.mint_address))],
    [moments],
  );
  const poolBuys = useMemo(() => moments.filter((m) => !m.unknown_reason).length, [moments]);
  const [view, setView] = useState<TokenView>('pass');
  const areas = useMemo(() => overlap(his, poolMints, mints), [his, poolMints, mints]);
  // An area picked in the diagram: the token table lists it, over the Pool / Pass view.
  const [area, setArea] = useState<AreaKey | null>(null);
  const listed = area ? areas[area] : view === 'pool' ? poolMints : mints;
  const ends = useMemo(() => {
    const n = { fail: 0, pause: 0, last: 0 };
    for (const c of chances) n[c.end] += 1;
    return n;
  }, [chances]);
  const records = useMintRecords(market ? listed : []);

  const markers = useMemo(() => {
    const map = new Map<string, ChartEventMarker[]>();
    for (const [mint, cs] of byMint) map.set(mint, markersOf(cs, showPoints));
    return map;
  }, [byMint, showPoints]);
  const useOverlay = useCallback<ChartOverlayHook<MarketRow>>(
    (row) => ({ eventMarkers: markers.get(row.mint_address) ?? [] }),
    [markers],
  );

  const rows = useMemo<MarketRow[]>(() => {
    if (!records.rows) return [];
    const have = new Map(records.rows.map((r) => [r.mint_address, r]));
    const out: MarketRow[] = [];
    for (const mint of listed) {
      const rec = have.get(mint);
      const cs = byMint.get(mint) ?? [];
      const n = cs.reduce((k, c) => k + c.points.length, 0);
      if (rec) out.push({ ...rec, chances: cs.length, points: n });
    }
    return out;
  }, [records.rows, listed, byMint]);

  const columns = useMemo(() => {
    const base = tokenColumns() as unknown as ColumnDef<MarketRow>[];
    let lastIdentity = -1;
    base.forEach((c, i) => {
      if (c.group === 'identity') lastIdentity = i;
    });
    const chanceCol: ColumnDef<MarketRow> = {
      key: 'mkt_chances',
      label: 'Chances',
      group: 'market',
      tooltip:
        'Separate opportunities on this token: a run of points, ended by a structure buy that fails the filters or a pause longer than Max pause.',
      render: (r) => r.chances.toLocaleString(),
      sortValue: (r) => r.chances,
      searchValue: () => '',
      filterNumber: (r) => r.chances,
    };
    const pointCol: ColumnDef<MarketRow> = {
      key: 'mkt_points',
      label: 'Points',
      group: 'market',
      tooltip: 'Points: buys of the selected structure whose analysis window passes the filters.',
      render: (r) => r.points.toLocaleString(),
      sortValue: (r) => r.points,
      searchValue: () => '',
      filterNumber: (r) => r.points,
    };
    const hisCol: ColumnDef<MarketRow> = {
      key: 'mkt_his',
      label: 'His buys',
      group: 'market',
      tooltip: 'His buys on this token in the range, and how many of them are at a point.',
      render: (r) => {
        const t = his.get(r.mint_address);
        return t ? `${t.buys.toLocaleString()}${t.atPoint > 0 ? ` · ${t.atPoint.toLocaleString()} at a point` : ''}` : '-';
      },
      sortValue: (r) => his.get(r.mint_address)?.buys ?? 0,
      searchValue: () => '',
      filterNumber: (r) => his.get(r.mint_address)?.buys ?? 0,
    };
    return [...base.slice(0, lastIdentity + 1), chanceCol, pointCol, hisCol, ...base.slice(lastIdentity + 1)];
  }, [his]);

  const [inspected, setInspected] = useState<{ mint: string; symbol?: string } | null>(null);
  const idea = ideaOf(logic);
  const ixFilters = idea.filter((c) => c.needsIxs).length;
  const missing = market ? listed.length - rows.length : 0;

  return (
    <div className="mb-4 flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-3 text-xs">
        {market && scanRequest ? (
          <Button
            variant="primary"
            disabled={scan.isFetching}
            title={
              scanRequest.to == null
                ? 'Reads the market again from the same start. The range ends now, so it also takes the trades since this scan.'
                : 'Reads the market again for the same range.'
            }
            onClick={() => rescan({ ...scanRequest, refresh: true })}
          >
            Re-scan
          </Button>
        ) : (
          <Button
            variant="primary"
            disabled={!hasTarget || scan.isFetching}
            title="Reads every buy made with the selected IXs in the range, and checks the filters right after each one, as a follower sees it."
            onClick={() => setArmedFor(key)}
          >
            Scan market
          </Button>
        )}
        {market && (
          <span className="flex items-center gap-1 text-text">
            Scanned {formatIso(market.scanned_at, timezone)} (<RelativeTimeCell iso={market.scanned_at} />)
          </span>
        )}
        <span className="text-text-dim">
          {!hasTarget
            ? 'Pick a pattern set above.'
            : scan.isFetching
              ? 'Scanning the market…'
              : scan.error
                ? apiErrorMessage(scan.error, 'The market scan failed')
                : market
                  ? [
                      market.truncated && 'Cut at the cap: most recent buys only.',
                      missing > 0 && !records.loading && `${count(missing)} tokens have no token record.`,
                    ]
                      .filter(Boolean)
                      .join(' ')
                  : null}
        </span>
      </div>
      {market && (
        <PassTable
          groups={['Buys', 'Tokens']}
          help={MARKET_HELP.table({ all: poolMints.length, pass: mints.length }, market.mints)}
          rows={[
            {
              key: 'pool',
              label: 'In pool',
              sub: 'buys made with the selected IXs, by any wallet',
              help: MARKET_HELP.pool(pool, { all: poolBuys, pass: points }),
              uses: 'every',
              usesNote: `${ixFilters} + ${idea.length - ixFilters} set`,
              cells: [
                {
                  all: poolBuys,
                  pass: points,
                  tip: `${count(points)} of the ${count(poolBuys)} buys made with the selected IXs pass the filters.`,
                },
                {
                  all: poolMints.length,
                  pass: mints.length,
                  tip: `${count(mints.length)} of their ${count(poolMints.length)} tokens have a buy that passes the filters.`,
                },
              ],
              highlight: true,
            },
            {
              key: 'all',
              label: 'All market',
              sub: 'every token traded in the range',
              help: MARKET_HELP.all(market.mints),
              uses: 'any',
              usesNote: 'not read yet',
              cells: [
                null,
                {
                  all: market.mints,
                  pass: null,
                  tip: `${count(market.mints)} tokens traded in the range. How many pass is not read yet.`,
                },
              ],
              missingTip: MARKET_HELP.missing,
            },
          ]}
        />
      )}
      {market && (
        <div className="rounded-md border border-white/8 bg-white/2 px-3 py-2">
          <p className="text-[11px] text-text-dim">
            <span className="font-semibold uppercase tracking-wider">His tokens against the market</span> · drawn
            to scale · click a number to list its tokens
          </p>
          <OverlapDiagram
            areas={areas}
            pool={poolMints.length}
            pass={mints.length}
            his={his.size}
            target={pool}
            selected={area}
            onSelect={setArea}
          />
        </div>
      )}
      {market && (
        <div className="flex flex-wrap items-center gap-x-5 gap-y-2 rounded-md border border-white/8 bg-white/2 px-3 py-2 text-[11px] text-text-dim">
          <span className="flex items-center gap-2">
            <span className="flex items-center gap-1 font-semibold uppercase tracking-wider">
              Chances
              <HelpTip
                title="Chances"
                help={CHANCE_HELP.chances({ chances: chances.length, passing: points, failEnds, ends })}
              />
            </span>
            <span className="text-lg font-semibold leading-none text-primary">{count(chances.length)}</span>
          </span>
          <span
            className="flex items-center gap-2"
            title="Points: the buys made with the selected IXs that pass the filters. Points close together on one token are one chance."
          >
            <span className="font-semibold uppercase tracking-wider">Points</span>
            <span className="text-lg font-semibold leading-none text-text">{count(points)}</span>
            <span>on {count(mints.length)} tokens</span>
          </span>
          <span className="flex items-center gap-1.5">
            Max pause (s)
            <Input
              numeric
              numericValue={pauseInput ?? maxPause}
              onNumericChange={(v) => setPauseInput(v != null && v >= 0 ? v : null)}
              min={0}
              className="w-[70px]"
            />
            <HelpTip
              title="Max pause"
              help={CHANCE_HELP.pause({
                suggested: suggested != null ? `${fmtSecs(suggested)} s` : null,
                fallbackSecs: FALLBACK_PAUSE_SECS,
                pauses: stats
                  ? `min ${fmtSecs(stats.min)} · median ${fmtSecs(stats.median)} · max ${fmtSecs(stats.max)} s`
                  : null,
                counts: sensitivity,
              })}
            />
            {pauseInput == null ? (
              <span>{suggested != null ? 'suggested' : 'default'}</span>
            ) : (
              <Button size="xs" variant="ghost" onClick={() => setPauseInput(null)}>
                Use suggested
              </Button>
            )}
          </span>
          <label
            className="flex items-center gap-1.5"
            title="On: a buy made with the selected IXs that fails the filters ends the chance. Off: it is passed over, and only a pause longer than Max pause ends one."
          >
            <Checkbox boxSize="sm" checked={failEnds} onChange={(e) => setFailEnds(e.target.checked)} />
            A failing buy ends a chance
          </label>
          <label
            className="flex items-center gap-1.5"
            title="Charts mark each chance at its first passing buy. On: every later passing buy is marked too."
          >
            <Checkbox boxSize="sm" checked={showPoints} onChange={(e) => setShowPoints(e.target.checked)} />
            Mark every passing buy
          </label>
        </div>
      )}
      {records.error != null && <p className="text-[11px] text-red">{records.error}</p>}
      {market && (poolMints.length > 0 || his.size > 0) && (
        <Accordion
          toggleLabel="Market tokens"
          header={
            <span className="flex flex-wrap items-center gap-2">
              <TokenViewToggle
                value={view}
                onChange={(v) => {
                  setArea(null);
                  setView(v);
                }}
                pool={poolMints.length}
                pass={mints.length}
              />
              {area && (
                <span className="flex items-center gap-2 text-[11px] text-text">
                  <span className="text-text-mid">
                    Listing: <Terms text={areaSentence(area, areas[area].length)} />
                  </span>
                  <Button size="xs" variant="ghost" onClick={() => setArea(null)}>
                    Clear
                  </Button>
                </span>
              )}
            </span>
          }
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
            defaultSort={{ col: 'mkt_chances', dir: 'desc' }}
            resetKey={`${area ?? view}|${listed.length}|${maxPause}|${failEnds}|${idea.map((c) => c.text).join(',')}`}
            highlightWallet={wallet}
            useRowOverlay={useOverlay}
            titleOf={(r) => r.symbol || r.name || shortAddr(r.mint_address)}
            selectedKey={inspected?.mint ?? null}
            onSelect={(mint) => {
              const row = mint ? rows.find((r) => r.mint_address === mint) : null;
              setInspected(mint ? { mint, symbol: row?.symbol } : null);
            }}
            emptyMessage={
              area ? 'No token in this area' : view === 'pool' ? 'No token in the pool' : 'No token in the market passes the filters'
            }
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
