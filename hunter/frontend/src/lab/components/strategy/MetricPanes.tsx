import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';

import { Select } from 'components/ui/Select';
import { Checkbox } from 'components/ui/Checkbox';
import { Button } from 'components/ui/Button';
import { Accordion } from 'components/ui/Accordion';
import { cn } from 'lib/cn';
import { ACCORDION_IDS, STORAGE_KEYS, getJSON, setJSON } from 'lib/storage';
import { metricColorStyle } from 'lib/strategy/metricColors';
import {
  allMetrics,
  familyName,
  findFamily,
  findMetric,
  metricHelp,
  unitSuffix,
  useStrategyRegistry,
  type MetricSpec,
  type MetricUnit,
  type StrategyRegistry,
} from 'lib/strategy/registry';
import { useGetStrategyRulesQuery } from 'store/sharedEndpoints';
import {
  CONDITION_SIDE_TAG,
  DEFAULT_WINDOWS,
  findRuleFireMarkers,
  metricClockHorizons,
  metricConditionBands,
  metricConditionStatesAt,
  metricThresholdsFor,
  nearestSeriesIndex,
  paneRuleFromJson,
  parseSeriesAtSec,
  rulePaneKeys,
  ruleAges,
  ruleWindows,
  seriesByLabel,
  type ConditionSide,
  type MetricConditionLanes,
  type MetricSeriesColumn,
} from 'lib/strategy/metricPanes';
import { useGetMetricSeriesQuery } from '@lab/store/labEndpoints';
import type { ChartEventMarker, ChartVisibleTimeRange } from 'components/token-price-chart';
import type { WindowSpec } from 'lib/strategy/windowSpec';

/** Wall-clock label for the truncation notice's `covered_until`. */
function formatCoveredUntil(at: string | null | undefined): string {
  if (!at) return 'the start of the token';
  const ms = Date.parse(at);
  return Number.isFinite(ms) ? new Date(ms).toLocaleTimeString() : at;
}

/** A read label (`m_flow.buy_sol @!volume [10s]`): family, dot, metric. Saved panes
 *  in any other shape name no read and are dropped on load. */
const READ_LABEL_RE = /^m_[a-z]+\.[a-z0-9_]+/;

interface Prefs {
  /** Read labels. */
  panes: string[];
  /** WHOLE spans, since a bare size cannot tell 30 slots from 30 seconds. */
  windows: WindowSpec[];
  ruleId: string | null;
  /** When true, panes follow the selected rule's reads (cleared on manual toggle). */
  autoPanes: boolean;
  /** Draw the rule's conditions as on/off lanes under the price chart. */
  timeline: boolean;
}

function loadPrefs(): Prefs {
  const stored = getJSON<Partial<Prefs> | null>(STORAGE_KEYS.metricPanes, null);
  // Merge over defaults (never a versioned key) so an added field is just absent.
  const merged = {
    panes: [],
    windows: [...DEFAULT_WINDOWS],
    ruleId: null,
    autoPanes: true,
    timeline: false,
    ...(stored ?? {}),
  };
  merged.panes = (merged.panes as unknown[]).filter(
    (k): k is string => typeof k === 'string' && READ_LABEL_RE.test(k),
  );
  // A bare number is a wall-clock span in seconds.
  merged.windows = (merged.windows as Array<WindowSpec | number>)
    .map((w) => (typeof w === 'number' ? { size: w, lag: 0, unit: 'sec' as const } : w))
    .filter((w) => w != null && Number.isFinite(w.size) && w.size > 0);
  return merged as Prefs;
}

/** Pin the pane overlay to explicit params instead of the saved-rule dropdown: the
 *  exact sweep combo / simulated rule the caller is inspecting, so the `signal`
 *  markers are computed from the params that produced the run. */
export interface MetricPanesRuleOverride {
  /** Raw rule `params` JSON (a rule's `params` or a sweep combo's blob). */
  paramsJson: unknown;
  /** Fingerprint whose tags the tagged reads use (null = none). */
  fingerprintId: string | null;
  /** Shown in place of the rule dropdown (e.g. "combo #37", the rule name). */
  label: string;
}

