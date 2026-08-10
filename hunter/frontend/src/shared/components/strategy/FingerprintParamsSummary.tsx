// Compact at-a-glance chip cluster for a fingerprint's match axes. Shared by
// Rules, Simulate, and the rule-editor picker — one SSOT so every surface that
// shows a fingerprint reads the same. Null / empty axes are omitted; bucket is
// always shown (every fingerprint has a width).

import { useState, type CSSProperties, type MouseEvent, type ReactNode } from 'react';
import { formatCompact, formatDecimalTrim } from 'utils/format';
import {
  configuredIxLabels,
  formatIxLabelsText,
  ixLabelsActions,
  ixLabelsCountTail,
} from 'lib/ixLabels';
import { cn } from 'lib/cn';
import { hashHue, metricColorStyle } from 'lib/strategy/metricColors';
import { volumeIxPatternsFromConfig } from 'lib/strategy/registry';
import { useGetFingerprintsQuery } from 'store/sharedEndpoints';
import { formatBucketWidth, lamportsToSol, type Fingerprint } from 'lib/strategy/types';

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
export function chip(
  text: ReactNode,
  opts?: {
    style?: CSSProperties;
    title?: string;
    onClick?: (e: MouseEvent) => void;
  },
): ReactNode {
  return (
    <span
      title={opts?.title}
      onClick={opts?.onClick}
      className={cn(
        'inline-block rounded border border-white/10 bg-surface px-1.5 py-0.5 font-mono text-[11px] leading-tight',
        opts?.onClick && 'cursor-pointer',
      )}
      style={opts?.style}
    >
      {text}
    </span>
  );
}

/**
 * The `Nix` chip — amber body (the ix family tone), plus a hashed color ribbon
 * on the leading edge.
 *
 * The count alone is NOT the axis: `[Create_v2, Create, Buy]` and
 * `[Create_v2, Create, BuyExactSolIn]` are different match criteria that both
 * render `3ix`, so two fingerprints looked identical wherever this chip appears.
 * The ribbon hue is `hashHue` over the joined sequence — the same FNV the metric
 * / rule-tag chips use, so no second hash — which makes any difference visible
 * without widening the chip or adding an opaque token to read. It is a *hint*:
 * two sequences can land on neighboring hues, so identity stays in the tooltip
 * (the JSON array, which is also what a click copies) and, on text-only
 * surfaces, in `ixLabelsCountTail`.
 */
export function IxLabelsChip({ labels }: { labels: string[] }) {
  const [copied, setCopied] = useState(false);
  const hue = hashHue(labels.join('|'));
  const json = formatIxLabelsText(labels);

  const copy = async (e: MouseEvent) => {
    e.stopPropagation();
    try {
      await navigator.clipboard.writeText(json);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* ignore */
    }
  };

  return (
    <>
      {chip(`${labels.length}ix`, {
        title: copied ? 'Copied!' : json,
        onClick: copy,
        style: {
          ...axisTint('ix'),
          boxShadow: `inset 3px 0 0 hsl(${hue}, 72%, 58%)`,
          paddingLeft: '0.5rem',
          // Visible ack — the title only re-reads on the next hover.
          ...(copied ? { filter: 'brightness(1.45)' } : undefined),
        },
      })}
    </>
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
  const ix = configuredIxLabels(fp.ix_labels);
  const chips: ReactNode[] = [
    intChip('cu_limit', fp.cu_limit),
    intChip('cu_price', fp.cu_price),
    solChip('init', fp.init_buy_lamports),
    solChip('max', fp.max_cost_lamports),
    solChip('spend', fp.spendable_lamports_in),
    solChip('fs_buy', fp.first_slot_buy_lamports),
    solChip('fs_sell', fp.first_slot_sell_lamports),
    ix ? <IxLabelsChip key="ix" labels={ix} /> : null,
    // `exact` carries no unit — appending ◎ would read as a zero-width bucket.
    chip(
      `bkt=${formatBucketWidth(fp.bucket_size_amount, 4)}${fp.bucket_size_amount == null ? '' : '◎'}`,
      { style: axisTint('bkt') },
    ),
  ].filter(Boolean);

  return <div className="flex flex-wrap items-center gap-1 text-left">{chips}</div>;
}

/** Axis chips for a fingerprint id — RTK-cached. Render as a sibling below
 *  `FingerprintPicker` when the parent needs full-width chips (e.g. the rule
 *  editor's fingerprint + TP/SL grid). The picker itself is controls-only. */
export function FingerprintParamsById({ id }: { id: string | null }) {
  const { data: fps = [] } = useGetFingerprintsQuery();
  const fp = fps.find((f) => f.id === id);
  if (!fp) return null;
  return fingerprintParamsCell(fp);
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
  const ix = configuredIxLabels(fp.ix_labels);
  if (ix) {
    // Count keeps `3ix` matchable; the tail + actions make the *sequence*
    // filterable, so two same-length sets don't both answer to one query.
    parts.push(`${ix.length}ix`);
    parts.push(ixLabelsCountTail(ix));
    parts.push(ixLabelsActions(ix));
    parts.push(formatIxLabelsText(ix));
  }
  const flowCount = volumeIxPatternsFromConfig(fp.metric_config).length;
  // Match the `flowStatusBadge` pill text (`flow N` / `flow✗`) so filtering by
  // what's actually shown works; the `flow=N` form stays matchable too.
  parts.push(flowCount > 0 ? `flow ${flowCount} flow=${flowCount}` : 'flow✗');
  parts.push(`bkt=${formatBucketWidth(fp.bucket_size_amount, 4)}`);
  return parts.join(' ');
}

/**
 * Stable identity string for a whole fingerprint — every match axis in a fixed
 * order, so two byte-identical fingerprints produce the same key (and sort
 * adjacent) while any difference splits them. Built from raw values (not the
 * display-formatted text) for precision, joined by a delimiter no name holds.
 * SSOT for the "sort by the fingerprint itself" column key. `undefined` fp falls
 * back to its id so unresolved rows still order deterministically.
 */
export function fingerprintIdentityKey(fp: Fingerprint | undefined, fallbackId?: string): string {
  if (!fp) return `~${fallbackId ?? ''}`;
  const flowCount = volumeIxPatternsFromConfig(fp.metric_config).length;
  return [
    fp.name || fp.id,
    fp.cu_limit ?? '',
    fp.cu_price ?? '',
    fp.init_buy_lamports ?? '',
    fp.max_cost_lamports ?? '',
    fp.spendable_lamports_in ?? '',
    fp.first_slot_buy_lamports ?? '',
    fp.first_slot_sell_lamports ?? '',
    formatBucketWidth(fp.bucket_size_amount, 4),
    (configuredIxLabels(fp.ix_labels) ?? []).join(','),
    flowCount,
  ].join('\u0001');
}
