// Compact at-a-glance chip cluster for a fingerprint's match axes. Shared by
// Rules, Simulate, and the rule-editor picker — one SSOT so every surface that
// shows a fingerprint reads the same. Null / empty axes are omitted, and so is the
// bucket width on a row with no SOL axis to spend it on (it reaches no match).

import { useState, type CSSProperties, type MouseEvent, type ReactNode } from 'react';
import { formatCompact, formatDecimalTrim } from 'utils/format';
import {
  configuredIxLabels,
  formatIxLabelsText,
  ixLabelsActions,
  ixLabelsCountTail,
} from 'lib/ixLabels';
import {
  formatVolumePatternsText,
  volumePatternsActions,
  volumePatternsIdentity,
} from 'lib/flow/volumePatterns';
import { cn } from 'lib/cn';
import { hashHue, metricColorStyle } from 'lib/strategy/metricColors';
import { volumeIxPatternsFromConfig } from 'lib/strategy/registry';
import { useGetFingerprintsQuery } from 'store/sharedEndpoints';
import {
  fingerprintAutoName,
} from 'lib/strategy/fingerprintNameFromGroupKey';
import {
  formatBucketWidth,
  hasSolAxis,
  lamportsToSol,
  type Fingerprint,
} from 'lib/strategy/types';

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
  // wildcard — its own hue: it is not an axis, it replaces all of them
  wildcard: 20,
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
 * A count chip whose count is NOT its axis — family tint for the body, plus a
 * `hashHue` ribbon on the leading edge over the *contents*, and a click that
 * copies the re-pastable JSON.
 *
 * Both label axes need exactly this: `[Create_v2, Create, Buy]` and
 * `[Create_v2, Create, BuyExactSolIn]` both render `3ix`; a one-pattern
 * `[Create, Buy]` and a one-pattern `[CreateIdempotent, Buy, Transfer]` both
 * render `flow 1`. Either way two fingerprints that arm on different tokens look
 * identical wherever the chip appears. The ribbon reuses the FNV the metric /
 * rule-tag chips use (no second hash) and makes the difference visible without
 * widening the chip. It is a *hint*: two contents can land on neighboring hues,
 * so identity stays in the tooltip and, on text-only surfaces, in
 * `ixLabelsCountTail` / `volumePatternsActions`.
 */
function ContentsChip({
  text,
  identity,
  title,
  copyText,
  tint,
}: {
  text: string;
  /** Hashed for the ribbon hue — the contents, not the count. */
  identity: string;
  /** Hover text; a click copies `copyText`. */
  title: string;
  copyText: string;
  tint: CSSProperties;
}) {
  const [copied, setCopied] = useState(false);
  const hue = hashHue(identity);

  const copy = async (e: MouseEvent) => {
    e.stopPropagation();
    try {
      await navigator.clipboard.writeText(copyText);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* ignore */
    }
  };

  return (
    <>
      {chip(text, {
        title: copied ? 'Copied!' : title,
        onClick: copy,
        style: {
          ...tint,
          boxShadow: `inset 3px 0 0 hsl(${hue}, 72%, 58%)`,
          paddingLeft: '0.5rem',
          // Visible ack — the title only re-reads on the next hover.
          ...(copied ? { filter: 'brightness(1.45)' } : undefined),
        },
      })}
    </>
  );
}

/** The `Nix` chip — amber body (the ix family tone). See {@link ContentsChip}. */
export function IxLabelsChip({ labels }: { labels: string[] }) {
  const json = formatIxLabelsText(labels);
  return (
    <ContentsChip
      text={`${labels.length}ix`}
      identity={labels.join('|')}
      title={json}
      copyText={json}
      tint={axisTint('ix')}
    />
  );
}

/**
 * The `flow N` chip — cyan body (the flow-split family tone). See
 * {@link ContentsChip}; the tooltip lists each pattern as its action sequence
 * and a click copies the whole set as JSON.
 *
 * Unlike every other axis, the unconfigured state stays VISIBLE as a dimmed
 * `flow✗`: an empty set is not a dropped criterion, it is the verdict "every
 * trade on this fingerprint classifies organic", which silently changes what
 * `nonvol_*` reads. A missing chip among nine reads as "didn't look".
 */
