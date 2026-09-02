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
import {
  formatVolumePatternsText,
  ixPatternsActions,
  ixPatternsIdentity,
} from 'lib/flow/volumePatterns';
import { cn } from 'lib/cn';
import { hashHue, metricColorStyle } from 'lib/strategy/metricColors';
import {
  dumpPatternsFromConfig,
  ixPatternsFromConfig,
  targetWalletsFromConfig,
  workingTemplatesFromConfig,
} from 'lib/strategy/registry';
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
  // flow-split tagged ix patterns — cyan
  flow: 180,
  // dump builds — the `m_dump_ix` group's own registry hue, so the chip and the
  // metric line on a chart are the same colour for the same list.
  dump: 115,
  // working templates — harvest grains (orange, next to dump's teal)
  work: 25,
  // copy targets — the `m_copy` group's own registry hue, so the chip and the
  // metric line on a chart are the same colour for the same list.
  copy: 282,
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

/**
 * The `flow N` chip — cyan body (the flow-split family tone). See
 * {@link ContentsChip}; the tooltip lists each pattern as its action sequence
 * and a click copies the whole set as JSON.
 *
 * Unlike every other axis, the unconfigured state stays VISIBLE as a dimmed
 * `flow✗`: an empty set is not a dropped criterion, it is the verdict "no trade
 * on this fingerprint is tagged", which silently changes what `untagged_*` reads.
 * A missing chip among nine reads as "didn't look".
 */
export function FlowPatternsChip({ patterns }: { patterns: string[][] }) {
  const n = patterns.length;
  if (n === 0) {
    return <>{chip('flow✗', { title: 'no ix patterns — nothing is tagged', style: OFF_TINT })}</>;
  }
  return (
    <ContentsChip
      text={`flow ${n}`}
      identity={ixPatternsIdentity(patterns)}
      title={`${n} volume ix pattern${n === 1 ? '' : 's'}\n${ixPatternsActions(patterns)}`}
      copyText={formatVolumePatternsText(patterns)}
      tint={axisTint('flow')}
    />
  );
}

/**
 * The `dump N` chip — the `m_dump_ix.ix_patterns` twin of {@link FlowPatternsChip}.
 *
 * Absent when the list is empty, where the flow chip stays visible as `flow✗`. The
 * asymmetry is the two lists' meanings: an empty flow list re-points `untagged_*` at
 * every trade, so it is a verdict worth stating on every row, while an empty dump
 * list leaves `dump_*` reading `NaN` — a metric no rule can be gating on, and a chip
 * on all 100-odd fingerprints saying so is noise. The fingerprints table gives it a
 * COLUMN, where an empty cell is a dash like any other.
 */
export function DumpPatternsChip({ patterns }: { patterns: string[][] }) {
  const n = patterns.length;
  if (n === 0) return null;
  return (
    <ContentsChip
      text={`dump ${n}`}
      identity={ixPatternsIdentity(patterns)}
      title={`${n} dump ix build${n === 1 ? '' : 's'} — their SELLS are what dump_sell / dump_sell_count count\n${ixPatternsActions(patterns)}`}
      copyText={formatVolumePatternsText(patterns)}
      tint={axisTint('dump')}
    />
  );
}

/**
 * The `work N` chip — `m_burst_slot.working_templates`. Absent when empty: an
 * empty list leaves burst metrics reading NaN, so a chip on every fingerprint
 * saying so is noise (same reason {@link DumpPatternsChip} hides).
 */
export function WorkingTemplatesChip({ templates }: { templates: string[] }) {
  const n = templates.length;
  if (n === 0) return null;
  const text = templates.join('\n');
  return (
    <ContentsChip
      text={`work ${n}`}
      identity={templates.slice().sort().join('|')}
      title={`${n} working template grain${n === 1 ? '' : 's'}\n${text}`}
      copyText={text}
      tint={axisTint('work')}
    />
  );
}

