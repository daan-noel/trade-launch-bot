// Compact at-a-glance chip cluster for a fingerprint's match axes. Shared by
// Rules, Simulate, and the rule-editor picker — one SSOT so every surface that
// shows a fingerprint reads the same. Null / empty axes are omitted; bucket is
// always shown (every fingerprint has a width).

import type { CSSProperties, ReactNode } from 'react';
import { formatCompact, formatDecimalTrim } from 'utils/format';
import { formatIxLabelsText } from 'lib/ixLabels';
import { metricColorStyle } from 'lib/strategy/metricColors';
import { volumeIxPatternsFromConfig } from 'lib/strategy/registry';
import { lamportsToSol, type Fingerprint } from 'lib/strategy/types';

/** Stable per-axis hue so each fingerprint param reads with its own color,
 *  mirroring the metric-condition chips. Related axes share a hue family (small
 *  offset within, wide gap between) so paired params read together at a glance.
 *  Unlisted axes fall back to the hashed hue in `metricColorStyle`, so a new
 *  axis still gets a color for free. */
const AXIS_HUE: Record<string, number> = {
  // compute budget — blue family
  cu_limit: 205,
  cu_price: 219,
  // buy amounts — green family
  init: 142,
  max: 150,
  spend: 158,
  // first-slot volume — violet family
  fs_buy: 268,
  fs_sell: 284,
  // instructions — amber family (labels + count share the tone)
  ix: 45,
  // flow-split volume ix patterns — cyan
  flow: 180,
  // bucket width — rose
  bkt: 340,
};

/** Muted chip style for an absent/unconfigured status flag (no tint, dimmed). */
const OFF_TINT: CSSProperties = { opacity: 0.5 };

/** Registry-consistent tint for a fingerprint axis (same engine as metrics).
 *  Exported so other surfaces showing the same underlying axes (e.g. a
 *  discovery group-key header) reuse this exact hue table instead of
 *  inventing a second one. */
export function axisTint(label: string): CSSProperties {
  return metricColorStyle({ hue: AXIS_HUE[label], group: 'fingerprint', metric: label }).style;
}

/** Exported so other surfaces (e.g. a discovery/sweep group-key header) can
 *  render their own key=value chips in the same visual language as the axis
 *  chips below, instead of inventing a second chip style. */
export function chip(text: ReactNode, opts?: { style?: CSSProperties; title?: string }): ReactNode {
  return (
    <span
      title={opts?.title}
      className="inline-block rounded border border-white/10 bg-surface px-1.5 py-0.5 font-mono text-[11px] leading-tight"
      style={opts?.style}
    >
      {text}
    </span>
  );
}

function solChip(label: string, lamports: number | null): ReactNode | null {
  const s = lamportsToSol(lamports);
  if (s == null) return null;
  return chip(`${label}=${formatDecimalTrim(s, 4)}◎`, { style: axisTint(label) });
}

function intChip(label: string, n: number | null): ReactNode | null {
  if (n == null) return null;
  return chip(`${label}=${formatCompact(n, 1)}`, { style: axisTint(label) });
}

/** Distinct at-a-glance flow-split status pill, meant to sit next to the
 *  fingerprint NAME (not among the mono axis chips) so presence/absence of
 *  volume ix patterns is noticeable separately. `flow N` (cyan) when configured,
 *  a dimmed `flow✗` when not. */
export function flowStatusBadge(fp: Fingerprint): ReactNode {
  const n = volumeIxPatternsFromConfig(fp.metric_config).length;
  const on = n > 0;
  return (
    <span
      title={on ? `${n} volume ix pattern${n === 1 ? '' : 's'} configured` : 'no volume ix patterns'}
      className="inline-flex items-center rounded-full px-1.5 py-0.5 text-[10px] font-semibold uppercase leading-none tracking-wide"
      style={on ? axisTint('flow') : OFF_TINT}
    >
      {on ? `flow ${n}` : 'flow✗'}
    </span>
  );
}

/** Axis chips only (no name) — set criteria + always-on bucket width. */
export function fingerprintParamsCell(fp: Fingerprint): ReactNode {
  const ix = fp.ix_labels?.length ? fp.ix_labels : null;
  const chips: ReactNode[] = [
    intChip('cu_limit', fp.cu_limit),
    intChip('cu_price', fp.cu_price),
    solChip('init', fp.init_buy_lamports),
    solChip('max', fp.max_cost_lamports),
    solChip('spend', fp.spendable_lamports_in),
    solChip('fs_buy', fp.first_slot_buy_lamports),
    solChip('fs_sell', fp.first_slot_sell_lamports),
    ix
      ? chip(`${ix.length}ix`, { title: formatIxLabelsText(ix), style: axisTint('ix') })
      : null,
    chip(`bkt=${formatDecimalTrim(fp.bucket_size_amount, 4)}◎`, { style: axisTint('bkt') }),
  ].filter(Boolean);

  return <div className="flex flex-wrap items-center gap-1 text-left">{chips}</div>;
}

/** Flat searchable text for table filters (axis labels + values + name). */
export function fingerprintParamsSearchText(fp: Fingerprint | undefined, fallbackId?: string): string {
  if (!fp) return fallbackId ?? '';
  const parts: string[] = [fp.name || fp.id.slice(0, 8)];
  if (fp.cu_limit != null) parts.push(`cu_limit=${fp.cu_limit}`);
  if (fp.cu_price != null) parts.push(`cu_price=${fp.cu_price}`);
  const pushSol = (label: string, lamports: number | null) => {
    const s = lamportsToSol(lamports);
    if (s != null) parts.push(`${label}=${formatDecimalTrim(s, 4)}`);
  };
  pushSol('init', fp.init_buy_lamports);
  pushSol('max', fp.max_cost_lamports);
  pushSol('spend', fp.spendable_lamports_in);
  pushSol('fs_buy', fp.first_slot_buy_lamports);
  pushSol('fs_sell', fp.first_slot_sell_lamports);
  if (fp.ix_labels?.length) {
    parts.push(`${fp.ix_labels.length}ix`);
    parts.push(formatIxLabelsText(fp.ix_labels));
  }
  const flowCount = volumeIxPatternsFromConfig(fp.metric_config).length;
  parts.push(flowCount > 0 ? `flow=${flowCount}` : 'flow✗');
  parts.push(`bkt=${formatDecimalTrim(fp.bucket_size_amount, 4)}`);
  return parts.join(' ');
}