export function FlowPatternsChip({ patterns }: { patterns: string[][] }) {
  const n = patterns.length;
  if (n === 0) {
    return <>{chip('flow✗', { title: 'no volume ix patterns', style: OFF_TINT })}</>;
  }
  return (
    <ContentsChip
      text={`flow ${n}`}
      identity={volumePatternsIdentity(patterns)}
      title={`${n} volume ix pattern${n === 1 ? '' : 's'}\n${volumePatternsActions(patterns)}`}
      copyText={formatVolumePatternsText(patterns)}
      tint={axisTint('flow')}
    />
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

/** Axis chips only (no name) — set criteria, plus the always-on bucket width and
 *  flow-pattern axes. */
export function fingerprintParamsCell(fp: Fingerprint): ReactNode {
  const ix = configuredIxLabels(fp.ix_labels);
  // A wildcard carries no axis and no usable bucket width, so the chip row is the
  // one chip that says what it matches. Rendering the axis chips too (they would
  // all be absent) would read as "unconfigured" — the opposite of what it does.
  if (fp.wildcard) {
    return (
      <div className="flex flex-wrap items-center gap-1 text-left">
        {chip('ALL tokens', {
          style: axisTint('wildcard'),
          title: 'Wildcard — matches every token, ignoring every creation-shape axis',
        })}
        <FlowPatternsChip patterns={volumeIxPatternsFromConfig(fp.metric_config)} />
      </div>
    );
  }
  const chips: ReactNode[] = [
    intChip('cu_limit', fp.cu_limit),
    intChip('cu_price', fp.cu_price),
    solChip('init', fp.init_buy_lamports),
    solChip('max', fp.max_cost_lamports),
    solChip('spend', fp.spendable_lamports_in),
    solChip('fs_buy', fp.first_slot_buy_lamports),
    solChip('fs_sell', fp.first_slot_sell_lamports),
    ix ? <IxLabelsChip key="ix" labels={ix} /> : null,
    <FlowPatternsChip key="flow" patterns={volumeIxPatternsFromConfig(fp.metric_config)} />,
    // The width is an axis only where a SOL axis spends it. With none configured it
    // reaches no match, so showing it invents a criterion the engine does not apply
    // — and made two rows that match identically read as different fingerprints.
    // `exact` carries no unit — appending ◎ would read as a zero-width bucket.
    hasSolAxis(fp)
      ? chip(
          `bkt=${formatBucketWidth(fp.bucket_size_amount)}${fp.bucket_size_amount == null ? '' : '◎'}`,
          { style: axisTint('bkt') },
        )
      : null,
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
  // Matches the chip text, so filtering by what is actually shown works.
  if (fp.wildcard) parts.push('ALL tokens wildcard');
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
  const patterns = volumeIxPatternsFromConfig(fp.metric_config);
  // Match the `FlowPatternsChip` text (`flow N` / `flow✗`) so filtering by what's
  // actually shown works; the `flow=N` form stays matchable too. The action
  // sequences go in for the same reason `ixLabelsActions` does — searching
  // `Transfer` must find the fingerprints that classify it as volume, which a
  // count alone can never answer.
  if (patterns.length > 0) {
    parts.push(`flow ${patterns.length} flow=${patterns.length}`);
    parts.push(volumePatternsActions(patterns));
    parts.push(formatVolumePatternsText(patterns));
  } else {
    parts.push('flow✗');
  }
  if (hasSolAxis(fp)) parts.push(`bkt=${formatBucketWidth(fp.bucket_size_amount)}`);
  return parts.join(' ');
}

/** Closed-input / dropdown title for a fingerprint option. */
export function fingerprintSelectLabel(fp: Fingerprint): string {
  const name = fp.name || fp.id.slice(0, 8);
  return fp.used_by != null ? `${name} · used by ${fp.used_by}` : name;
}

/** Dropdown row: name plus the same axis chips the tables show. */
export function FingerprintOptionBody({
  fp,
  label,
}: {
  fp: Fingerprint;
  label: string;
}): ReactNode {
  const auto = fingerprintAutoName(fp);
  return (
    <div className="flex min-w-0 flex-col gap-1">
      <span className="truncate font-medium">{label}</span>
      {fp.name.trim() !== auto && (
        <span
          className="truncate font-mono text-[10px] text-text-dim"
          title={`Auto-name from the match axes.\nTwo rows showing the same one match the same tokens, whatever they are called.`}
        >
          {auto}
        </span>
      )}
      {fingerprintParamsCell(fp)}
    </div>
  );
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
  return [
    fp.name || fp.id,
    // Identity, not decoration: a wildcard and an axis-free row would otherwise
    // sort as the same fingerprint while matching opposite token sets.
    fp.wildcard ? 'wildcard' : '',
    fp.cu_limit ?? '',
    fp.cu_price ?? '',
    fp.init_buy_lamports ?? '',
    fp.max_cost_lamports ?? '',
    fp.spendable_lamports_in ?? '',
    fp.first_slot_buy_lamports ?? '',
    fp.first_slot_sell_lamports ?? '',
    // The EFFECTIVE width: an inert one is not identity, and keying on it sorted
    // two rows matching the same tokens apart — the tell that used to hide them.
    hasSolAxis(fp) ? formatBucketWidth(fp.bucket_size_amount) : '',
    (configuredIxLabels(fp.ix_labels) ?? []).join(','),
    // The pattern SEQUENCES, not their count: two fingerprints matching one
    // pattern each are the same criterion only if it is the same pattern.
    volumePatternsIdentity(volumeIxPatternsFromConfig(fp.metric_config)),
  ].join('\u0001');
}
