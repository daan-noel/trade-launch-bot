import { useCallback, useMemo, useState } from 'react';
import {
  TokenPriceChart,
  type ChartBarSelection,
  type ChartEventMarker,
  type ChartMetric,
  type ChartSwingLeg,
} from 'components/token-price-chart';
import { usePriceUnit } from 'context/PriceUnitContext';
import { DataTable } from 'components/table/DataTable';
import type { ColumnDef } from 'components/table/types';
import { Button } from 'components/ui/Button';
import { Input } from 'components/ui/Input';
import { Badge } from 'components/ui/Badge';
import { SectionDivider } from 'components/ui/SectionDivider';
import { InfoTooltip } from 'components/ui/InfoTooltip';
import { cn } from 'lib/cn';
import { getString, setString, STORAGE_KEYS } from 'lib/storage';
import {
  apiErrorMessage,
  useGetTokenDetailQuery,
  useGetTokenTradesQuery,
} from 'store/apiSlice';
import { SWING1_AXES, groupAxesBySubgroup } from '@lab/components/sweep/groupedTypes';
import { PasteParamsSection } from 'components/strategy/PasteParamsSection';
import { swing1BlobToDetectParams, swing1DetectParamsToJson } from 'lib/ruleParams';
import {
  fetchSwing1Detect,
  type Swing1DetectParams,
  type Swing1DetectResponse,
  type Swing1LowVerdict,
} from '@lab/services/swing1Detect';
import type { TradeRecord } from 'types';

const EMPTY_TRADES: TradeRecord[] = [];
const LS_KEY = `${STORAGE_KEYS.sweepConfig}.swing1detect`;

/** Probe-validated firing defaults (matches `swing_probe::probe_rule` — the
 *  config that actually latches on the lake), so a token funnels out of the box.
 *  `null` = inert/off. */
const DEFAULT_PARAMS: Swing1DetectParams = {
  take_profit: 100,
  stop_loss: 50,
  trailing_stop_pct: null,
  time_stop_secs: null,
  stall_secs: 60,
  liquidity_drop_pct: null,
  swing_high_to_low_sol: null,
  swing_high_to_low_pct: 15,
  swing_low_to_high_sol: null,
  swing_low_to_high_pct: 15,
  swing_min_leg_trades: null,
  dust_frac: 0.05,
  kill_depth_min_pct: 0.4,
  kill_max_duration_ms: 10000,
  kill_min_net_flow_per_sec: null,
  vol_depth_max_pct: 0.6,
  vol_min_duration_ms: null,
  vol_min_up_duration_ms: null,
  min_kills_before_volume: 0,
  entry_pullback_pct: 10,
  entry_higher_low_secs: null,
  entry_max_age_secs: null,
  entry_min_liquidity_sol: null,
  entry_max_cohort_held: null,
  exit_next_kill_depth_min_pct: null,
  exit_next_kill_max_duration_ms: 8000,
};

function loadParams(): Swing1DetectParams {
  try {
    const raw = getString(LS_KEY);
    if (!raw) return DEFAULT_PARAMS;
    return { ...DEFAULT_PARAMS, ...(JSON.parse(raw) as Partial<Swing1DetectParams>) };
  } catch {
    return DEFAULT_PARAMS;
  }
}

/** Param keys typed as integers (the rest parse as floats). */
const INT_KEYS = new Set<keyof Swing1DetectParams>([
  'time_stop_secs',
  'stall_secs',
  'swing_min_leg_trades',
  'kill_max_duration_ms',
  'vol_min_duration_ms',
  'vol_min_up_duration_ms',
  'min_kills_before_volume',
  'entry_higher_low_secs',
  'entry_max_age_secs',
  'exit_next_kill_max_duration_ms',
]);

/** A small summary stat chip. */
function Stat({ label, value, tone }: { label: string; value: string; tone?: string }) {
  return (
    <div className="rounded-md border border-white/8 bg-bg-card/40 px-3 py-2">
      <div className="text-[9px] font-bold uppercase tracking-wider text-text-dim/70">{label}</div>
      <div className={cn('mt-0.5 font-mono text-[13px]', tone ?? 'text-text')}>{value}</div>
    </div>
  );
}