export interface MetricPanesProps {
  mint: string;
  /** Shared wall-clock crosshair (unix seconds): from price chart or pane hover. */
  crosshairTimeSec?: number | null;
  /** Shared visible window from the price chart (unix seconds). */
  visibleTimeRange?: ChartVisibleTimeRange | null;
  /** Pane hover drives the shared crosshair (and the price chart). */
  onCrosshairTimeChange?: (timeSec: number | null) => void;
  /** Emit the rule's first buy / sell fires as chart markers. */
  onEventMarkersChange?: (markers: ChartEventMarker[]) => void;
  /**
   * Emit the rule's conditions as chart bottom-pane lanes (the "timeline"). Wiring
   * it is what surfaces the toggle: a host with no chart beside the panes has
   * nowhere to draw them, so it stays hidden there rather than inert.
   */
  onConditionBandsChange?: (bands: MetricConditionLanes | null) => void;
  /** The inspected run's exit reason: picks which condition the timeline draws as a
   *  value line. Without it the line falls back to the first exit condition. */
  exitReason?: string | null;
  /** When set, overlay these params (not the dropdown rule's). */
  ruleOverride?: MetricPanesRuleOverride | null;
  /** Inspected run's entry fill. Supplies the `m_position` reads, which anchor on the
   *  entry: without it the endpoint omits them. */
  positionEntry?: { time: string; price: number } | null;
}

export type MetricPanesLayout = 'page' | 'inspect';
export type MetricPanesPartKind = 'selector' | 'values' | 'graphs';

type MetricPanesModel = ReturnType<typeof useMetricPanesModel>;

const MetricPanesContext = createContext<MetricPanesModel | null>(null);

function useMetricPanesCtx(): MetricPanesModel {
  const ctx = useContext(MetricPanesContext);
  if (!ctx) throw new Error('MetricPanesPart must render inside MetricPanesProvider');
  return ctx;
}

/** Everything a pane needs about one read, from the series column and the registry. */
interface PaneMeta {
  key: string;
  path: string;
  family: string;
  unit: MetricUnit;
  spec: MetricSpec | undefined;
  column: MetricSeriesColumn | undefined;
  /** CSS colour from the registry hue. */
  color: string;
  /** Tooltip: the read, then the registry's definition. */
  help: string;
}

function paneMeta(
  key: string,
  column: MetricSeriesColumn | undefined,
  registry: StrategyRegistry | undefined,
): PaneMeta {
  const path = column?.metric ?? key.split(' ')[0];
  const family = column?.family ?? familyName(path);
  const spec = findMetric(registry, path);
  return {
    key,
    path,
    family,
    unit: column?.unit ?? spec?.unit ?? 'count',
    spec,
    column,
    color: metricColorStyle({ hue: spec?.hue, group: family, metric: path }).color,
    help: spec ? `${key}\n${metricHelp(spec)}` : key,
  };
}

/** A pane group: the rule's own reads, or one registry family. */
interface PaneGroup {
  key: string;
  title: string;
  help: string;
  /** The family of a family group (its items drop the `family.` prefix); null for the rule group. */
  family: string | null;
  items: string[];
}

/** A read as shown inside its family group: `buy_sol @!volume [10s]`. */
function shortLabel(key: string, family: string | null): string {
  return family && key.startsWith(`${family}.`) ? key.slice(family.length + 1) : key;
}

