// Metric UI colors — consumes the registry `hue` (backend SSOT) and applies a
// fixed per-operator shade. Used by the sweep axis builder and rule-params
// chips so the same metric always looks the same across surfaces.
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
 */
export function chipColorsFromHue(hue: number, sat: number, light: number): MetricColorStyle {
  const border = `hsla(${hue}, ${sat}%, ${light}%, 0.5)`;
  const background = `hsla(${hue}, ${sat}%, ${light}%, 0.12)`;
  const color = `hsl(${hue}, ${clamp(sat + 5, 45, 90)}%, ${clamp(light + 18, 62, 82)}%)`;
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
    clamp(68 + (OP_SATURATION[op] ?? 0), 40, 85),
    clamp(58 + (OP_LIGHTNESS[op] ?? 0), 42, 72),
  );
}