/**
 * The `copy N` chip — `m_copy.target_wallets`.
 *
 * Load-bearing on a copy fingerprint in a way the other contents chips are not: a
 * copy row is a WILDCARD, so without this its whole summary reads `ALL tokens` and
 * the one thing that identifies the rule — whose trades it follows — is invisible.
 * The tooltip carries the addresses; a click copies them.
 */
export function TargetWalletsChip({ wallets }: { wallets: string[] }) {
  const n = wallets.length;
  if (n === 0) return null;
  const text = wallets.join('\n');
  return (
    <ContentsChip
      text={n === 1 ? `copy ${wallets[0].slice(0, 4)}` : `copy ${n}`}
      identity={wallets.slice().sort().join('|')}
      title={`${n} target wallet${n === 1 ? '' : 's'}\n${text}`}
      copyText={text}
      tint={axisTint('copy')}
    />
  );
}

/**
 * One fingerprint-scoped `metric_config` list, in every form the surfaces that
 * show a fingerprint need it.
 *
 * **The reason this table exists.** The criteria axes are generated from the axis
 * registry, so an axis added there becomes a chip, a column, a search term and part
 * of the sort key with no edit. The `metric_config` lists had no such table and were
 * written out by hand at four call sites — so `m_burst_slot` and `m_copy` shipped
 * with a form control and a chip, but no column, no search text and no identity
 * term, and two fingerprints differing only in whose wallet they copy rendered
 * identically on the Fingerprints page. `metric_config` IS row identity
 * (`fingerprints_identity_uniq`), so that is not cosmetic: it hides the one field
 * that says the rows are different.
 *
 * The engine's own list is hand-kept the same way (`FingerprintPatterns` has one
 * field per group, `validate_fingerprint_metric_config` one call), so this mirrors
 * it one-for-one. `fingerprintConfigLists.test.ts` reads the Rust registry and fails
 * when a group declares `fingerprint_config` with no entry here.
 */
export interface FpConfigListSpec {
  /** Chip prefix and search token (`flow`, `dump`, `work`, `copy`). */
  key: string;
  /** The `metric_config` group this list lives under — the guard test's join key. */
  group: string;
  /** Fingerprints-table column key. Stable: it keys saved sort/visibility prefs. */
  columnKey: string;
  /** Fingerprints-table column header. */
  label: string;
  /** Column tooltip — what the list does, one line. */
  definition: string;
  /** Configured entries (0 = the group is unconfigured and its metrics read NaN). */
  count: (fp: Fingerprint) => number;
  /** The chip. Empty renders per that list's own rule — see {@link FlowPatternsChip}
   *  for why only `flow` stays visible when empty. */
  chip: (fp: Fingerprint) => ReactNode;
  /** Flat text for a table filter or the picker search: the chip's own words plus
   *  the CONTENTS, so searching a build or a wallet address finds the row that
   *  names it — a count alone can never answer that. */
  searchText: (fp: Fingerprint) => string;
  /** Order-independent identity of the contents, for the whole-fingerprint sort
   *  key. Two rows with one entry each are the same criterion only if it is the
   *  same entry. */
  identity: (fp: Fingerprint) => string;
}