function useMetricPanesModel({
  mint,
  crosshairTimeSec = null,
  visibleTimeRange = null,
  onCrosshairTimeChange,
  onEventMarkersChange,
  onConditionBandsChange,
  exitReason = null,
  ruleOverride = null,
  positionEntry = null,
  layout = 'page',
}: MetricPanesProps & { layout?: MetricPanesLayout }) {
  const { data: registry } = useStrategyRegistry();
  const { data: rules = [] } = useGetStrategyRulesQuery();
  const [prefs, setPrefs] = useState<Prefs>(loadPrefs);

  const selectedRule = useMemo(
    () => (ruleOverride ? null : rules.find((r) => r.id === prefs.ruleId) ?? null),
    [ruleOverride, rules, prefs.ruleId],
  );

  // Override params are derived, never written into prefs: closing the inspect leaves
  // the saved dropdown / pane selection untouched.
  const ruleParamsJson = ruleOverride ? ruleOverride.paramsJson : selectedRule?.params ?? null;
  const parsed = useMemo(
    () => (ruleParamsJson != null ? paneRuleFromJson(ruleParamsJson) : null),
    [ruleParamsJson],
  );
  const rule = parsed?.rule ?? null;
  const ruleError = parsed?.error ?? null;
  const ruleKeys = useMemo(() => (rule ? rulePaneKeys(rule) : []), [rule]);

  // A dropdown rule (with autoPanes on) writes its reads into prefs, so a manual
  // toggle later starts from them.
  useEffect(() => {
    if (!selectedRule || !rule || !prefs.autoPanes) return;
    setPrefs((p) => ({ ...p, windows: ruleWindows(rule), panes: ruleKeys.length ? ruleKeys : p.panes }));
  }, [selectedRule, rule, ruleKeys, prefs.autoPanes]);

  // A shown rule always fetches its own spans, or its windowed reads are not columns.
  const windows = useMemo(() => (rule ? ruleWindows(rule) : prefs.windows), [rule, prefs.windows]);
  const ages = useMemo(() => (rule ? ruleAges(rule) : []), [rule]);

  // The backend sizes its sparse tick grid from what we evaluate over the series:
  // `windows` covers the trailing reads; the two clocks have to be declared, or their
  // crossings can fall in a gap the grid skipped.
  const clockHorizons = useMemo(
    () => (rule ? metricClockHorizons(rule, registry) : null),
    [rule, registry],
  );

  const { data, isFetching, error } = useGetMetricSeriesQuery(
    {
      mint,
      windows,
      ages,
      fingerprintId: ruleOverride ? ruleOverride.fingerprintId : selectedRule?.fingerprint_id ?? null,
      entryTime: positionEntry?.time ?? null,
      entryPrice: positionEntry?.price ?? null,
      timeHorizonSec: clockHorizons?.timeHorizonSec ?? null,
      stallHorizonSec: clockHorizons?.stallHorizonSec ?? null,
    },
    { skip: !mint },
  );

  useEffect(() => {
    setJSON(STORAGE_KEYS.metricPanes, prefs);
  }, [prefs]);

  /** Panes actually rendered: the rule's own reads until the user toggles. */
  const panes = rule && prefs.autoPanes && ruleKeys.length ? ruleKeys : prefs.panes;

  const atSec = useMemo(() => parseSeriesAtSec(data?.at ?? []), [data?.at]);
  const seriesByKey = useMemo(() => seriesByLabel(data?.series ?? []), [data]);

  // Push the rule's buy / sell fires up to the price chart.
  useEffect(() => {
    if (!onEventMarkersChange) return;
    onEventMarkersChange(data && rule && registry ? findRuleFireMarkers(rule, data, registry) : []);
  }, [data, rule, registry, onEventMarkersChange]);

  // The lanes fold the series the panes already fetched, so the toggle costs no
  // request, only the fold: that is why it still gates on `prefs.timeline`.
  const conditionBands = useMemo(
    () =>
      prefs.timeline && data && rule && registry
        ? metricConditionBands(rule, data, registry, exitReason)
        : null,
    [prefs.timeline, data, rule, registry, exitReason],
  );

  // Cleared on unmount so a modal that switches to a run with no overlay does not
  // leave the previous one's lanes on the chart.
  useEffect(() => {
    if (!onConditionBandsChange) return;
    onConditionBandsChange(conditionBands);
    return () => onConditionBandsChange(null);
  }, [conditionBands, onConditionBandsChange]);

  const metaOf = useCallback(
    (key: string) => paneMeta(key, seriesByKey.get(key), registry),
    [seriesByKey, registry],
  );

  /** The picker: every read the series carries, family by family, metric by metric,
   *  in registry order. */
  const catalog = useMemo(() => {
    const order = new Map(allMetrics(registry).map((m, i) => [m.path, i]));
    const famOrder = new Map((registry?.families ?? []).map((f, i) => [f.name, i]));
    const byFamily = new Map<string, Map<string, string[]>>();
    for (const s of data?.series ?? []) {
      const fam = byFamily.get(s.family) ?? new Map<string, string[]>();
      fam.set(s.metric, [...(fam.get(s.metric) ?? []), s.label]);
      byFamily.set(s.family, fam);
    }
    const rank = (m: Map<string, number>, k: string) => m.get(k) ?? Number.MAX_SAFE_INTEGER;
    return [...byFamily.entries()]
      .sort(([a], [b]) => rank(famOrder, a) - rank(famOrder, b) || a.localeCompare(b))
      .map(([name, metrics]) => {
        const fam = findFamily(registry, name);
        return {
          name,
          title: fam?.title ?? name,
          summary: fam?.summary ?? '',
          metrics: [...metrics.entries()]
            .sort(([a], [b]) => rank(order, a) - rank(order, b) || a.localeCompare(b))
            .map(([path, reads]) => ({ path, spec: findMetric(registry, path), reads })),
        };
      });
  }, [data, registry]);

  const allKeys = useMemo(() => (data?.series ?? []).map((s) => s.label), [data]);

  /** Selected reads in groups: the rule's own conditions first, then family by
   *  family in registry order. */
  const groupPanes = useCallback(
    (keys: string[]): PaneGroup[] => {
      const ruleSet = new Set(ruleKeys);
      const out: PaneGroup[] = [];
      const own = ruleKeys.filter((k) => keys.includes(k));
      if (own.length) {
        out.push({
          key: 'rule',
          title: 'Rule conditions',
          help: "The reads this rule's conditions make, in rule order",
          family: null,
          items: own,
        });
      }
      const byFamily = new Map<string, string[]>();
      for (const k of keys) {
        if (ruleSet.has(k)) continue;
        const fam = metaOf(k).family;
        byFamily.set(fam, [...(byFamily.get(fam) ?? []), k]);
      }
      const famOrder = new Map((registry?.families ?? []).map((f, i) => [f.name, i]));
      const ranked = [...byFamily.keys()].sort(
        (a, b) => (famOrder.get(a) ?? Infinity) - (famOrder.get(b) ?? Infinity) || a.localeCompare(b),
      );
      for (const fam of ranked) {
        const f = findFamily(registry, fam);
        out.push({ key: fam, title: f?.title ?? fam, help: f?.summary ?? fam, family: fam, items: byFamily.get(fam)! });
      }
      return out;
    },
    [ruleKeys, metaOf, registry],
  );

  const crosshairIdx = useMemo(() => {
    if (crosshairTimeSec == null || !atSec.length) return null;
    return nearestSeriesIndex(atSec, crosshairTimeSec);
  }, [crosshairTimeSec, atSec]);

  // Keyed by READ, never by metric: a rule may read `m_flow.buy_sol` over the life and
  // over 10 s at once, and those are different numbers with different verdicts.
  const conditionByColumn = useMemo(() => {
    const map = new Map<string, { ok: boolean; side: ConditionSide }>();
    if (crosshairIdx == null || !rule || !data || !registry) return map;
    for (const s of metricConditionStatesAt(rule, crosshairIdx, data, registry)) {
      // A failing condition wins over a passing one on the same read.
      const prev = map.get(s.key);
      if (!prev || (prev.ok && !s.ok)) map.set(s.key, { ok: s.ok, side: s.side });
    }
    return map;
  }, [crosshairIdx, rule, data, registry]);

  /** One readable number per selected pane: crosshair when hovering, else latest. */
  const valueStrip = useMemo(
    () =>
      panes.map((key) => {
        const meta = metaOf(key);
        const values = meta.column?.values;
        const idx = values ? (crosshairIdx ?? lastFiniteIdx(values)) : null;
        const raw = values && idx != null ? values[idx] : null;
        const text = raw != null && Number.isFinite(raw) ? `${formatMetric(raw)}${unitSuffix(meta.unit)}` : '—';
        return { key, meta, text, ok: conditionByColumn.get(key)?.ok ?? null };
      }),
    [panes, metaOf, crosshairIdx, conditionByColumn],
  );

  const togglePane = (key: string) =>
    setPrefs((p) => ({
      ...p,
      autoPanes: false,
      // Seed from the rendered set so the first manual toggle under a rule edits the
      // rule's panes instead of resurfacing stale saved ones.
      panes: panes.includes(key) ? panes.filter((k) => k !== key) : [...panes, key],
    }));

  const allSelected = allKeys.length > 0 && allKeys.every((k) => panes.includes(k));

  const toggleSelectAll = () =>
    setPrefs((p) => ({ ...p, autoPanes: false, panes: allSelected ? [] : allKeys }));

  const xDomain: ChartVisibleTimeRange | null = useMemo(() => {
    if (visibleTimeRange && visibleTimeRange.to > visibleTimeRange.from) return visibleTimeRange;
    const finite = atSec.filter((t) => Number.isFinite(t));
    if (finite.length < 2) return null;
    return { from: finite[0], to: finite[finite.length - 1] };
  }, [visibleTimeRange, atSec]);

  /** Map pointer X on a pane to the nearest series timestamp (drives the crosshair). */
  const handlePanePointer = useCallback(
    (clientX: number, svgEl: Element) => {
      if (!onCrosshairTimeChange || !atSec.length) return;
      const xFrom = xDomain?.from ?? atSec.find((t) => Number.isFinite(t)) ?? 0;
      const xTo = xDomain?.to ?? atSec.filter((t) => Number.isFinite(t)).at(-1) ?? 1;
      const xSpan = xTo - xFrom || 1;
      const rect = svgEl.getBoundingClientRect();
      if (rect.width <= 0) return;
      const ratio = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
      const idx = nearestSeriesIndex(atSec, xFrom + ratio * xSpan);
      if (idx == null || !Number.isFinite(atSec[idx])) return;
      onCrosshairTimeChange(atSec[idx]);
    },
    [onCrosshairTimeChange, atSec, xDomain],
  );

  const handlePaneLeave = useCallback(() => {
    onCrosshairTimeChange?.(null);
  }, [onCrosshairTimeChange]);

  return {
    layout,
    registry,
    ruleOverride,
    rules,
    prefs,
    setPrefs,
    panes,
    allKeys,
    catalog,
    metaOf,
    groupPanes,
    valueStrip,
    togglePane,
    toggleSelectAll,
    allSelected,
    error,
    isFetching,
    data,
    crosshairIdx,
    atSec,
    xDomain,
    rule,
    ruleError,
    conditionByColumn,
    crosshairTimeSec,
    handlePanePointer,
    handlePaneLeave,
    /** The host draws lanes: without one the toggle has nowhere to point. */
    timelineSupported: !!onConditionBandsChange,
    timelineOn: prefs.timeline,
    laneCount: conditionBands?.lanes.length ?? 0,
    setTimelineOn: (on: boolean) => setPrefs((p) => ({ ...p, timeline: on })),
  };
}

