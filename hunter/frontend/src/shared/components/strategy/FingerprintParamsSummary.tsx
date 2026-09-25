// Compact at-a-glance chip cluster for a fingerprint's match axes. Shared by
// Rules, Simulate, and the rule-editor picker — one SSOT so every surface that
// shows a fingerprint reads the same.
//
// Chips are generated from the axis registry in its own order, so an axis added
// there is shown, searchable and part of the sort key here without an edit. An
// unconfigured axis is simply absent from the criteria map and so from the row.

import { Fragment, useState, type CSSProperties, type MouseEvent, type ReactNode } from 'react';
import {
  axisDef,
  configuredAxes,
  formatPredicate,
  predicateSpans,
  type AxisId,
  type AxisPredicate,
} from 'lib/strategy/fingerprintAxes';
import {
  configuredIxLabels,
  formatIxLabelsText,
  ixLabelsActions,
  ixLabelsCountTail,
} from 'lib/ixLabels';
import { cn } from 'lib/cn';
import { hashHue, metricColorStyle } from 'lib/strategy/metricColors';
import { tagsFromJson, tagsToJson, usedMatchers, type TagDef } from 'lib/strategy/tagsDoc';
import { tagSentence } from 'lib/strategy/tagsDoc';
import { useGetFingerprintsQuery } from 'store/sharedEndpoints';
import {
  fingerprintAutoName,
} from 'lib/strategy/fingerprintNameFromGroupKey';
import {
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
  // fingerprint tags — cyan
  flow: 180,
  // bucket width — rose
  bkt: 340,
  // wildcard — its own hue: it is not an axis, it replaces all of them
  wildcard: 20,
};


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
 * `ixLabelsCountTail` / `ixPatternsActions`.
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

/** The `@volume` chip: one per fingerprint tag. The tooltip says what the tag
 *  matches; a click copies its definition as JSON. */
export function TagChip({ tag }: { tag: TagDef }) {
  const json = JSON.stringify(tagsToJson([tag])[tag.name]);
  return (
    <ContentsChip
      text={`@${tag.name} ${usedMatchers(tag).length}`}
      identity={json}
      title={`${tagSentence(tag)}\n\nRules read @${tag.name} (trades with it) and @!${tag.name} (the rest). Click to copy.`}
      copyText={json}
      tint={axisTint('flow')}
    />
  );
}

/**
 * One fingerprint-scoped list, in every form the surfaces that show a fingerprint
 * need it: chip, table column, search text and sort key.
 *
 * `tags` is part of the row's IDENTITY (`fingerprints_identity_uniq` is criteria +
 * wildcard + tags), so every surface shows it: two rows that classify trades
 * differently must never render the same. A copy fingerprint is a wildcard, so its
 * `targets` tag is the only thing that says whose trades it follows.
 */
export interface FpConfigListSpec {
  /** Chip prefix and search token. */
  key: string;
  /** The fingerprint field this list lives under. */
  group: string;
  /** Fingerprints-table column key. Stable: it keys saved sort/visibility prefs. */
  columnKey: string;
  /** Fingerprints-table column header. */
  label: string;
  /** Column tooltip: what the list does, one line. */
  definition: string;
  /** Configured entries. */
  count: (fp: Fingerprint) => number;
  chip: (fp: Fingerprint) => ReactNode;
  /** Flat text for a table filter or the picker search: names AND contents, so
   *  searching a program or a wallet address finds the row that names it. */
  searchText: (fp: Fingerprint) => string;
  /** Order-independent identity of the contents, for the sort key. */
  identity: (fp: Fingerprint) => string;
}

/** Stable JSON: object keys sorted, so two equal documents give one string. */
function stableJson(v: unknown): string {
  if (Array.isArray(v)) return `[${v.map(stableJson).join(',')}]`;
  if (v && typeof v === 'object') {
    return `{${Object.keys(v as Record<string, unknown>)
      .sort()
      .map((k) => `${JSON.stringify(k)}:${stableJson((v as Record<string, unknown>)[k])}`)
      .join(',')}}`;
  }
  return JSON.stringify(v);
}

