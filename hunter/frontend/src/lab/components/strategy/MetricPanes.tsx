import { useCallback, useEffect, useMemo, useState } from 'react';

import { Select } from 'components/ui/Select';
import { Checkbox } from 'components/ui/Checkbox';
import { Accordion } from 'components/ui/Accordion';
import { useStrategyRegistry, unitSuffix, type MetricUnit } from 'lib/strategy/registry';
import { useGetStrategyRulesQuery } from 'store/sharedEndpoints';
import { ruleParamsFromJson, type RuleParams } from 'lib/strategy/ruleParams';
import {
  DEFAULT_WINDOWS,
  extractRuleMetricPrefs,
  findRuleFireMarkers,
  metricColKey,
  metricConditionStatesAt,
  metricPrefsFromParams,
  nearestSeriesIndex,
  parseSeriesAtSec,
} from 'lib/strategy/metricPanes';
import { useGetMetricSeriesQuery } from '@lab/store/labEndpoints';
import type { ChartEventMarker, ChartVisibleTimeRange } from 'components/token-price-chart';
import type { MetricSeriesColumn } from 'lib/strategy/types';

const PREF_KEY = 'mt:metric-panes';

interface Prefs {
  panes: string[]; // column keys
  windows: number[];
  ruleId: string | null;
  /** When true, panes follow the selected rule's metrics (cleared on manual toggle). */
  autoPanes: boolean;
}

function loadPrefs(): Prefs {
  try {
    const raw = localStorage.getItem(PREF_KEY);
    if (raw) {
      return {
        windows: DEFAULT_WINDOWS,
        panes: [],
        ruleId: null,
        autoPanes: true,
        ...JSON.parse(raw),
      };
    }
  } catch {
    /* ignore */
  }
  return { panes: [], windows: [...DEFAULT_WINDOWS], ruleId: null, autoPanes: true };
}

/** Pin the pane overlay to explicit params instead of the saved-rule dropdown —
 *  e.g. the exact sweep combo / simulated rule the caller is inspecting, so the
 *  `signal` markers (`{metric}{op}`) are computed from the same params that
 *  produced the run. */
export interface MetricPanesRuleOverride {
  /** Raw `RuleParams` JSON (a rule's `params` or a sweep combo's blob). */
  paramsJson: unknown;
  /** Fingerprint to key the metric-series fetch by (null = none). */
  fingerprintId: string | null;
  /** Shown in place of the rule dropdown (e.g. "combo #37", the rule name). */
  label: string;
}

export interface MetricPanesProps {
  mint: string;
  /** Shared wall-clock crosshair (unix seconds) — from price chart or pane hover. */
  crosshairTimeSec?: number | null;
  /** Shared visible window from the price chart (unix seconds). */
  visibleTimeRange?: ChartVisibleTimeRange | null;
  /** Pane hover drives the shared crosshair (and the price chart). */
  onCrosshairTimeChange?: (timeSec: number | null) => void;
  /** Emit first metric entry/exit fires as chart markers. */
  onEventMarkersChange?: (markers: ChartEventMarker[]) => void;
  /** When set, overlay these params (not the dropdown rule's). */
  ruleOverride?: MetricPanesRuleOverride | null;
  /** Inspected run's entry fill. Supplies the position-scoped `m_position`
   *  (retrace/pnl/held) columns — those anchor on the entry, so without it they
   *  can't be computed and the whole group is hidden. */
  positionEntry?: { time: string; price: number } | null;
}

/**
 * Registry-driven metric panes for one token (lab-only — metric-series needs the
 * lake). Plots selected metrics on a shared wall-clock axis with the price chart
 * (crosshair + visible range), overlays a rule's thresholds, and reports first
 * entry/exit metric fires as `role: 'signal'` chart markers (`{metric}{op}`).
 */
