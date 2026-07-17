import { useEffect, useMemo, useState } from 'react';

import { Select } from 'components/ui/Select';
import { Checkbox } from 'components/ui/Checkbox';
import { useStrategyRegistry, unitSuffix, type MetricUnit } from 'lib/strategy/registry';
import { useGetStrategyRulesQuery } from 'store/sharedEndpoints';
import { ruleParamsFromJson, type RuleParams } from 'lib/strategy/ruleParams';
import { useGetMetricSeriesQuery } from '@lab/store/labEndpoints';
import type { MetricSeriesColumn } from 'lib/strategy/types';

const DEFAULT_WINDOWS = [10, 30, 60];
const PREF_KEY = 'mt:metric-panes';

interface Prefs {
  panes: string[]; // column keys
  windows: number[];
  ruleId: string | null;
}

/** Stable key for a series column (metric, optionally @window). */
function colKey(metric: string, window: number | null): string {
  return window == null ? metric : `${metric}@${window}`;
}

function loadPrefs(): Prefs {
  try {
    const raw = localStorage.getItem(PREF_KEY);
    if (raw) return { windows: DEFAULT_WINDOWS, panes: [], ruleId: null, ...JSON.parse(raw) };
  } catch {
    /* ignore */
  }
  return { panes: [], windows: DEFAULT_WINDOWS, ruleId: null };
}

/**
 * Registry-driven metric panes for one token (FE4, lab-only — the metric-series
 * endpoint needs the lake). Fetches every metric's value over the token's life and
 * renders the selected metrics as stacked line panes, with a selected rule's
 * condition thresholds overlaid. Pane / window / rule prefs persist to localStorage.
 *
 * NOTE: this is a self-contained pane layer (SVG sparklines). Placing the panes
 * under the live price chart with a shared crosshair / time-scale (§3.3) is a
 * deferred follow-up — it means threading into the 1.9k-line `TokenPriceChart`.
 */
export function MetricPanes({ mint }: { mint: string }) {
  const { data: registry } = useStrategyRegistry();
  const { data: rules = [] } = useGetStrategyRulesQuery();
  const [prefs, setPrefs] = useState<Prefs>(loadPrefs);
  const { data, isFetching, error } = useGetMetricSeriesQuery(
    { mint, windows: prefs.windows },
    { skip: !mint },
  );

  useEffect(() => {
    try {
      localStorage.setItem(PREF_KEY, JSON.stringify(prefs));
    } catch {
      /* ignore */
    }
  }, [prefs]);

  // The rule whose thresholds overlay the panes (parsed once).
  const ruleParams: RuleParams | null = useMemo(() => {
    const rule = rules.find((r) => r.id === prefs.ruleId);
    return rule && registry ? ruleParamsFromJson(rule.params, registry) : null;
  }, [rules, prefs.ruleId, registry]);

  // All selectable columns from the registry × requested windows.
  const allColumns = useMemo(() => {
    const cols: Array<{ key: string; metric: string; unit: MetricUnit; window: number | null }> = [];
    for (const g of registry?.groups ?? []) {
      for (const m of g.metrics) {
        if (g.kind === 'dynamic') {
          for (const w of prefs.windows) {
            cols.push({ key: colKey(m.name, w), metric: m.name, unit: m.unit, window: w });
          }
        } else {
          cols.push({ key: colKey(m.name, null), metric: m.name, unit: m.unit, window: null });
        }
      }
    }
    return cols;
  }, [registry, prefs.windows]);

  const seriesByKey = useMemo(() => {
    const map = new Map<string, MetricSeriesColumn>();
    for (const s of data?.series ?? []) map.set(colKey(s.metric, s.window_size_sec), s);
    return map;
  }, [data]);

  const togglePane = (key: string) =>
    setPrefs((p) => ({
      ...p,
      panes: p.panes.includes(key) ? p.panes.filter((k) => k !== key) : [...p.panes, key],
    }));

  if (!registry) return <p className="text-[12px] text-text-dim">loading registry…</p>;

  return (
    <div className="flex flex-col gap-3">
      {/* Pane picker + rule selector */}
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2 rounded-md border border-white/8 bg-white/2 p-2">
        <span className="text-[11px] font-semibold uppercase text-text-dim">panes</span>
        {allColumns.map((c) => (
          <label key={c.key} className="flex items-center gap-1.5 text-[12px] text-text-dim">
            <Checkbox checked={prefs.panes.includes(c.key)} onChange={() => togglePane(c.key)} />
            <span className="font-mono">
              {c.metric}
              {c.window != null && <span className="text-text-dim/60">@{c.window}s</span>}
            </span>
          </label>
        ))}
        <div className="ml-auto flex items-center gap-2">
          <span className="text-[11px] text-text-dim">rule overlay</span>
          <Select
            fieldSize="sm"
            value={prefs.ruleId ?? ''}
            onChange={(e) => setPrefs((p) => ({ ...p, ruleId: e.target.value || null }))}
            className="min-w-40"
          >
            <option value="">none</option>
            {rules.map((r) => (
              <option key={r.id} value={r.id}>
                {r.rule_name}
              </option>
            ))}
          </Select>
        </div>
      </div>

      {error && <p className="text-[12px] text-red">metric series unavailable for this token.</p>}
      {isFetching && <p className="text-[12px] text-text-dim">computing…</p>}

      {prefs.panes.length === 0 ? (
        <p className="text-[12px] text-text-dim/70">Pick a metric above to add a pane.</p>
      ) : (
        <div className="flex flex-col gap-2">
          {prefs.panes.map((key) => {
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
                values={col.values}
                thresholds={ruleParams ? metricThresholds(ruleParams, meta.metric) : []}
              />
            );
          })}
        </div>
      )}
    </div>
  );
}