/** Every fingerprint-scoped list: the tags. */
export const FP_CONFIG_LISTS: readonly FpConfigListSpec[] = [
  {
    key: 'tags',
    group: 'tags',
    columnKey: 'tags',
    label: 'tags',
    definition:
      'Named trade lists. Rules read @name (the trades with the tag) and @!name (the rest). A metric that needs a tag the fingerprint does not define reads nothing.',
    count: (fp) => tagsFromJson(fp.tags).length,
    chip: (fp) => (
      <>
        {tagsFromJson(fp.tags).map((t) => (
          <TagChip key={t.name} tag={t} />
        ))}
      </>
    ),
    searchText: (fp) =>
      tagsFromJson(fp.tags)
        .map((t) => `@${t.name} ${tagSentence(t)} ${JSON.stringify(tagsToJson([t]))}`)
        .join(' '),
    identity: (fp) => stableJson(fp.tags ?? {}),
  },
];

/** One axis's chip. The value reads in the axis's own display unit — SOL for a
 *  lamports axis, the integer for a tally — through the ONE formatter, so a chip,
 *  the auto-name and the form all show a bound the same way. */
function axisChip(id: AxisId, pred: AxisPredicate): ReactNode | null {
  const def = axisDef(id);
  if (pred.kind === 'sequence') {
    const ix = configuredIxLabels(pred.labels);
    return ix ? <IxLabelsChip key={id} labels={ix} /> : null;
  }
  const unit = def.unit === 'lamports' ? '◎' : '';
  return (
    <span key={id}>
      {chip(`${def.chip}=${formatPredicate(id, pred)}${unit}`, {
        style: axisTint(def.chip),
        // The axis's ONE definition, rendered from the registry — never a second copy.
        title: `${def.label} — ${def.definition}`,
      })}
    </span>
  );
}

/** Axis chips only (no name) — the configured criteria plus the flow-pattern axis. */
export function fingerprintParamsCell(fp: Fingerprint): ReactNode {
  // A wildcard carries no axis, so the chip row is the one chip that says what it
  // matches. Rendering the axis chips too (they would all be absent) would read as
  // "unconfigured" — the opposite of what it does.
  const listChips = FP_CONFIG_LISTS.map((s) => (
    <Fragment key={s.key}>{s.chip(fp)}</Fragment>
  ));
  if (fp.wildcard) {
    return (
      <div className="flex flex-wrap items-center gap-1 text-left">
        {chip('ALL tokens', {
          style: axisTint('wildcard'),
          title: 'Wildcard — matches every token, ignoring every creation-shape axis',
        })}
        {listChips}
      </div>
    );
  }
  const chips: ReactNode[] = [
    ...configuredAxes(fp.criteria ?? {}).map(([id, pred]) => axisChip(id, pred)),
    ...listChips,
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
  for (const [id, pred] of configuredAxes(fp.criteria ?? {})) {
    const def = axisDef(id);
    if (pred.kind === 'sequence') {
      const ix = configuredIxLabels(pred.labels);
      if (!ix) continue;
      // Count keeps `3ix` matchable; the tail + actions make the *sequence*
      // filterable, so two same-length sets don't both answer to one query.
      parts.push(`${ix.length}ix`, ixLabelsCountTail(ix), ixLabelsActions(ix), formatIxLabelsText(ix));
      continue;
    }
    parts.push(`${def.chip}=${formatPredicate(id, pred)}`, def.label);
  }
  // Each list's own words plus its CONTENTS, from the ONE table. The contents go in
  // for the same reason `ixLabelsActions` does: searching `Transfer` must find the
  // fingerprints that classify it as volume, and pasting a wallet address must find
  // the one that copies it — a count alone can never answer either.
  for (const s of FP_CONFIG_LISTS) {
    const text = s.searchText(fp);
    if (text) parts.push(text);
  }
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
    // Every configured axis, in registry order — so a new axis is part of the sort
    // key without an edit here, and two rows differing only on one still sort apart.
    configuredAxes(fp.criteria ?? {})
      .map(([id, pred]) =>
        pred.kind === 'sequence'
          ? `${id}:${(configuredIxLabels(pred.labels) ?? []).join(',')}`
          // Every span, so a `!=` / `|` axis sorts on the whole set it accepts
          // rather than on its first window.
          : `${id}:${predicateSpans(pred)
              .map((sp) => `${sp.min ?? ''}-${sp.max ?? ''}`)
              .join('+')}`,
      )
      .join(''),
    // The tags' CONTENTS, not their count: `tags` is row identity, so two rows that
    // classify trades differently must not collapse into one sort key.
    ...FP_CONFIG_LISTS.map((s) => s.identity(fp)),
  ].join('\u0001');
}