export function Swing1DetectPage() {
  const { unit, usdRate } = usePriceUnit();
  const [mintInput, setMintInput] = useState('');
  const [activeMint, setActiveMint] = useState<string | null>(null);
  const [params, setParams] = useState<Swing1DetectParams>(loadParams);
  const [curveOnly, setCurveOnly] = useState(true);
  const [result, setResult] = useState<Swing1DetectResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [chartMetric, setChartMetric] = useState<ChartMetric>('price');
  const [selectedBar, setSelectedBar] = useState<ChartBarSelection | null>(null);
  const [selectedLegKey, setSelectedLegKey] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const { data: detail } = useGetTokenDetailQuery(activeMint ?? '', { skip: !activeMint });
  const {
    data: tradesData,
    isFetching: tradesLoading,
    error: tradesErrorRaw,
  } = useGetTokenTradesQuery(activeMint ?? '', { skip: !activeMint });
  const trades = tradesData ?? EMPTY_TRADES;
  const tradesError = activeMint ? apiErrorMessage(tradesErrorRaw, 'Failed to load trades') : null;

  const toChartValue = useCallback(
    (sol: number) => (unit === 'USD' && usdRate != null ? sol * usdRate : sol),
    [unit, usdRate],
  );

  // Commit a parsed numeric value for a param (the Input handles raw in-progress
  // text + parsing in its `numeric` mode; here we just persist the result).
  const setParam = useCallback((key: keyof Swing1DetectParams, next: number | null) => {
    setParams((prev) => {
      const updated = { ...prev, [key]: next };
      setString(LS_KEY, JSON.stringify(updated));
      return updated;
    });
  }, []);

  // Recognized detect-param keys == the swing1 axis keys (the page's form IS the
  // axis grid). Used to filter a pasted swing1 combo/rule blob to just the knobs
  // this page owns. Static (defaults never gain keys), so compute once.
  const paramKeySet = useMemo(
    () => new Set(Object.keys(DEFAULT_PARAMS)),
    [],
  );

  // Paste a swing1 combo/rule blob (copied via the sweep page's ⎘) into the form.
  // `merge` overwrites only the blob's keys; `replace` resets to defaults first.
  const handlePasteParams = useCallback(
    (blob: Parameters<typeof swing1BlobToDetectParams>[0], mode: 'merge' | 'replace') => {
      const { params: patch, applied, dropped } = swing1BlobToDetectParams(blob, paramKeySet);
      setParams((prev) => {
        const base = mode === 'replace' ? { ...DEFAULT_PARAMS } : { ...prev };
        const updated = { ...base, ...patch } as Swing1DetectParams;
        setString(LS_KEY, JSON.stringify(updated));
        return updated;
      });
      return { applied, skipped: 0, dropped };
    },
    [paramKeySet],
  );

  // Copy the current form params as a swing1 RuleParamsBlob (same shape the
  // sweep row's ⎘ emits), so they can be pasted into the sweep config / rule
  // modal / this page.
  const handleCopyParams = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(
        swing1DetectParamsToJson(params as Record<string, number | null | undefined>),
      );
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* ignore */
    }
  }, [params]);

  const handleRun = useCallback(async () => {
    const mint = mintInput.trim();
    if (!mint) return;
    setActiveMint(mint);
    setLoading(true);
    setError(null);
    setSelectedLegKey(null);
    setSelectedBar(null);
    try {
      const res = await fetchSwing1Detect(mint, params, {
        startMs: null,
        endMs: null,
        curveOnly,
      });
      setResult(res);
    } catch (e) {
      setResult(null);
      setError(e instanceof Error ? e.message : 'swing1 detection failed');
    } finally {
      setLoading(false);
    }
  }, [mintInput, params, curveOnly]);

  // Legs → chart overlay (SwingLeg serde shape == ChartSwingLeg). Drawn one
  // isolated segment per leg (`perLeg`): legs are NOT connected and the idle
  // gaps between them are never drawn. Each segment spans the leg's full
  // `start_at`→`end_at` (`perLegFullSpanEnd`), so the line aligns exactly with
  // the full-span candle highlight.
  const swingOverlay = useMemo(() => {
    if (!result || !result.legs.length) return null;
    const legs = result.legs as unknown as ChartSwingLeg[];
    return {
      legs,
      segmentMode: 'perLeg' as const,
      perLegFullSpanEnd: true,
    };
  }, [result]);

  // Entry/exit pins (arrow + dashed price line) from the resolved fills.
  const eventMarkers = useMemo<ChartEventMarker[] | null>(() => {
    if (!result) return null;
    const markers: ChartEventMarker[] = [];
    if (result.entry) {
      markers.push({
        kind: 'entry',
        time: result.entry.time,
        priceInSol: result.entry.price,
        label: 'Entry',
      });
    }
    if (result.exit) {
      markers.push({
        kind: 'exit',
        time: result.exit.time,
        priceInSol: result.exit.price,
        label: `Exit · ${result.exit.reason}`,
      });
    }
    return markers.length ? markers : null;
  }, [result]);

  const handleLegClick = useCallback((leg: ChartSwingLeg | null) => {
    setSelectedLegKey(leg ? `${leg.type}-${leg.start_at}-${leg.end_at}` : null);
  }, []);

  // Selecting a verdict row highlights that leg on the chart; DataTable already
  // resolves the toggle and hands back the next key (null on deselect). The key
  // shape matches the chart's `${type}-${start}-${end}` (lows are `swing_low`).
  const handleLowSelect = useCallback((key: string | null) => {
    setSelectedLegKey(key);
  }, []);

  const entryGateMissing = result != null && !result.gate_configured;

  // Labelled-row buckets (swing · kill · volume · confirm · ladder · next-kill),
  // shared with the sweep config form so the two screens read identically.
  const paramBuckets = useMemo(() => groupAxesBySubgroup(SWING1_AXES), []);

  return (
    <div>
      <div className="mb-3.5 flex flex-wrap items-center gap-3">
        <h2 className="text-lg font-extrabold text-text">swing1 detect</h2>
        <span className="text-[12px] text-text-dim">
          Per-token kill→volume funnel — identical to the sweep's decision fns
        </span>
      </div>

      <SectionDivider />

      {/* Token + run controls */}
      <div className="mb-3 flex flex-wrap items-end gap-3">
        <label className="flex flex-col gap-1 text-[10px] font-bold uppercase tracking-widest text-text-dim">
          Mint
          <Input
            value={mintInput}
            onChange={(e) => setMintInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') handleRun();
            }}
            placeholder="token mint address"
            className="min-w-[420px] font-mono font-normal normal-case tracking-normal"
          />
        </label>
        <label className="flex items-center gap-2 text-[12px] text-text">
          <input
            type="checkbox"
            checked={curveOnly}
            onChange={(e) => setCurveOnly(e.target.checked)}
          />
          Curve-only (creation → migration)
        </label>
        <Button variant="primary" onClick={handleRun} disabled={loading || !mintInput.trim()}>
          {loading ? 'Detecting…' : 'Detect'}
        </Button>
        <Button
          variant="ghost"
          className="text-[11px] text-text-dim"
          onClick={() => {
            setParams(DEFAULT_PARAMS);
            setString(LS_KEY, JSON.stringify(DEFAULT_PARAMS));
          }}
        >
          Reset params
        </Button>
        <Button
          variant="ghost"
          className={cn('text-[11px]', copied ? 'text-green' : 'text-text-dim')}
          onClick={handleCopyParams}
          title="Copy current params to clipboard"
        >
          {copied ? '✓ Copied' : '⎘ Copy params'}
        </Button>
      </div>

      {/* Params grid — labelled rows per sub-group (swing → kill → volume →
          confirm → ladder → next-kill), matching the sweep config form. */}
      <div className="mb-3 rounded-lg border border-white/8 bg-bg-card/40 p-4">
        <div className="mb-2 text-[10px] font-bold uppercase tracking-wider text-violet-300">
          swing1 params
        </div>
        <div className="flex flex-col gap-3">
          {paramBuckets.map((bucket, i) => (
            <div key={bucket.meta?.key ?? `untagged-${i}`}>
              {bucket.meta && (
                <span className="mb-1.5 flex items-baseline gap-1.5">
                  <span
                    className={cn(
                      'text-[10px] font-bold uppercase tracking-wider',
                      bucket.meta.accent,
                    )}
                  >
                    {bucket.meta.label}
                  </span>
                  <span className="text-[9px] lowercase text-text-dim/50">{bucket.meta.hint}</span>
                </span>
              )}
              <div className="grid grid-cols-2 gap-2 sm:grid-cols-3 lg:grid-cols-4">
                {bucket.axes.map((a) => {
                  const key = a.key as keyof Swing1DetectParams;
                  return (
                    <label key={a.key} className="flex flex-col gap-1">
                      <span className="flex items-center gap-1 text-[9px] font-bold uppercase tracking-wider text-text-dim/80">
                        {a.label}
                        {a.desc && <InfoTooltip title={a.label} body={a.desc} />}
                      </span>
                      <Input
                        numeric
                        integer={INT_KEYS.has(key)}
                        numericValue={params[key]}
                        onNumericChange={(n) => setParam(key, n)}
                        placeholder={a.nullable ? 'off' : '0'}
                        className="font-normal normal-case tracking-normal"
                      />
                    </label>
                  );
                })}
              </div>
            </div>
          ))}
        </div>
        <div className="mt-3">
          <PasteParamsSection strategy="swing_1" live={false} onApply={handlePasteParams} />
        </div>
      </div>

      {error && <p className="mb-2 text-sm text-red">{error}</p>}
      {entryGateMissing && (
        <p className="mb-2 rounded-md border border-amber-400/30 bg-amber-400/10 px-3 py-2 text-[12px] text-amber-200">
          No entry gate configured (needs a pullback % AND a volume bound) — entry can never
          resolve. Legs / verdicts / latch below still show, for diagnosis.
        </p>
      )}

      {/* Funnel summary */}
      {result && (
        <div className="mb-3 grid grid-cols-2 gap-2 sm:grid-cols-3 lg:grid-cols-6">
          <Stat label="Trades" value={String(result.trade_count)} />
          <Stat label="Legs" value={String(result.legs.length)} />
          <Stat label="Swing lows" value={String(result.lows.length)} />
          <Stat label="Kills seen" value={String(result.latch.kills_seen)} />
          <Stat
            label="Latched"
            value={result.latch.volume_phase_latched ? 'YES' : 'no'}
            tone={result.latch.volume_phase_latched ? 'text-green' : 'text-text-dim'}
          />
          <Stat
            label="Entry"
            value={result.entry ? 'FIRED' : 'none'}
            tone={result.entry ? 'text-green' : 'text-text-dim'}
          />
        </div>
      )}

      {result?.entry && (
        <div className="mb-3 flex flex-wrap items-center gap-2 text-[12px]">
          <Badge variant="primary" className="font-mono font-normal">
            entry {result.entry.price.toExponential(3)} @ {result.entry.time.slice(11, 19)}
          </Badge>
          {result.exit ? (
            <Badge variant="primary" className="font-mono font-normal">
              exit · {result.exit.reason} {result.exit.price.toExponential(3)} (held{' '}
              {result.exit.holding_secs}s)
            </Badge>
          ) : (
            <Badge variant="neutral" className="font-mono font-normal">
              still open (no exit fired)
            </Badge>
          )}
        </div>
      )}

      {/* Chart with leg overlay + entry/exit pins */}
      {activeMint && (
        <div className="mb-4">
          <TokenPriceChart
            symbol={detail?.symbol || detail?.name || activeMint}
            id={activeMint}
            trades={trades}
            loading={tradesLoading}
            error={tradesError}
            toValue={toChartValue}
            priceLabel={unit}
            priceUnit={unit}
            metric={chartMetric}
            onMetricChange={setChartMetric}
            onBarClick={setSelectedBar}
            selectedBar={selectedBar}
            swingOverlay={swingOverlay}
            selectedSwingLegKey={selectedLegKey}
            onSwingLegClick={handleLegClick}
            eventMarkers={eventMarkers}
            athPriceInSol={detail?.ath_price ?? null}
            isMigrated={detail?.is_migrated}
            tokenCreatedAt={detail?.created_at}
          />
        </div>
      )}

      {/* Per-low verdict table */}
      {result && result.lows.length > 0 && (
        <LowVerdictTable
          lows={result.lows}
          latch={result.latch}
          selectedLegKey={selectedLegKey}
          onSelect={handleLowSelect}
        />
      )}
      {result && result.lows.length === 0 && (
        <p className="text-[12px] text-text-dim">
          No swing-low legs detected — token too short / thresholds too tight to classify.
        </p>
      )}
    </div>
  );
}