/** Entry/exit threshold values a rule places on one metric (any group). */
function metricThresholds(params: RuleParams, metric: string): Array<{ side: 'entry' | 'exit'; value: number }> {
  const out: Array<{ side: 'entry' | 'exit'; value: number }> = [];
  for (const side of ['entry', 'exit'] as const) {
    const sc = params[side];
    if (!sc) continue;
    for (const g of Object.values(sc)) {
      const conds = g.metrics[metric];
      if (conds) for (const c of conds) out.push({ side, value: c.value });
    }
  }
  return out;
}

const PANE_H = 56;

/** One metric pane: an SVG line of the values with the rule's thresholds drawn as
 *  horizontal reference lines (entry = teal, exit = amber). */
function MetricPane({
  label,
  unit,
  values,
  thresholds,
}: {
  label: string;
  unit: MetricUnit;
  values: Array<number | null>;
  thresholds: Array<{ side: 'entry' | 'exit'; value: number }>;
}) {
  const finite = values.filter((v): v is number => v != null && Number.isFinite(v));
  const thrVals = thresholds.map((t) => t.value);
  const lo = Math.min(...(finite.length ? finite : [0]), ...thrVals);
  const hi = Math.max(...(finite.length ? finite : [1]), ...thrVals);
  const span = hi - lo || 1;
  const n = values.length;
  const W = 800;
  const x = (i: number) => (n <= 1 ? 0 : (i / (n - 1)) * W);
  const y = (v: number) => PANE_H - ((v - lo) / span) * PANE_H;

  // Build the polyline, breaking at nulls (null → no point).
  const segments: string[] = [];
  let cur: string[] = [];
  values.forEach((v, i) => {
    if (v == null || !Number.isFinite(v)) {
      if (cur.length) segments.push(cur.join(' '));
      cur = [];
    } else {
      cur.push(`${x(i).toFixed(1)},${y(v).toFixed(1)}`);
    }
  });
  if (cur.length) segments.push(cur.join(' '));

  const suffix = unitSuffix(unit);

  return (
    <div className="rounded-md border border-white/8 bg-white/2 p-2">
      <div className="mb-1 flex items-center justify-between">
        <span className="font-mono text-[11px] text-text">{label}</span>
        <span className="text-[10px] text-text-dim/70">
          {finite.length ? `${finite[finite.length - 1].toFixed(2)}${suffix}` : '—'}
        </span>
      </div>
      <svg viewBox={`0 0 ${W} ${PANE_H}`} preserveAspectRatio="none" className="h-14 w-full">
        {thresholds.map((t, i) => (
          <line
            key={i}
            x1={0}
            x2={W}
            y1={y(t.value)}
            y2={y(t.value)}
            stroke={t.side === 'entry' ? 'var(--color-primary)' : 'var(--color-warning)'}
            strokeWidth={1}
            strokeDasharray="4 3"
            opacity={0.7}
          />
        ))}
        {segments.map((pts, i) => (
          <polyline key={i} points={pts} fill="none" stroke="var(--color-green)" strokeWidth={1.5} />
        ))}
      </svg>
    </div>
  );
}
