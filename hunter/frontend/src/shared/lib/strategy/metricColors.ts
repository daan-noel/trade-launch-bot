// Metric UI colors. The registry `hue` is the one color: a family is one band,
// and each metric in it is a few degrees off. `metricTone` / `metricRowStyle`
// paint a row from that hue. The wash stays light; the name stays saturated so it
// still reads on that wash. Sweep chips add a fixed per-operator shade on top.
// Role colors (buy, signal, stage) live in `roleColors.ts`.
//
// Fallback: if a metric has no `hue` yet (or the registry hasn't loaded), hash
// `group.metric` to a stable hue so unknown metrics still get a color.

import type { CSSProperties } from 'react';
import type { Operator } from './registry';

/** Fixed lightness deltas (°HSL L) per operator — same op ⇒ same shade everywhere. */
const OP_LIGHTNESS: Record<Operator, number> = {
  '>': 0,
  '>=': -4,
  '<': 8,
  '<=': 4,
  '=': -10,
  '!=': 12,
};

/** Slight saturation nudge so `=` / `!=` read apart from inequalities. */
const OP_SATURATION: Record<Operator, number> = {
  '>': 0,
  '>=': -2,
  '<': 0,
  '<=': -2,
  '=': 8,
  '!=': -8,
};

export interface MetricColorInput {
  /** Registry hue `[0, 359]`; omit / NaN → hash fallback. */
  hue?: number | null;
  group: string;
  metric: string;
  /** When set, applies the fixed op shade. Side does not affect color. */
  operator?: Operator | string | null;
}

export interface MetricColorStyle {
  /** Ready-to-spread onto a chip / axis row. */
  style: CSSProperties;
  /** Resolved hue after fallback (for tests / debug). */
  hue: number;
  border: string;
  background: string;
  color: string;
}

/**
 * Stable hue `[0, 359]` from any string (FNV-1a). The ONE string→hue hash on the
 * frontend — rule-tag chips reuse it so an unregistered label still gets a
 * consistent color without a second implementation.
 */
export function hashHue(s: string): number {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return (h >>> 0) % 360;
}

/** Stable hue from `group.metric` when the registry has no hue yet. */
export function hashMetricHue(group: string, metric: string): number {
  return hashHue(`${group}.${metric}`);
}

/** `m_flow` → `flow`. */
export function familyShort(family: string): string {
  return family.startsWith('m_') ? family.slice(2) : family;
}

/**
 * The metric's own tone from its registry hue. The wash stays light. The name is
 * saturated and light so it stands out on that wash, and lightness still steps
 * with the hue so two metrics a few degrees apart read apart. No operator shade:
 * the row is the metric, not the comparison.
 */
export function metricTone(hue: number): MetricColorStyle {
  const h = ((Math.round(hue) % 360) + 360) % 360;
  return chipColorsFromHue(h, 38, Math.min(62, 50 + (h % 10)), true);
}

/** Left rail and a light wash for a whole condition row. The name uses `metricTone`. */
export function metricRowStyle(hue: number): CSSProperties {
  const tone = metricTone(hue);
  return {
    borderLeftWidth: 3,
    borderLeftStyle: 'solid',
    borderLeftColor: tone.color,
    backgroundColor: tone.background,
  };
}

function clamp(n: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, n));
}

function resolveHue(input: MetricColorInput): number {
  const h = input.hue;
  if (typeof h === 'number' && Number.isFinite(h)) {
    return ((Math.round(h) % 360) + 360) % 360;
  }
  return hashMetricHue(input.group, input.metric);
}

/**
 * Chip colors from a resolved hue + saturation/lightness. The ONE place the
 * border/background/text formula lives — metric chips and rule-tag chips share
 * it so a chip reads the same wherever it is rendered.
 *
 * `quiet` is the metric wash: a thin fill, and text saturated enough to stand
 * out on it. Tag chips leave it off; they are labels, not a row of metrics.
 */
export function chipColorsFromHue(hue: number, sat: number, light: number, quiet = false): MetricColorStyle {
  const border = `hsla(${hue}, ${sat}%, ${light}%, ${quiet ? 0.45 : 0.5})`;
  const background = `hsla(${hue}, ${sat}%, ${light}%, ${quiet ? 0.1 : 0.12})`;
  const color = quiet
    ? `hsl(${hue}, ${clamp(sat + 36, 68, 82)}%, ${clamp(light + 24, 74, 84)}%)`
    : `hsl(${hue}, ${clamp(sat + 5, 45, 90)}%, ${clamp(light + 18, 62, 82)}%)`;
  return {
    hue,
    border,
    background,
    color,
    style: { borderColor: border, backgroundColor: background, color },
  };
}

/**
 * Border / background / text colors for a metric (+ optional op shade).
 * Entry vs exit does not change the color — only group.metric (+ op).
 */
export function metricColorStyle(input: MetricColorInput): MetricColorStyle {
  const op = (input.operator ?? '>') as Operator;
  return chipColorsFromHue(
    resolveHue(input),
    clamp(40 + (OP_SATURATION[op] ?? 0), 28, 52),
    clamp(52 + (OP_LIGHTNESS[op] ?? 0), 44, 64),
    true,
  );
}