/** The funnel table: one row per swing-low, kill/volume/higher-low verdicts
 *  color-coded. The latched leg's whole row stands out (violet tint + ring +
 *  ◆ marker on the leg #) rather than getting its own column. */
function LowVerdictTable({
  lows,
  latch,
  selectedLegKey,
  onSelect,
}: {
  lows: Swing1LowVerdict[];
  latch: Swing1DetectResponse['latch'];
  selectedLegKey: string | null;
  onSelect: (key: string | null) => void;
}) {
  // Chart-selection key: the `${type}-${start}-${end}` shape the chart overlay
  // matches against (`selectedSwingLegKey` / `swingLegKey`). NOT unique — two
  // 0-duration lows can share a start/end second — so it must NOT be the React
  // row key (that collision corrupts row order on re-detect). See `rowKey`.
  const legKey = useCallback(
    (l: Swing1LowVerdict) => `swing_low-${l.start_at_ms}-${l.end_at_ms}`,
    [],
  );
  // React/selection row key: `leg_index` is unique per leg, so this never
  // collides even when two lows pivot in the same second.
  const rowKey = useCallback((l: Swing1LowVerdict) => `low-${l.leg_index}`, []);
  // The selected low (resolved from the chart-shape `selectedLegKey`), so we can
  // drive DataTable's selection in its own unique row-key space.
  const selectedRowKey = useMemo(() => {
    if (!selectedLegKey) return null;
    const hit = lows.find((l) => legKey(l) === selectedLegKey);
    return hit ? rowKey(hit) : null;
  }, [selectedLegKey, lows, legKey, rowKey]);
  // DataTable hands back a row key (or null); translate to the chart-shape key.
  const handleRowSelect = useCallback(
    (key: string | null) => {
      if (!key) return onSelect(null);
      const hit = lows.find((l) => rowKey(l) === key);
      onSelect(hit ? legKey(hit) : null);
    },
    [lows, rowKey, legKey, onSelect],
  );
  const isLatched = useCallback(
    (l: Swing1LowVerdict) => latch.latched_leg_index === l.leg_index,
    [latch.latched_leg_index],
  );

  const columns = useMemo<ColumnDef<Swing1LowVerdict>[]>(
    () => [
      {
        key: 'leg_index',
        label: 'Leg #',
        render: (l) => (
          <span className="flex items-center justify-center gap-1">
            {isLatched(l) && (
              <span title="Latched leg" className="text-violet-300">
                ◆
              </span>
            )}
            {l.leg_index}
          </span>
        ),
        sortValue: (l) => l.leg_index,
        searchValue: (l) => String(l.leg_index),
        filterNumber: (l) => l.leg_index,
      },
      {
        key: 'depth_pct',
        label: 'Depth',
        render: (l) => `${(l.depth_pct * 100).toFixed(1)}%`,
        sortValue: (l) => l.depth_pct,
        searchValue: () => '',
        filterNumber: (l) => l.depth_pct * 100,
      },
      {
        key: 'duration_ms',
        label: 'Dur (ms)',
        render: (l) => l.duration_ms,
        sortValue: (l) => l.duration_ms,
        searchValue: () => '',
        filterNumber: (l) => l.duration_ms,
      },
      {
        key: 'net_flow_per_sec',
        label: 'Flow/s',
        render: (l) => l.net_flow_per_sec.toFixed(2),
        sortValue: (l) => l.net_flow_per_sec,
        searchValue: () => '',
        filterNumber: (l) => l.net_flow_per_sec,
      },
      {
        key: 'trade_count',
        label: 'Trades',
        render: (l) => l.trade_count,
        sortValue: (l) => l.trade_count,
        searchValue: () => '',
        filterNumber: (l) => l.trade_count,
      },
      {
        key: 'pivot_price',
        label: 'Pivot',
        render: (l) => l.pivot_price.toExponential(2),
        sortValue: (l) => l.pivot_price,
        searchValue: () => '',
        filterNumber: (l) => l.pivot_price,
      },
      {
        key: 'is_kill',
        label: 'Kill?',
        render: (l) => (
          <span className={l.is_kill ? 'text-red' : 'text-text-dim/40'}>
            {l.is_kill ? 'KILL' : '—'}
          </span>
        ),
        sortValue: (l) => (l.is_kill ? 1 : 0),
        searchValue: (l) => (l.is_kill ? 'kill' : ''),
      },
      {
        key: 'is_volume',
        label: 'Vol?',
        render: (l) => (
          <span className={l.is_volume ? 'text-green' : 'text-text-dim/40'}>
            {l.is_volume ? 'VOL' : '—'}
          </span>
        ),
        sortValue: (l) => (l.is_volume ? 1 : 0),
        searchValue: (l) => (l.is_volume ? 'vol' : ''),
      },
      {
        key: 'higher_low_ok',
        label: 'Higher-low?',
        render: (l) => (
          <span className={l.higher_low_ok ? 'text-text-mid' : 'text-red'}>
            {l.higher_low_ok ? 'yes' : 'NO'}
          </span>
        ),
        sortValue: (l) => (l.higher_low_ok ? 1 : 0),
        searchValue: (l) => (l.higher_low_ok ? 'yes' : 'no'),
      },
    ],
    [isLatched],
  );

  // Per-row tint: kill / volume verdict colors, with the latched leg overriding
  // everything via a violet tint + inset ring so it reads as the funnel pivot.
  // The selected row wins over ALL of these — `rowClassName` is applied after
  // DataTable's own selection class, so the verdict tints would otherwise wash
  // it out. A bright primary fill + bold inset ring keeps it unmistakable on top
  // of any green/red/violet background.
  const rowClassName = useCallback(
    (l: Swing1LowVerdict) => {
      if (selectedLegKey === legKey(l)) {
        return 'bg-primary/30 shadow-[inset_0_0_0_2px_var(--color-primary)] hover:bg-primary/30';
      }
      return cn(
        l.is_kill && 'bg-red/8',
        l.is_volume && !l.is_kill && 'bg-green/8',
        isLatched(l) &&
          'bg-violet-400/15 shadow-[inset_0_0_0_1px_rgba(167,139,250,0.5)] hover:bg-violet-400/20',
      );
    },
    [selectedLegKey, legKey, isLatched],
  );

  return (
    <DataTable
      columns={columns}
      rows={lows}
      rowKey={rowKey}
      // Show lows in chronological order (the backend may emit them unsorted).
      defaultSort={{ col: 'leg_index', dir: 'asc' }}
      defaultPageSize={5}
      pageSizeOptions={[5,10,20,50]}
      selectedKey={selectedRowKey}
      onSelect={handleRowSelect}
      rowClassName={rowClassName}
      searchable={false}
      emptyMessage="No swing-low legs detected."
    />
  );
}