/** Index of the last finite value, or null. */
function lastFiniteIdx(values: Array<number | null>): number | null {
  for (let i = values.length - 1; i >= 0; i--) {
    const v = values[i];
    if (v != null && Number.isFinite(v)) return i;
  }
  return null;
}

const groupLabelClass =
  'font-mono text-[10px] font-semibold uppercase tracking-wider text-secondary';

function MetricPanesSelector() {
  const {
    layout,
    registry,
    ruleOverride,
    rules,
    prefs,
    panes,
    allKeys,
    catalog,
    togglePane,
    toggleSelectAll,
    allSelected,
    setPrefs,
    ruleError,
  } = useMetricPanesCtx();

  if (!registry) return null;

  const inspect = layout === 'inspect';

  return (
    <div className="flex flex-col gap-2 rounded-md border border-white/8 bg-white/2 p-2">
      <Accordion
        title="Metrics"
        padding="none"
        bordered={false}
        storageKey={inspect ? ACCORDION_IDS.metricSelectorInspect : ACCORDION_IDS.metricSelector}
        // Collapsed by default in both placements: the picker is a tall checklist,
        // and open it pushes the panes it selects off screen.
        defaultOpen={false}
      >
        <div className="flex flex-col gap-2">
          <div className="flex items-center justify-between gap-2 border-b border-white/8 pb-2">
            <span className="text-[11px] text-text-dim">
              {panes.length} / {allKeys.length} selected
            </span>
            <Button variant="subtle" size="xs" onClick={toggleSelectAll}>
              {allSelected ? 'unselect all' : 'select all'}
            </Button>
          </div>
          {catalog.map((fam) => (
            <div
              key={fam.name}
              className={cn(
                'border-b border-white/5 pb-1.5 last:border-b-0 last:pb-0',
                inspect ? 'flex flex-col gap-1' : 'flex items-start gap-x-3',
              )}
            >
              <span className={cn(groupLabelClass, inspect ? 'w-full' : 'w-32 shrink-0 pt-0.5')} title={fam.summary}>
                {fam.title}
              </span>
              <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                {fam.metrics.map((m) => (
                  <div key={m.path} className="flex min-w-0 items-start gap-2 rounded px-1 py-0.5 odd:bg-white/2">
                    <span
                      className="w-32 shrink-0 truncate font-mono text-[11px]"
                      style={{ color: metricColorStyle({ hue: m.spec?.hue, group: fam.name, metric: m.path }).color }}
                      title={m.spec ? `${m.path}\n${metricHelp(m.spec)}` : m.path}
                    >
                      {shortLabel(m.path, fam.name)}
                    </span>
                    <div className="flex min-w-0 flex-1 flex-wrap items-center gap-x-2.5 gap-y-0.5">
                      {m.reads.map((key) => (
                        <label key={key} className="flex shrink-0 items-center gap-1 text-[11px] text-text-dim" title={key}>
                          <Checkbox boxSize="sm" checked={panes.includes(key)} onChange={() => togglePane(key)} />
                          <span className="font-mono whitespace-nowrap">{key.slice(m.path.length).trim() || 'life'}</span>
                        </label>
                      ))}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
        <div className="flex flex-wrap items-center gap-2 border-t border-white/8 pt-2">
          <span className="text-[11px] text-text-dim">rule overlay</span>
          {ruleOverride ? (
            <span
              className="rounded border border-white/10 bg-surface px-2 py-1 font-mono text-[12px] text-secondary"
              title="Thresholds + fire markers use the exact params of the inspected run"
            >
              {ruleOverride.label}
            </span>
          ) : (
            <Select
              fieldSize="sm"
              value={prefs.ruleId ?? ''}
              onChange={(e) => setPrefs((p) => ({ ...p, ruleId: e.target.value || null, autoPanes: true }))}
              className="min-w-40"
            >
              <option value="">none</option>
              {rules.map((r) => (
                <option key={r.id} value={r.id}>
                  {r.rule_name}
                </option>
              ))}
            </Select>
          )}
          {ruleError && <span className="text-[11px] text-warning">{ruleError}</span>}
        </div>
      </Accordion>
    </div>
  );
}

function MetricPanesValues() {
  const { valueStrip, groupPanes, crosshairIdx, rule, timelineSupported, timelineOn, laneCount, setTimelineOn } =
    useMetricPanesCtx();

  const timelineAvailable = timelineSupported && rule != null;
  if (valueStrip.length === 0 && !timelineAvailable) return null;
  const byKey = new Map(valueStrip.map((v) => [v.key, v]));

  return (
    <div className="sticky top-0 z-10 flex flex-col gap-y-1.5 rounded-md border border-white/10 bg-bg-panel/95 px-2.5 py-2 backdrop-blur-sm">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span className="text-[10px] font-semibold uppercase tracking-wider text-text-dim">
          {crosshairIdx != null ? 'at crosshair' : 'latest'}
        </span>
        {timelineAvailable && (
          <button
            type="button"
            onClick={() => setTimelineOn(!timelineOn)}
            className={cn(
              'rounded px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider',
              timelineOn ? 'bg-white/10 text-text' : 'text-text-dim hover:text-text',
            )}
            title={
              timelineOn
                ? `Hide the per-condition timeline under the chart (${laneCount} lanes)`
                : "Draw each of the rule's conditions as a lane under the chart, filled where its reading held. Reads the same series as these panes, so it models no entry lock and no stage."
            }
          >
            timeline
          </button>
        )}
      </div>
      {groupPanes(valueStrip.map((v) => v.key)).map((g) => (
        <div key={g.key} className="flex items-start gap-x-3">
          <span className={cn(groupLabelClass, 'w-32 shrink-0 pt-0.5')} title={g.help}>
            {g.title}
          </span>
          <div className="flex min-w-0 flex-1 flex-wrap items-end gap-x-3 gap-y-1 px-1">
            {g.items.map((key) => {
              const v = byKey.get(key)!;
              return (
                <div key={key} className="min-w-18" title={v.meta.help}>
                  <div className="font-mono text-[10px]" style={{ color: v.meta.color }}>
                    {shortLabel(key, g.family)}
                  </div>
                  <div
                    className={cn(
                      'font-mono text-[15px] font-semibold tabular-nums leading-tight',
                      v.ok === true ? 'text-green' : v.ok === false ? 'text-warning' : 'text-text',
                    )}
                  >
                    {v.text}
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      ))}
    </div>
  );
}

function MetricPanesGraphs() {
  const {
    layout,
    panes,
    metaOf,
    groupPanes,
    atSec,
    xDomain,
    crosshairTimeSec,
    crosshairIdx,
    rule,
    conditionByColumn,
    handlePanePointer,
    handlePaneLeave,
    error,
    isFetching,
    data,
  } = useMetricPanesCtx();

  const compact = layout === 'inspect';

  return (
    <div className="flex min-w-0 flex-col gap-3">
      {error && <p className="text-[12px] text-red">metric series unavailable for this token.</p>}
      {isFetching && <p className="text-[12px] text-text-dim">computing…</p>}
      {data?.truncated && (
        <p className="text-[12px] text-warning">
          Series truncated at the row ceiling: panes and condition markers cover only up
          to {formatCoveredUntil(data.covered_until)}. Anything after that is not drawn.
        </p>
      )}

      {panes.length === 0 ? (
        <p className="text-[12px] text-text-dim/70">
          Pick a metric above, or select a rule to load its conditions.
        </p>
      ) : (
        <div
          className="flex flex-col gap-2"
          onPointerLeave={(e) => {
            if (!e.currentTarget.contains(e.relatedTarget as Node | null)) handlePaneLeave();
          }}
        >
          {groupPanes(panes).map((g) => (
            <div key={g.key} className="flex flex-col gap-1.5">
              <span className={groupLabelClass} title={g.help}>
                {g.title}
              </span>
              {g.items.map((key) => {
                const meta = metaOf(key);
                if (!meta.column) {
                  return (
                    <div
                      key={key}
                      className="rounded border border-white/8 p-2 text-[11px] text-text-dim/60"
                      title={meta.help}
                    >
                      {key}: not in this series
                    </div>
                  );
                }
                return (
                  <MetricPane
                    key={key}
                    label={shortLabel(key, g.family)}
                    help={meta.help}
                    color={meta.color}
                    unit={meta.unit}
                    atSec={atSec}
                    values={meta.column.values}
                    xDomain={xDomain}
                    crosshairTimeSec={crosshairTimeSec}
                    crosshairIdx={crosshairIdx}
                    thresholds={rule ? metricThresholdsFor(rule, key) : []}
                    conditionOk={conditionByColumn.get(key)?.ok ?? null}
                    onPointerTime={handlePanePointer}
                    compact={compact}
                  />
                );
              })}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

/** Mount once per token inspect: shares query/state across split panes. */
export function MetricPanesProvider({
  children,
  layout = 'page',
  ...props
}: MetricPanesProps & { layout?: MetricPanesLayout; children: ReactNode }) {
  const model = useMetricPanesModel({ ...props, layout });
  if (!model.registry) {
    return <p className="text-[12px] text-text-dim">loading registry…</p>;
  }
  return <MetricPanesContext.Provider value={model}>{children}</MetricPanesContext.Provider>;
}

export function MetricPanesPart({ part }: { part: MetricPanesPartKind }) {
  if (part === 'selector') return <MetricPanesSelector />;
  if (part === 'values') return <MetricPanesValues />;
  return <MetricPanesGraphs />;
}

/**
 * Registry-driven metric panes for one token (lab-only: metric-series needs the
 * lake). Default stacked layout; use {@link MetricPanesProvider} +
 * {@link MetricPanesPart} for the inspect modal split (graphs right, values under
 * the chart).
 */
export function MetricPanes(props: MetricPanesProps) {
  return (
    <MetricPanesProvider {...props} layout="page">
      <div className="flex min-w-0 flex-col gap-3">
        <MetricPanesPart part="selector" />
        <MetricPanesPart part="values" />
        <MetricPanesPart part="graphs" />
      </div>
    </MetricPanesProvider>
  );
}

/** Compact metric number for the HUD / pane rail. */
function formatMetric(v: number): string {
  if (!Number.isFinite(v)) return '—';
  const a = Math.abs(v);
  if (a >= 1000) return v.toFixed(0);
  if (a >= 100) return v.toFixed(1);
  if (a >= 1) return v.toFixed(2);
  if (a >= 0.01) return v.toFixed(3);
  return v.toPrecision(2);
}

const PANE_H = 64;

const THRESHOLD_COLOR: Record<ConditionSide, string> = {
  entry: 'var(--color-primary)',
  signal: 'var(--color-secondary)',
  exit: 'var(--color-warning)',
};

type Threshold = { side: ConditionSide; value: number };

/** Rule threshold values labelled on the right edge of a pane's sparkline. Shared by
 *  the compact and full pane layouts so the two never drift. */
function ThresholdLabels({ thresholds, hi, span }: { thresholds: Threshold[]; hi: number; span: number }) {
  return (
    <>
      {thresholds.map((t, i) => (
        <span
          key={`thr-${i}`}
          className="pointer-events-none absolute right-0 font-mono text-[9px] font-semibold tabular-nums"
          style={{
            top: `${((hi - t.value) / span) * 100}%`,
            transform: 'translateY(-50%)',
            color: THRESHOLD_COLOR[t.side],
          }}
          title={`${t.side} threshold`}
        >
          {CONDITION_SIDE_TAG[t.side]} {formatMetric(t.value)}
        </span>
      ))}
    </>
  );
}

/** One pane: value-first rail + wall-clock sparkline with min/max + thresholds. */
function MetricPane({
  label,
  help,
  color,
  unit,
  atSec,
  values,
  xDomain,
  crosshairTimeSec,
  crosshairIdx,
  thresholds,
  conditionOk,
  onPointerTime,
  compact = false,
}: {
  label: string;
  help: string;
  color: string;
  unit: MetricUnit;
  atSec: number[];
  values: Array<number | null>;
  xDomain: ChartVisibleTimeRange | null;
  crosshairTimeSec: number | null;
  crosshairIdx: number | null;
  thresholds: Threshold[];
  conditionOk: boolean | null;
  onPointerTime?: (clientX: number, svgEl: Element) => void;
  compact?: boolean;
}) {
  const xFrom = xDomain?.from ?? atSec.find((t) => Number.isFinite(t)) ?? 0;
  const xTo = xDomain?.to ?? atSec.filter((t) => Number.isFinite(t)).at(-1) ?? 1;
  const xSpan = xTo - xFrom || 1;

  // Scale + readout from the *visible* window so zoomed views stay meaningful.
  const visibleVals: number[] = [];
  values.forEach((v, i) => {
    const t = atSec[i];
    if (v != null && Number.isFinite(v) && Number.isFinite(t) && t >= xFrom && t <= xTo) {
      visibleVals.push(v);
    }
  });
  const thrVals = thresholds.map((t) => t.value);
  const lo = Math.min(...(visibleVals.length ? visibleVals : [0]), ...thrVals);
  const hi = Math.max(...(visibleVals.length ? visibleVals : [1]), ...thrVals);
  const span = hi - lo || 1;
  const W = 800;
  const x = (t: number) => ((t - xFrom) / xSpan) * W;
  const y = (v: number) => PANE_H - ((v - lo) / span) * PANE_H;

  const segments: string[] = [];
  let cur: string[] = [];
  values.forEach((v, i) => {
    const t = atSec[i];
    if (v == null || !Number.isFinite(v) || !Number.isFinite(t) || t < xFrom || t > xTo) {
      if (cur.length) segments.push(cur.join(' '));
      cur = [];
      return;
    }
    cur.push(`${x(t).toFixed(1)},${y(v).toFixed(1)}`);
  });
  if (cur.length) segments.push(cur.join(' '));

  const primaryIdx = crosshairIdx ?? lastFiniteIdx(values);
  const primary = primaryIdx != null ? values[primaryIdx] : null;
  const primaryText =
    primary != null && Number.isFinite(primary) ? `${formatMetric(primary)}${unitSuffix(unit)}` : '—';
  const crossX =
    crosshairTimeSec != null && crosshairTimeSec >= xFrom && crosshairTimeSec <= xTo
      ? x(crosshairTimeSec)
      : null;
  const crossY =
    primary != null && Number.isFinite(primary) && crosshairIdx != null ? y(primary) : null;

  const valueTone =
    conditionOk === true ? 'text-green' : conditionOk === false ? 'text-warning' : 'text-text';

  const sparkline = (
    <div className="relative min-w-0">
      <svg
        viewBox={`0 0 ${W} ${PANE_H}`}
        preserveAspectRatio="none"
        className="h-16 w-full min-w-0 cursor-crosshair touch-none"
        onPointerMove={(e) => onPointerTime?.(e.clientX, e.currentTarget)}
      >
        {thresholds.map((t, i) => (
          <line
            key={i}
            x1={0}
            x2={W}
            y1={y(t.value)}
            y2={y(t.value)}
            stroke={THRESHOLD_COLOR[t.side]}
            strokeWidth={1}
            strokeDasharray="4 3"
            opacity={0.75}
          />
        ))}
        {segments.map((pts, i) => (
          <polyline key={i} points={pts} fill="none" stroke={color} strokeWidth={1.5} />
        ))}
        {crossX != null && (
          <line x1={crossX} x2={crossX} y1={0} y2={PANE_H} stroke="var(--color-text)" strokeWidth={1} opacity={0.65} />
        )}
        {crossY != null && (
          <line
            x1={0}
            x2={W}
            y1={crossY}
            y2={crossY}
            stroke="var(--color-text)"
            strokeWidth={1}
            opacity={0.35}
            strokeDasharray="3 3"
          />
        )}
        {crossX != null && crossY != null && <circle cx={crossX} cy={crossY} r={3.5} fill={color} opacity={0.9} />}
      </svg>
      <ThresholdLabels thresholds={thresholds} hi={hi} span={span} />
    </div>
  );

  if (compact) {
    return (
      <div className="flex min-w-0 flex-col gap-1 rounded-md border border-white/8 bg-white/2 p-2">
        {/* Label left, readout right: the crosshair is shared across every pane, so
            hovering ANY pane (or the price chart) updates all of these at once. */}
        <div className="flex min-w-0 items-baseline justify-between gap-2">
          <span className="truncate font-mono text-[10px]" style={{ color }} title={help}>
            {label}
          </span>
          <span
            className={`shrink-0 font-mono text-[11px] font-semibold tabular-nums ${valueTone}`}
            title={crosshairIdx != null ? 'at crosshair' : 'latest'}
          >
            {primaryText}
          </span>
        </div>
        {sparkline}
        <div className="flex justify-between px-0.5 font-mono text-[10px] tabular-nums text-text-dim">
          <span title="visible max">{formatMetric(hi)}</span>
          <span title="visible min">{formatMetric(lo)}</span>
        </div>
      </div>
    );
  }

  return (
    <div className="grid grid-cols-[7.5rem_minmax(0,1fr)_auto] items-stretch gap-2 rounded-md border border-white/8 bg-white/2 p-2">
      <div className="flex min-w-0 flex-col justify-center gap-0.5">
        <span className="truncate font-mono text-[11px]" style={{ color }} title={help}>
          {label}
        </span>
        <span className={`font-mono text-[18px] font-semibold tabular-nums leading-none ${valueTone}`}>
          {primaryText}
        </span>
        <span className="text-[10px] text-text-dim/70">{crosshairIdx != null ? 'crosshair' : 'latest'}</span>
      </div>
      {sparkline}
      <div className="flex w-12 flex-col justify-between py-0.5 text-right font-mono text-[10px] tabular-nums text-text-dim">
        <span title="visible max">{formatMetric(hi)}</span>
        <span title="visible min">{formatMetric(lo)}</span>
      </div>
    </div>
  );
}