/** Every fingerprint-scoped `metric_config` list, in registry order. */
export const FP_CONFIG_LISTS: readonly FpConfigListSpec[] = [
  {
    key: 'flow',
    group: 'm_flow_ix',
    columnKey: 'flow_patterns',
    label: 'flow patterns',
    definition:
      'm_flow_ix.ix_patterns — the builds the flow split calls volume-side. An empty list re-points every untagged_* metric at EVERY trade, so it shows as flow✗ rather than a dash.',
    count: (fp) => ixPatternsFromConfig(fp.metric_config).length,
    chip: (fp) => <FlowPatternsChip patterns={ixPatternsFromConfig(fp.metric_config)} />,
    searchText: (fp) => {
      const p = ixPatternsFromConfig(fp.metric_config);
      // The unconfigured state is searchable here and nowhere else: it is a verdict
      // ("nothing is tagged"), not a dropped criterion.
      if (p.length === 0) return 'flow✗';
      return `flow ${p.length} flow=${p.length} ${ixPatternsActions(p)} ${formatVolumePatternsText(p)}`;
    },
    identity: (fp) => ixPatternsIdentity(ixPatternsFromConfig(fp.metric_config)),
  },
  {
    key: 'dump',
    group: 'm_dump_ix',
    columnKey: 'dump_patterns',
    label: 'dump builds',
    definition:
      'm_dump_ix.ix_patterns — the builds whose SELLS dump_sell / dump_sell_count count. Its own list, free to overlap the flow one: the two ask different questions of one transaction.',
    count: (fp) => dumpPatternsFromConfig(fp.metric_config).length,
    chip: (fp) => <DumpPatternsChip patterns={dumpPatternsFromConfig(fp.metric_config)} />,
    searchText: (fp) => {
      const p = dumpPatternsFromConfig(fp.metric_config);
      if (p.length === 0) return '';
      return `dump ${p.length} dump=${p.length} ${ixPatternsActions(p)} ${formatVolumePatternsText(p)}`;
    },
    identity: (fp) => ixPatternsIdentity(dumpPatternsFromConfig(fp.metric_config)),
  },
  {
    key: 'work',
    group: 'm_burst_slot',
    columnKey: 'working_templates',
    label: 'working templates',
    definition:
      'm_burst_slot.working_templates — build-template grain ids (program|CU|ATA|N|S|F) whose prints the burst group treats as working. Grain ids, NOT ix_labels sequences. Absent ⇒ every burst metric reads NaN, never 0.',
    count: (fp) => workingTemplatesFromConfig(fp.metric_config).length,
    chip: (fp) => <WorkingTemplatesChip templates={workingTemplatesFromConfig(fp.metric_config)} />,
    searchText: (fp) => {
      const t = workingTemplatesFromConfig(fp.metric_config);
      if (t.length === 0) return '';
      return `work ${t.length} work=${t.length} ${t.join(' ')}`;
    },
    identity: (fp) => workingTemplatesFromConfig(fp.metric_config).slice().sort().join('|'),
  },
  {
    key: 'copy',
    group: 'm_copy',
    columnKey: 'target_wallets',
    label: 'copy targets',
    definition:
      "m_copy.target_wallets — base58 wallets both copy groups follow, matched against the address the VENUE credited. A copy fingerprint is a WILDCARD, so this list is the only thing that says whose trades it follows. Absent ⇒ every copy metric reads NaN, never 0.",
    count: (fp) => targetWalletsFromConfig(fp.metric_config).length,
    chip: (fp) => <TargetWalletsChip wallets={targetWalletsFromConfig(fp.metric_config)} />,
    searchText: (fp) => {
      const w = targetWalletsFromConfig(fp.metric_config);
      if (w.length === 0) return '';
      // The whole address, so pasting one selects the fingerprint that follows it.
      return `copy ${w.length} copy=${w.length} ${w.join(' ')}`;
    },
    identity: (fp) => targetWalletsFromConfig(fp.metric_config).slice().sort().join('|'),
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
  // Every fingerprint-scoped list, from the ONE table — so a group added to the
  // engine's `fingerprint_config` shows up here as soon as it has an entry.
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
    // Every fingerprint-scoped list's CONTENTS, not their counts: two fingerprints
    // naming one entry each are the same criterion only if it is the same entry.
    // `metric_config` is row identity, so two rows differing only in one of these
    // lists are different rows and must not collapse into one sort key — which is
    // exactly what a copy fingerprint did while `m_copy` was missing from here.
    ...FP_CONFIG_LISTS.map((s) => s.identity(fp)),
  ].join('\u0001');
}