export function MetricPanes({
  mint,
  crosshairTimeSec = null,
  visibleTimeRange = null,
  onCrosshairTimeChange,
  onEventMarkersChange,
  ruleOverride = null,
  positionEntry = null,
}: MetricPanesProps) {
  const { data: registry } = useStrategyRegistry();
  const { data: rules = [] } = useGetStrategyRulesQuery();
  const [prefs, setPrefs] = useState<Prefs>(loadPrefs);

  const selectedRule = useMemo(
    () => (ruleOverride ? null : rules.find((r) => r.id === prefs.ruleId) ?? null),
    [ruleOverride, rules, prefs.ruleId],
  );

  // Override params are derived, never written into prefs — closing the inspect
  // leaves the user's saved dropdown/pane selection untouched.
  const overrideParams: RuleParams | null = useMemo(
    () => (ruleOverride && registry ? ruleParamsFromJson(ruleOverride.paramsJson, registry) : null),
    [ruleOverride, registry],
  );
  const overridePrefs = useMemo(
    () => (overrideParams ? metricPrefsFromParams(overrideParams, registry) : null),
    [overrideParams, registry],
  );

  // When a rule is selected (and autoPanes is on), adopt its metrics + windows.
  useEffect(() => {
    if (!selectedRule || !registry || !prefs.autoPanes) return;
    const rp = extractRuleMetricPrefs(selectedRule, registry);
    setPrefs((p) => ({
      ...p,
      windows: rp.windows,
      panes: rp.paneKeys.length ? rp.paneKeys : p.panes,
    }));
  }, [selectedRule, registry, prefs.autoPanes]);

  const windows = overridePrefs
    ? overridePrefs.windows
    : prefs.autoPanes && selectedRule && registry
      ? extractRuleMetricPrefs(selectedRule, registry).windows
      : prefs.windows;

  const { data, isFetching, error } = useGetMetricSeriesQuery(
    {
      mint,
      windows,
      fingerprintId: ruleOverride ? ruleOverride.fingerprintId : selectedRule?.fingerprint_id ?? null,
      entryTime: positionEntry?.time ?? null,
      entryPrice: positionEntry?.price ?? null,
    },
    { skip: !mint },
  );

  useEffect(() => {
    try {
      localStorage.setItem(PREF_KEY, JSON.stringify(prefs));
    } catch {
      /* ignore */
    }
  }, [prefs]);

  const ruleParams: RuleParams | null = useMemo(() => {
    if (overrideParams) return overrideParams;
    return selectedRule && registry ? ruleParamsFromJson(selectedRule.params, registry) : null;
  }, [overrideParams, selectedRule, registry]);

  /** Panes actually rendered: the override's own metrics until the user toggles. */
  const panes =
    overridePrefs && prefs.autoPanes && overridePrefs.paneKeys.length
      ? overridePrefs.paneKeys
      : prefs.panes;

  const atSec = useMemo(() => parseSeriesAtSec(data?.at ?? []), [data?.at]);

  // Push metric entry/exit markers up to the price chart.
  useEffect(() => {
    if (!onEventMarkersChange) return;
    if (!data || !ruleParams || !registry) {
      onEventMarkersChange([]);
      return;
    }
    onEventMarkersChange(findRuleFireMarkers(ruleParams, data, registry));
  }, [data, ruleParams, registry, onEventMarkersChange]);

  const allColumns = useMemo(() => {
    const cols: Array<{
      key: string;
      metric: string;
      unit: MetricUnit;
      window: number | null;
      group: string;
    }> = [];
    for (const g of registry?.groups ?? []) {
      // Position-scoped metrics need the inspected run's entry fill; with no entry
      // context the backend can't compute them, so hide the group entirely rather
      // than surface an all-empty pane set.
      if (g.scope === 'position' && !positionEntry) continue;
      for (const m of g.metrics) {
        if (g.kind === 'dynamic') {
          for (const w of windows) {
            cols.push({ key: metricColKey(m.name, w), metric: m.name, unit: m.unit, window: w, group: g.name });
          }
        } else {
          cols.push({ key: metricColKey(m.name, null), metric: m.name, unit: m.unit, window: null, group: g.name });
        }
      }
    }
    return cols;
  }, [registry, windows, positionEntry]);

  /** Registry group order + name for a column key — drives the grouped layout. */
  const groupOf = useMemo(() => {
    const map = new Map<string, string>();
    for (const c of allColumns) map.set(c.key, c.group);
    return map;
  }, [allColumns]);

  const groupOrder = useMemo(() => (registry?.groups ?? []).map((g) => g.name), [registry]);

  /** All selectable columns bucketed by group, in registry order (for the selector). */
  const columnsByGroup = useMemo(() => {
    const buckets = new Map<string, typeof allColumns>();
    for (const c of allColumns) {
      const arr = buckets.get(c.group) ?? [];
      arr.push(c);
      buckets.set(c.group, arr);
    }
    return groupOrder.filter((g) => buckets.has(g)).map((g) => ({ group: g, cols: buckets.get(g)! }));
  }, [allColumns, groupOrder]);

  /** Split a flat list of selected pane keys into registry-ordered groups. */
  const groupKeys = useCallback(
    <T,>(items: T[], keyOf: (item: T) => string): Array<{ group: string; items: T[] }> => {
      const buckets = new Map<string, T[]>();
      for (const it of items) {
        const g = groupOf.get(keyOf(it)) ?? 'other';
        const arr = buckets.get(g) ?? [];
        arr.push(it);
        buckets.set(g, arr);
      }
      const ordered = [...groupOrder, 'other'].filter((g) => buckets.has(g));
      return ordered.map((g) => ({ group: g, items: buckets.get(g)! }));
    },
    [groupOf, groupOrder],
  );

  const seriesByKey = useMemo(() => {
    const map = new Map<string, MetricSeriesColumn>();
    for (const s of data?.series ?? []) map.set(metricColKey(s.metric, s.window_size_sec), s);
    return map;
  }, [data]);

  const crosshairIdx = useMemo(() => {
    if (crosshairTimeSec == null || !atSec.length) return null;
    return nearestSeriesIndex(atSec, crosshairTimeSec);
  }, [crosshairTimeSec, atSec]);

  const conditionStates = useMemo(() => {
    if (crosshairIdx == null || !ruleParams || !data || !registry) return [];
    return metricConditionStatesAt(ruleParams, crosshairIdx, data, registry);
  }, [crosshairIdx, ruleParams, data, registry]);

  const conditionByMetric = useMemo(() => {
    const map = new Map<string, { ok: boolean; side: 'entry' | 'exit' }>();
    for (const s of conditionStates) {
      // Prefer a failing side if both exist; otherwise last write wins.
      const prev = map.get(s.metric);
      if (!prev || (prev.ok && !s.ok)) map.set(s.metric, { ok: s.ok, side: s.side });
    }
    return map;
  }, [conditionStates]);

  /** One readable number per selected pane — crosshair when hovering, else latest. */
  const valueStrip = useMemo(() => {
    return panes.map((key) => {
      const meta = allColumns.find((c) => c.key === key);
      const col = seriesByKey.get(key);
      if (!meta || !col) return { key, label: key, text: '—', ok: null as boolean | null };
      const idx =
        crosshairIdx != null
          ? crosshairIdx
          : col.values.reduceRight<number | null>(
            (found, v, i) => (found != null ? found : v != null && Number.isFinite(v) ? i : null),
            null,
          );
      const raw = idx != null ? col.values[idx] : null;
      const suffix = unitSuffix(meta.unit);
      const text = raw != null && Number.isFinite(raw) ? `${formatMetric(raw)}${suffix}` : '—';
      const cond = conditionByMetric.get(meta.metric);
      return {
        key,
        label: key,
        text,
        ok: cond ? cond.ok : null,
      };
    });
  }, [panes, allColumns, seriesByKey, crosshairIdx, conditionByMetric]);

  const togglePane = (key: string) =>
    setPrefs((p) => ({
      ...p,
      autoPanes: false,
      // Seed from the rendered set so the first manual toggle under an override
      // edits the override's panes instead of resurfacing stale saved ones.
      panes: panes.includes(key) ? panes.filter((k) => k !== key) : [...panes, key],
    }));

  const xDomain: ChartVisibleTimeRange | null = useMemo(() => {
    if (visibleTimeRange && visibleTimeRange.to > visibleTimeRange.from) return visibleTimeRange;
    const finite = atSec.filter((t) => Number.isFinite(t));
    if (finite.length < 2) return null;
    return { from: finite[0], to: finite[finite.length - 1] };
  }, [visibleTimeRange, atSec]);

  /** Map pointer X on a pane → nearest series timestamp (drives shared crosshair). */
  const handlePanePointer = useCallback(
    (clientX: number, svgEl: Element) => {
      if (!onCrosshairTimeChange || !atSec.length) return;
      const xFrom = xDomain?.from ?? atSec.find((t) => Number.isFinite(t)) ?? 0;
      const xTo = xDomain?.to ?? atSec.filter((t) => Number.isFinite(t)).at(-1) ?? 1;
      const xSpan = xTo - xFrom || 1;
      const rect = svgEl.getBoundingClientRect();
      if (rect.width <= 0) return;
      const ratio = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
      const t = xFrom + ratio * xSpan;
      const idx = nearestSeriesIndex(atSec, t);
      if (idx == null || !Number.isFinite(atSec[idx])) return;
      onCrosshairTimeChange(atSec[idx]);
    },
    [onCrosshairTimeChange, atSec, xDomain],
  );

  const handlePaneLeave = useCallback(() => {
    onCrosshairTimeChange?.(null);
  }, [onCrosshairTimeChange]);

  if (!registry) return <p className="text-[12px] text-text-dim">loading registry…</p>;

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-col gap-2 rounded-md border border-white/8 bg-white/2 p-2">
        <Accordion
          title="Metrics"
          padding="none"
          bordered={false}
          storageKey="mt:metric-selector-open"
        >
          <div className="flex flex-col gap-2">
            {columnsByGroup.map(({ group, cols }) => (
              <div key={group} className="flex flex-wrap items-center gap-x-4 gap-y-1.5">
                <span className="w-24 shrink-0 font-mono text-[10px] font-semibold uppercase tracking-wider text-secondary">
                  {group}
                </span>
                {cols.map((c) => (
                  <label key={c.key} className="flex items-center gap-1.5 text-[12px] text-text-dim">
                    <Checkbox checked={panes.includes(c.key)} onChange={() => togglePane(c.key)} />
                    <span className="font-mono">
                      {c.metric}
                      {c.window != null && <span className="text-text-dim/60">@{c.window}s</span>}
                    </span>
                  </label>
                ))}
              </div>
            ))}
          </div>
          <div className="flex items-center gap-2 border-t border-white/8 pt-2">
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
                onChange={(e) =>
                  setPrefs((p) => ({
                    ...p,
                    ruleId: e.target.value || null,
                    autoPanes: true,
                  }))
                }
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
          </div>
        </Accordion>
      </div>

      {/* Sticky value strip — scan every selected metric without reading each sparkline. */}
      {valueStrip.length > 0 && (
        <div className="sticky top-0 z-10 flex flex-col gap-y-1.5 rounded-md border border-white/10 bg-bg-panel/95 px-2.5 py-2 backdrop-blur-sm">
          <span className="text-[10px] font-semibold uppercase tracking-wider text-text-dim">
            {crosshairIdx != null ? 'at crosshair' : 'latest'}
          </span>
          {groupKeys(valueStrip, (v) => v.key).map(({ group, items }) => (
            <div key={group} className="flex flex-wrap items-end gap-x-3 gap-y-2">
              <span className="w-24 shrink-0 self-center font-mono text-[10px] font-semibold uppercase tracking-wider text-secondary">
                {group}
              </span>
              {items.map((v) => (
                <div key={v.key} className="min-w-[4.5rem]">
                  <div className="font-mono text-[10px] text-text-dim">{v.label}</div>
                  <div
                    className={`font-mono text-[15px] font-semibold tabular-nums leading-tight ${v.ok === true
                        ? 'text-green'
                        : v.ok === false
                          ? 'text-warning'
                          : 'text-text'
                      }`}
                  >
                    {v.text}
                  </div>
                </div>
              ))}
            </div>
          ))}
        </div>
      )}

      {error && <p className="text-[12px] text-red">metric series unavailable for this token.</p>}
      {isFetching && <p className="text-[12px] text-text-dim">computing…</p>}

      {panes.length === 0 ? (
        <p className="text-[12px] text-text-dim/70">
          Pick a metric above, or select a rule to auto-load its conditions.
        </p>
      ) : (
        <div
          className="flex flex-col gap-2"
          onPointerLeave={(e) => {
            // Only clear when leaving the whole pane stack (not when moving pane→pane).
            if (!e.currentTarget.contains(e.relatedTarget as Node | null)) {
              handlePaneLeave();
            }
          }}
        >
          {groupKeys(panes, (key) => key).map(({ group, items }) => (
            <div key={group} className="flex flex-col gap-2">
              <span className="font-mono text-[10px] font-semibold uppercase tracking-wider text-secondary">
                {group}
              </span>
              {items.map((key) => {
                const col = seriesByKey.get(key);
                const meta = allColumns.find((c) => c.key === key);
                if (!col || !meta) {
                  return (
                    <div key={key} className="rounded border border-white/8 p-2 text-[11px] text-text-dim/60">
                      {key} — no data
                    </div>
                  );
                }
                return (
                  <MetricPane
                    key={key}
                    label={key}
                    unit={meta.unit}
                    atSec={atSec}
                    values={col.values}
                    xDomain={xDomain}
                    crosshairTimeSec={crosshairTimeSec}
                    crosshairIdx={crosshairIdx}
                    thresholds={ruleParams ? metricThresholds(ruleParams, meta.metric) : []}
                    conditionOk={conditionByMetric.get(meta.metric)?.ok ?? null}
                    onPointerTime={handlePanePointer}
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

/** Entry/exit threshold values a rule places on one metric (any group). */
function metricThresholds(
  params: RuleParams,
  metric: string,
): Array<{ side: 'entry' | 'exit'; value: number }> {
  const out: Array<{ side: 'entry' | 'exit'; value: number }> = [];
  for (const side of ['entry', 'exit'] as const) {
    const sc = params[side];
    if (!sc) continue;
    for (const g of Object.values(sc)) {
      // `metrics[metric]` is DNF (`Condition[][]`) — flatten arms → atoms.
      const arms = g.metrics[metric];
      if (!arms) continue;
      for (const arm of arms) {
        for (const c of arm) {
          if (Number.isFinite(c.value)) out.push({ side, value: c.value });
        }
      }
    }
  }
  return out;
}

const PANE_H = 64;

/** One metric pane: value-first rail + wall-clock sparkline with min/max + thresholds. */
function MetricPane({
  label,
  unit,
  atSec,
  values,
  xDomain,
  crosshairTimeSec,
  crosshairIdx,
  thresholds,
  conditionOk,
  onPointerTime,
}: {
  label: string;
  unit: MetricUnit;
  atSec: number[];
  values: Array<number | null>;
  xDomain: ChartVisibleTimeRange | null;
  crosshairTimeSec: number | null;
  crosshairIdx: number | null;
  thresholds: Array<{ side: 'entry' | 'exit'; value: number }>;
  conditionOk: boolean | null;
  onPointerTime?: (clientX: number, svgEl: Element) => void;
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
    if (v == null || !Number.isFinite(v) || !Number.isFinite(t)) {
      if (cur.length) segments.push(cur.join(' '));
      cur = [];
      return;
    }
    if (t < xFrom || t > xTo) {
      if (cur.length) segments.push(cur.join(' '));
      cur = [];
      return;
    }
    cur.push(`${x(t).toFixed(1)},${y(v).toFixed(1)}`);
  });
  if (cur.length) segments.push(cur.join(' '));

  const suffix = unitSuffix(unit);
  const primaryIdx =
    crosshairIdx != null
      ? crosshairIdx
      : values.reduceRight<number | null>(
        (found, v, i) => (found != null ? found : v != null && Number.isFinite(v) ? i : null),
        null,
      );
  const primary = primaryIdx != null ? values[primaryIdx] : null;
  const primaryText =
    primary != null && Number.isFinite(primary) ? `${formatMetric(primary)}${suffix}` : '—';
  const crossX =
    crosshairTimeSec != null && crosshairTimeSec >= xFrom && crosshairTimeSec <= xTo
      ? x(crosshairTimeSec)
      : null;
  const crossY =
    primary != null && Number.isFinite(primary) && crosshairIdx != null ? y(primary) : null;

  const valueTone =
    conditionOk === true ? 'text-green' : conditionOk === false ? 'text-warning' : 'text-text';

  return (
    <div className="grid grid-cols-[7.5rem_minmax(0,1fr)_auto] items-stretch gap-2 rounded-md border border-white/8 bg-white/2 p-2">
      <div className="flex min-w-0 flex-col justify-center gap-0.5">
        <span className="truncate font-mono text-[11px] text-text-dim" title={label}>
          {label}
        </span>
        <span className={`font-mono text-[18px] font-semibold tabular-nums leading-none ${valueTone}`}>
          {primaryText}
        </span>
        <span className="text-[10px] text-text-dim/70">
          {crosshairIdx != null ? 'crosshair' : 'latest'}
        </span>
      </div>

      <div className="relative min-w-0">
        <svg
          viewBox={`0 0 ${W} ${PANE_H}`}
          preserveAspectRatio="none"
          className="h-16 w-full cursor-crosshair touch-none"
          onPointerMove={(e) => onPointerTime?.(e.clientX, e.currentTarget)}
        >
          {thresholds.map((t, i) => (
            <g key={i}>
              <line
                x1={0}
                x2={W}
                y1={y(t.value)}
                y2={y(t.value)}
                stroke={t.side === 'entry' ? 'var(--color-primary)' : 'var(--color-warning)'}
                strokeWidth={1}
                strokeDasharray="4 3"
                opacity={0.75}
              />
            </g>
          ))}
          {segments.map((pts, i) => (
            <polyline key={i} points={pts} fill="none" stroke="var(--color-green)" strokeWidth={1.5} />
          ))}
          {crossX != null && (
            <line
              x1={crossX}
              x2={crossX}
              y1={0}
              y2={PANE_H}
              stroke="var(--color-text)"
              strokeWidth={1}
              opacity={0.65}
            />
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
          {crossX != null && crossY != null && (
            <circle cx={crossX} cy={crossY} r={3.5} fill="var(--color-green)" opacity={0.9} />
          )}
        </svg>
        {/* Threshold labels overlaid on the right of the sparkline */}
        {thresholds.map((t, i) => (
          <span
            key={`thr-${i}`}
            className="pointer-events-none absolute right-0 font-mono text-[9px] tabular-nums"
            style={{
              top: `${((hi - t.value) / span) * 100}%`,
              transform: 'translateY(-50%)',
              color: t.side === 'entry' ? 'var(--color-primary)' : 'var(--color-warning)',
            }}
          >
            {t.side[0].toUpperCase()} {formatMetric(t.value)}
          </span>
        ))}
      </div>

      <div className="flex w-12 flex-col justify-between py-0.5 text-right font-mono text-[10px] tabular-nums text-text-dim">
        <span title="visible max">{formatMetric(hi)}</span>
        <span title="visible min">{formatMetric(lo)}</span>
      </div>
    </div>
  );
}
