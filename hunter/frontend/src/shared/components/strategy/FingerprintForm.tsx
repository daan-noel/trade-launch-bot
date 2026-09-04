import { useEffect, useMemo, useRef, useState } from 'react';

import { Input } from 'components/ui/Input';
import { IconButton } from 'components/ui/IconButton';
import { SaveIcon, SpinnerIcon, RefreshIcon, PlusIcon } from 'components/ui/icons';
import { Button } from 'components/ui/Button';
import { Checkbox } from 'components/ui/Checkbox';
import { IxLabelsInput } from 'components/ui/IxLabelsInput';
import { configuredIxLabels, formatIxLabelsText, parseIxLabelsText } from 'lib/ixLabels';
import { WILDCARD_NAME, type Fingerprint, type FingerprintDraft } from 'lib/strategy/types';
import {
  AXES,
  axisDef,
  criteriaProblems,
  type AxisDef,
  type AxisId,
  type Criteria,
} from 'lib/strategy/fingerprintAxes';
import { axisPredicateText } from 'lib/strategy/fingerprintGrammar';
import {
  findGroup,
  dumpPatternRowsFromConfig,
  flowClassifierFromConfig,
  ixMarkers,
  metricConfigWithDumpPatterns,
  metricConfigWithFlowClassifier,
  metricConfigWithTargetWallets,
  metricConfigWithWorkingTemplates,
  useStrategyRegistry,
  targetWalletsFromConfig,
  workingTemplatesFromConfig,
  type FlowClassifier,
  type FpConfigFieldSpec,
  type MarkerSide,
  type StrategyRegistry,
  BURST_GROUP,
  COPY_GROUP,
  DUMP_GROUP,
} from 'lib/strategy/registry';
import { fingerprintAutoName, isStaleAutoName } from 'lib/strategy/fingerprintNameFromGroupKey';
import { FINGERPRINT_FIELD_HELP, type HelpTip } from 'lib/strategy/strategyHelp';
import { LabelTip } from './LabelTip';
import { parseGrainIds } from 'lib/strategy/templateGrain';
import type { IxPatternRow } from 'lib/strategy/ixPatternRows';

import { IxPatternRowsEditor } from './IxPatternsEditor';
import {
  AxisConditionInput,
  axisConditionProblem,
  axisConditionState,
} from './AxisConditionInput';

export interface FingerprintFormProps {
  /** Existing fingerprint to edit; omit to create. */
  initial?: Fingerprint;
  onSubmit: (draft: FingerprintDraft) => void;
  onCancel?: () => void;
  submitting?: boolean;
  error?: string | null;
}

interface FormState {
  name: string;
  /** Per-axis condition expression, as the operator typed it, in the axis's own
   *  display unit. Kept as raw text so a half-typed `1.` is not silently rounded
   *  mid-keystroke and so an unparseable expression can be *shown* as an error
   *  rather than dropped — a dropped axis reads as "unconstrained", which widens
   *  the match. */
  conditions: Partial<Record<AxisId, string>>;
  /** Textarea text — pretty JSON string array (see `parseIxLabelsText`). */
  ix_labels: string;
  /** Match EVERY token, ignoring every axis. Mutually exclusive with the axes
   *  (backend `validate` + the `fingerprints_wildcard_excludes_axes` CHECK), so
   *  turning it on clears them rather than leaving a contradiction on screen. */
  wildcard: boolean;
  /** The WHOLE `m_flow_ix` classifier — patterns, marker mask and side, and the two
   *  wallet rules. One value rather than a field per key, because the group is written
   *  as a whole: a save that rebuilds it from a subset lands as a full write and drops
   *  the rest, which is how the marker masks used to be deleted on every edit. */
  flow: FlowClassifier;
  /** `m_dump_ix.ix_patterns` — the builds whose SELLS `dump_sell` / `dump_sell_count`
   *  count. A separate list from the classifier's, and separately optional: a
   *  fingerprint may configure either group, both, or neither. */
  dump_patterns: IxPatternRow[];
  /** `m_burst_slot.working_templates` — grains or program names. */
  working_templates: string[];
  /** `m_copy.target_wallets` — the addresses a copy rule follows. Its own list and
   *  its own vocabulary: every other fingerprint list names a BUILD, this one names
   *  who signed. */
  target_wallets: string[];
  /** The ORIGINAL metric_config, whole. The save merges into this rather than
   *  rebuilding from the fields above, so every key the form does not render — other
   *  groups, and any `m_flow_ix` key added later — survives an edit. */
  metric_config_prev: Record<string, unknown>;
}


const NUMERIC_AXES: readonly AxisDef[] = AXES.filter((a) => a.kind === 'numeric');

function fromFingerprint(fp?: Fingerprint, reg?: StrategyRegistry): FormState {
  const cfg = fp?.metric_config ?? {};
  const criteria = fp?.criteria ?? {};
  const conditions: Partial<Record<AxisId, string>> = {};
  for (const def of NUMERIC_AXES) {
    conditions[def.id] = axisPredicateText(def.id, criteria[def.id]);
  }
  const labels = criteria.ix_labels;
  return {
    name: fp?.name ?? '',
    conditions,
    ix_labels: formatIxLabelsText(labels?.kind === 'sequence' ? labels.labels : null),
    wildcard: fp?.wildcard ?? false,
    flow: flowClassifierFromConfig(cfg, reg),
    dump_patterns: dumpPatternRowsFromConfig(cfg),
    working_templates: workingTemplatesFromConfig(cfg),
    target_wallets: targetWalletsFromConfig(cfg),
    metric_config_prev: cfg,
  };
}

/** The criteria the form currently configures, plus every axis whose expression
 *  does not say something storable.
 *
 *  An unreadable expression is reported, never dropped: dropping it leaves the axis
 *  unconfigured, which matches MORE tokens than the operator asked for — the silent
 *  direction. Read through `axisConditionState`, the same interpreter the input
 *  renders its chips from, so the form can never save something other than what it
 *  shows. */
function toCriteria(s: FormState): { criteria: Criteria; badAxes: string[] } {
  const criteria: Criteria = {};
  const badAxes: string[] = [];
  // A wildcard row carries NO axis — the backend rejects one that does, and the
  // matcher would ignore it anyway. Dropping them here (rather than only disabling
  // the inputs) means a form that was filled in first still saves as what it reads
  // as now.
  if (s.wildcard) return { criteria, badAxes };

  for (const def of NUMERIC_AXES) {
    const state = axisConditionState(s.conditions[def.id] ?? '', def);
    if (state.kind === 'ok') {
      criteria[def.id] = state.predicate;
      continue;
    }
    // Blank ⇒ the axis is not part of identity. There is exactly one spelling of
    // that: absent from the map.
    const problem = axisConditionProblem(state, def);
    if (problem) badAxes.push(problem);
  }

  const { labels } = parseIxLabelsText(s.ix_labels);
  const configured = configuredIxLabels(labels);
  if (configured) criteria.ix_labels = { kind: 'sequence', labels: configured };
  return { criteria, badAxes };
}

function toDraft(s: FormState): FingerprintDraft {
  return {
    name: s.name.trim(),
    wildcard: s.wildcard,
    criteria: toCriteria(s).criteria,
    // Each group through its own writer, chained so both land in one config and
    // neither drops what the other owns.
    metric_config: metricConfigWithTargetWallets(
      metricConfigWithWorkingTemplates(
        metricConfigWithDumpPatterns(
          metricConfigWithFlowClassifier(s.metric_config_prev, s.flow),
          s.dump_patterns,
        ),
        s.working_templates,
      ),
      s.target_wallets,
    ),
  };
}

/** The `m_flow_ix` editor — the whole classifier, generated from the group's declared
 *  fields.
 *
 *  Every key the classifier reads gets a control here. It used to render `ix_patterns`
 *  alone, which left the marker masks — the vocabulary a purely STRUCTURAL gate is
 *  stated in, and the one the routers live in — reachable only by hand-posting JSON,
 *  and then deleted by the next save from this form. The two wallet rules were shown
 *  only when a pattern row existed, so a classifier made of markers, or of the creator
 *  and contagion alone, had no editor either.
 *
 *  A field the registry does not declare renders nothing, and a field it adds renders
 *  with its own definition as the tooltip. */
function FlowClassifierEditor({
  value,
  fields,
  markers,
  disabled,
  onChange,
}: {
  value: FlowClassifier;
  fields: FpConfigFieldSpec[];
  markers: { name: string; router: boolean }[];
  disabled?: boolean;
  onChange: (patch: Partial<FlowClassifier>) => void;
}) {
  const field = (name: string) => fields.find((f) => f.name === name);
  const tip = (name: string, fallback: HelpTip): HelpTip => {
    const body = field(name)?.description;
    // The registry definition wins, with the frontend copy BELOW it — the same
    // resolution a metric tooltip uses, so the definition can never say something the
    // engine does not while the worked guidance still gets to be long.
    return body ? { title: fallback.title, body: `${body}

${fallback.body}` } : fallback;
  };
  // The masks name opposite sides of one split, so the row picks a side and the OTHER
  // key is never written. Declared in the registry (`conflicts_with`) rather than
  // assumed, and the backend rejects the combination the same way.
  const markerField = field('untagged_ix_markers') ?? field('tagged_ix_markers');
  const patternsBlocked =
    value.markers_side === 'untagged' &&
    value.markers.length > 0 &&
    (field('untagged_ix_markers')?.conflicts_with ?? []).includes('ix_patterns');

  const toggleMarker = (name: string) =>
    onChange({
      markers: value.markers.includes(name)
        ? value.markers.filter((m) => m !== name)
        : [...value.markers, name],
    });

  return (
    <div className="flex flex-col gap-2 text-[11px] text-text-dim">
      <div className="flex flex-col gap-1">
        <LabelTip tip={tip('ix_patterns', FINGERPRINT_FIELD_HELP.ix_patterns)}>
          ix_patterns (m_flow_ix)
        </LabelTip>
        <IxPatternRowsEditor
          rows={value.ix_patterns.map((p) => (Array.isArray(p) ? { labels: p } : p))}
          onChange={(p) => onChange({ ix_patterns: p })}
          disabled={disabled || patternsBlocked}
        />
        {patternsBlocked && (
          <span className="text-text-dim/80">
            an untagged-marker mask already judges every build, so a pattern list has
            nothing left to say — switch the side to add one
          </span>
        )}
      </div>

      {markerField && markers.length > 0 && (
        <div className="flex flex-col gap-1">
          <LabelTip tip={tip(markerField.name, FINGERPRINT_FIELD_HELP.ix_markers)}>
            ix markers (m_flow_ix)
          </LabelTip>
          <div className="flex items-center gap-2">
            {(['tagged', 'untagged'] as MarkerSide[]).map((side) => (
              <label key={side} className="flex cursor-pointer items-center gap-1 text-text-mid">
                <input
                  type="radio"
                  name="marker-side"
                  checked={value.markers_side === side}
                  disabled={disabled}
                  onChange={() => onChange({ markers_side: side })}
                />
                <span>{side === 'tagged' ? 'a marker TAGS' : 'a marker leaves UNTAGGED'}</span>
              </label>
            ))}
          </div>
          <div className="flex flex-wrap gap-1">
            {markers.map((m) => (
              <button
                key={m.name}
                type="button"
                disabled={disabled}
                onClick={() => toggleMarker(m.name)}
                className={
                  'rounded border px-1.5 py-0.5 font-mono text-[10px] ' +
                  (value.markers.includes(m.name)
                    ? 'border-accent/60 bg-accent/15 text-text-hi'
                    : 'border-white/10 text-text-dim hover:text-text-mid')
                }
                title={m.router ? 'a retail router - a person clicked through it' : 'machinery'}
              >
                {m.name}
              </button>
            ))}
          </div>
        </div>
      )}

      <div className="flex flex-col gap-1 pl-1">
        {(['wallet_contagion', 'creator_is_tagged'] as const).map((name) =>
          field(name) ? (
            <label key={name} className="flex cursor-pointer items-start gap-1.5 text-text-mid">
              <Checkbox
                boxSize="sm"
                className="mt-0.5"
                checked={value[name]}
                disabled={disabled}
                onChange={() => onChange({ [name]: !value[name] })}
              />
              <LabelTip tip={tip(name, FINGERPRINT_FIELD_HELP[name])}>
                {name.replace(/_/g, ' ')}
              </LabelTip>
            </label>
          ) : null,
        )}
        {(value.wallet_contagion || value.creator_is_tagged) && (
          <span className="text-text-dim/80">
            a tag is a property of the WALLET here — untick both for a purely structural
            gate
          </span>
        )}
      </div>
    </div>
  );
}

/** Why every axis input greys out under the wildcard. */
const AXIS_DISABLED_TITLE =
  'A wildcard fingerprint matches every token, so it carries no axes.' +
  '\nUncheck "match every token" to narrow it by creation shape.';

/** How an axis condition reads, spelled out once for every axis tooltip.
 *
 *  One expression per axis rather than a pair of boxes, because exact, band, open
 *  end, gap and alternatives are all the same question — which values pass — and a
 *  pair of boxes can only ask two of the five. */
function boundsHelp(def: AxisDef): HelpTip {
  const u = def.unit === 'lamports' ? '◎' : '';
  const [lo, hi] = def.unit === 'lamports' ? ['1.5', '2'] : ['3', '5'];
  return {
    title: def.label,
    // The axis's ONE definition, rendered from the registry — never a second copy
    // that can say something the matcher does not do.
    body: [
      def.definition,
      '',
      '`..` is INCLUSIVE at both ends; `-` is the half-open form a group chip spans, so a chip pasted here selects that chip\'s tokens.',
      '`,` is AND and `|` is OR, the same as a rule condition.',
      '',
      def.phase === 'first_slot'
        ? 'Settles only after the creation slot closes, so a rule using it cannot fire at birth.'
        : 'Known at creation.',
    ].join('\n'),
    figure: [
      `${lo}${u}          exactly ${lo}${u}`,
      `${lo}..${hi}${u}      ${lo} to ${hi}, both ends in`,
      `${lo}-${hi}${u}       ${lo} up to but NOT ${hi}`,
      `>=${lo}${u}        ${lo}${u} or more   (also >, <, <=)`,
      `!=${lo}${u}        anything but ${lo}${u}`,
      `<=${lo}${u} | >=${hi}${u}  either side of the gap`,
    ].join('\n'),
  };
}

const NAME_HELP: HelpTip = {
  title: 'Name',
  body:
    'Picker handle and log label. NOT identity — renaming changes nothing about what this fingerprint matches.\n\n' +
    'Left blank, or written in the generated grammar, it re-derives from the axes on every edit. A nickname you type is never overwritten: it is the only record of WHY this fingerprint exists, and the axes can always be re-read.',
};

const WILDCARD_HELP: HelpTip = {
  title: 'Match every token',
  body:
    'Arms on EVERY token, ignoring every axis.\n\n' +
    'A rule always needs a fingerprint, but one deciding purely on what the tape is doing has no creation shape to name — and clearing every axis means MATCH NOTHING, because the matcher refuses a criterion-less row on purpose. So "every token" is said out loud here rather than inferred from a blank form.\n\n' +
    'Mutually exclusive with the axes: turning it on clears them.',
};

/** Chip list for `m_burst_slot.working_templates` — grains or program names. */
function WorkingTemplatesEditor({
  value,
  onChange,
  field,
  disabled,
}: {
  value: string[];
  onChange: (next: string[]) => void;
  field: FpConfigFieldSpec;
  disabled?: boolean;
}) {
  const [draft, setDraft] = useState('');
  const tip: HelpTip = {
    title: `${BURST_GROUP}.working_templates`,
    body: field.description
      ? `${field.description}

${FINGERPRINT_FIELD_HELP.working_templates.body}`
      : FINGERPRINT_FIELD_HELP.working_templates.body,
  };
  const add = () => {
    const ids = parseGrainIds(draft);
    if (ids.length === 0) return;
    const seen = new Set(value);
    onChange([...value, ...ids.filter((id) => !seen.has(id))]);
    setDraft('');
  };
  return (
    <div className="flex flex-col gap-1.5 rounded border border-white/10 p-2 text-[11px] text-text-dim">
      <LabelTip tip={tip}>
        {BURST_GROUP} · working templates
        {value.length > 0 && (
          <span className="ml-1 font-normal text-text-dim/70">{value.length}</span>
        )}
      </LabelTip>
      {value.length === 0 ? (
        <p className="rounded border border-dashed border-white/10 px-2 py-2 text-text-dim/50">
          No working list. Paste a grain (`Axiom Trade|CU|ATA|F`) or a program
          name (`Axiom Trade`), or on the tape switch the list to{' '}
          <span className="font-semibold text-text-dim">working</span> and click a badge
          (grain by default; flip the strip to program). Burst metrics read NaN
          until this list is set.
        </p>
      ) : (
        <div className="flex flex-wrap gap-1">
          {value.map((id) => (
            <button
              key={id}
              type="button"
              disabled={disabled}
              onClick={() => onChange(value.filter((x) => x !== id))}
              className="inline-flex items-center gap-1 rounded border border-green/40 bg-green/10 px-1.5 py-0.5 font-mono text-[10px] text-text-hi hover:border-red/50 hover:bg-red/10"
              title="Remove from working list"
            >
              {id}
              <span aria-hidden className="text-text-dim/60">
                ×
              </span>
            </button>
          ))}
        </div>
      )}
      <div className="flex items-center gap-1">
        <Input
          fieldSize="sm"
          className="min-w-0 flex-1 font-mono"
          value={draft}
          disabled={disabled}
          placeholder="Axiom Trade  or  Axiom Trade|CU|ATA|F"
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault();
              add();
            }
          }}
        />
        <IconButton
          variant="ghost"
          size="sm"
          disabled={disabled || parseGrainIds(draft).length === 0}
          onClick={add}
          title="Add grain id"
          aria-label="Add grain id"
        >
          <PlusIcon />
        </IconButton>
      </div>
    </div>
  );
}

/** Split a paste into addresses. Commas, whitespace and newlines all separate:
 *  an address never contains any of them, and an operator pasting from a block
 *  explorer gets whichever one that page used. */
function parseWalletAddresses(text: string): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const raw of text.split(/[\s,]+/)) {
    const a = raw.trim();
    if (!a || seen.has(a)) continue;
    seen.add(a);
    out.push(a);
  }
  return out;
}

/** Chip list for `m_copy.target_wallets` — addresses, not a build vocabulary. */
function TargetWalletsEditor({
  value,
  onChange,
  field,
  disabled,
}: {
  value: string[];
  onChange: (next: string[]) => void;
  field: FpConfigFieldSpec;
  disabled?: boolean;
}) {
  const [draft, setDraft] = useState('');
  const tip: HelpTip = {
    title: `${COPY_GROUP}.target_wallets`,
    body: field.description
      ? `${field.description}

${FINGERPRINT_FIELD_HELP.target_wallets.body}`
      : FINGERPRINT_FIELD_HELP.target_wallets.body,
  };
  const add = () => {
    const addrs = parseWalletAddresses(draft);
    if (addrs.length === 0) return;
    const seen = new Set(value);
    onChange([...value, ...addrs.filter((a) => !seen.has(a))]);
    setDraft('');
  };
  return (
    <div className="flex flex-col gap-1.5 rounded border border-white/10 p-2 text-[11px] text-text-dim">
      <LabelTip tip={tip}>
        {COPY_GROUP} · target wallets
        {value.length > 0 && (
          <span className="ml-1 font-normal text-text-dim/70">{value.length}</span>
        )}
      </LabelTip>
      {value.length === 0 ? (
        <p className="rounded border border-dashed border-white/10 px-2 py-2 text-text-dim/50">
          No target. Copy metrics read NaN until one is set, so a rule on this
          fingerprint does nothing rather than firing on everyone.
        </p>
      ) : (
        <div className="flex flex-wrap gap-1">
          {value.map((addr) => (
            <button
              key={addr}
              type="button"
              disabled={disabled}
              onClick={() => onChange(value.filter((x) => x !== addr))}
              className="inline-flex items-center gap-1 rounded border border-green/40 bg-green/10 px-1.5 py-0.5 font-mono text-[10px] text-text-hi hover:border-red/50 hover:bg-red/10"
              title="Remove from target list"
            >
              {addr}
              <span aria-hidden className="text-text-dim/60">
                ×
              </span>
            </button>
          ))}
        </div>
      )}
      {value.length > 1 && (
        <p className="text-[10px] text-amber">
          One target per rule — the seat, the size gate and the exit are per-target
          questions, and a shared list makes one fire indistinguishable from another's.
        </p>
      )}
      <div className="flex items-center gap-1">
        <Input
          fieldSize="sm"
          className="min-w-0 flex-1 font-mono"
          value={draft}
          disabled={disabled}
          placeholder="target wallet address"
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault();
              add();
            }
          }}
        />
        <IconButton
          variant="ghost"
          size="sm"
          disabled={disabled || parseWalletAddresses(draft).length === 0}
          onClick={add}
          title="Add target wallet"
          aria-label="Add target wallet"
        >
          <PlusIcon />
        </IconButton>
      </div>
    </div>
  );
}

/**
 * Create / edit a fingerprint.
 *
 * Every numeric axis is ONE condition expression in its own display unit — SOL for
 * a lamports axis, the raw integer for a tally — converted to the integer identity
 * carries only at submit. One field rather than a min/max pair because exact,
 * band, open end, gap (`!=`) and alternatives (`|`) are all the same question, and
 * a pair of boxes can only ask two of them. Blocks submit until the draft would
 * pass the backend's own gate, so the form never discovers a rejection as a 400.
 */
export function FingerprintForm({
  initial,
  onSubmit,
  onCancel,
  submitting,
  error,
}: FingerprintFormProps) {
  const [s, setS] = useState<FormState>(() => fromFingerprint(initial));
  const set = <K extends keyof FormState>(k: K, v: FormState[K]) => setS((p) => ({ ...p, [k]: v }));
  const setCondition = (id: AxisId, v: string) =>
    setS((p) => ({ ...p, conditions: { ...p.conditions, [id]: v } }));

  const { data: registry } = useStrategyRegistry();
  // The `m_flow_ix` editor is generated from the group's declared fields, so a field
  // added to the registry gets its control and its tooltip with no change here.
  const flowFields = findGroup(registry, 'm_flow_ix')?.fingerprint_config ?? [];
  // The dump list is one declared field, so it needs no generated editor — but it is
  // still read from the registry, so a group that stops declaring it stops rendering.
  const dumpField = findGroup(registry, DUMP_GROUP)?.fingerprint_config?.find(
    (f) => f.name === 'ix_patterns',
  );
  const workingField = findGroup(registry, BURST_GROUP)?.fingerprint_config?.find(
    (f) => f.name === 'working_templates',
  );
  const targetField = findGroup(registry, COPY_GROUP)?.fingerprint_config?.find(
    (f) => f.name === 'target_wallets',
  );
  const dumpTip: HelpTip = {
    title: `${DUMP_GROUP}.ix_patterns — the builds counted as dumps`,
    body: dumpField?.description
      ? `${dumpField.description}

${FINGERPRINT_FIELD_HELP.dump_ix_patterns.body}`
      : FINGERPRINT_FIELD_HELP.dump_ix_patterns.body,
  };
  const setFlow = (patch: Partial<FlowClassifier>) =>
    setS((p) => ({ ...p, flow: { ...p.flow, ...patch, configured: true } }));
  const ixParsed = useMemo(() => parseIxLabelsText(s.ix_labels), [s.ix_labels]);
  const { criteria, badAxes } = useMemo(() => toCriteria(s), [s]);
  const draft = useMemo(() => toDraft(s), [s]);
  const autoName = useMemo(() => fingerprintAutoName(draft), [draft]);
  const prevAutoRef = useRef<string | null>(null);

  // `ALL` is the auto-name of two different drafts: a wildcard (which really is
  // named for the token set it matches) and an axis-less one (which has nothing to
  // name yet, and the backend refuses anyway). Only the first is a usable name.
  const autoNameIsReal = autoName !== WILDCARD_NAME || s.wildcard;

  // Keep the name glued to the auto-label while it is blank, still the previous
  // auto-name, or any auto-label that no longer matches the axes (a retired shape —
  // the `bkt=` width chip included — or a current-grammar one written before
  // `fingerprintAutoName` changed). A typed nickname is left alone.
  useEffect(() => {
    setS((p) => {
      const synced =
        p.name === '' ||
        p.name === prevAutoRef.current ||
        p.name === autoName ||
        isStaleAutoName(p.name, autoName);
      if (!synced) return p;
      prevAutoRef.current = autoName;
      if (!autoNameIsReal || p.name === autoName) return p;
      return { ...p, name: autoName };
    });
  }, [autoName, autoNameIsReal]);

  // Mirrors the backend gate, so the form fails fast instead of on a 400.
  const criterionCount = s.wildcard ? 1 : Object.keys(criteria).length;
  const problems = useMemo(
    () => (s.wildcard ? [] : [...badAxes, ...criteriaProblems(criteria)]),
    [s.wildcard, badAxes, criteria],
  );
  const nameOk = s.name.trim().length > 0 || autoNameIsReal;
  const canSubmit =
    criterionCount > 0 && nameOk && !submitting && !ixParsed.error && problems.length === 0;

  const axisRow = (def: AxisDef) => {
    return (
      <div key={def.id} className="flex flex-col gap-1 text-[11px] text-text-dim">
        <LabelTip tip={boundsHelp(def)}>{def.label}</LabelTip>
        <AxisConditionInput
          def={def}
          value={s.conditions[def.id] ?? ''}
          onChange={(v) => setCondition(def.id, v)}
          disabled={s.wildcard}
          title={s.wildcard ? AXIS_DISABLED_TITLE : undefined}
        />
      </div>
    );
  };

  return (
    <div className="flex flex-col gap-3">
      <label className="flex flex-col gap-1 text-[11px] text-text-dim">
        <LabelTip tip={NAME_HELP}>Name</LabelTip>
        <div className="flex items-center gap-1">
          <Input
            fieldSize="sm"
            className="min-w-0 flex-1"
            value={s.name}
            onChange={(e) => set('name', e.target.value)}
            placeholder={autoNameIsReal ? autoName : 'auto-filled from axes'}
          />
          <IconButton
            variant="ghost"
            size="sm"
            disabled={submitting || !autoNameIsReal || s.name === autoName}
            onClick={() => {
              prevAutoRef.current = autoName;
              set('name', autoName);
            }}
            title="Reset to auto-name from axes"
            aria-label="Reset to auto-name from axes"
          >
            <RefreshIcon />
          </IconButton>
        </div>
      </label>

      <label className="flex cursor-pointer items-start gap-1.5 text-[11px] text-text-mid">
        <Checkbox
          className="mt-0.5"
          checked={s.wildcard}
          disabled={submitting}
          onChange={() => set('wildcard', !s.wildcard)}
        />
        <LabelTip tip={WILDCARD_HELP}>match every token (wildcard)</LabelTip>
      </label>

      <div className="grid grid-cols-2 gap-2">{NUMERIC_AXES.map(axisRow)}</div>

      <label className="flex flex-col gap-1 text-[11px] text-text-dim">
        <LabelTip tip={{ title: axisDef('ix_labels').label, body: axisDef('ix_labels').definition }}>
          {axisDef('ix_labels').label} (JSON array)
        </LabelTip>
        <IxLabelsInput
          value={s.ix_labels}
          onValueChange={(v) => set('ix_labels', v)}
          disabled={s.wildcard}
          title={s.wildcard ? AXIS_DISABLED_TITLE : undefined}
          error={s.wildcard ? null : ixParsed.error}
        />
      </label>

      {flowFields.length > 0 && (
        <FlowClassifierEditor
          value={s.flow}
          fields={flowFields}
          markers={ixMarkers(registry)}
          disabled={submitting}
          onChange={setFlow}
        />
      )}

      {dumpField && (
        <div className="flex flex-col gap-1 rounded border border-white/10 p-2 text-[11px] text-text-dim">
          <LabelTip tip={dumpTip}>{DUMP_GROUP} · dump builds</LabelTip>
          <IxPatternRowsEditor
            rows={s.dump_patterns}
            onChange={(p) => set('dump_patterns', p)}
            disabled={submitting}
          />
        </div>
      )}

      {workingField && (
        <WorkingTemplatesEditor
          value={s.working_templates}
          onChange={(v) => set('working_templates', v)}
          field={workingField}
          disabled={submitting}
        />
      )}

      {targetField && (
        <TargetWalletsEditor
          value={s.target_wallets}
          onChange={(v) => set('target_wallets', v)}
          field={targetField}
          disabled={submitting}
        />
      )}

      <div className="flex items-center justify-between gap-2">
        <span className="min-w-0 text-[11px] text-text-dim/80">
          {problems.length > 0 ? (
            <span className="text-red">{problems[0]}</span>
          ) : criterionCount === 0 ? (
            <span className="text-red">needs ≥1 match criterion</span>
          ) : s.wildcard ? (
            'matches EVERY token · no creation-shape axes'
          ) : (
            `${criterionCount} axis${criterionCount === 1 ? '' : 'es'}`
          )}
        </span>
        <div className="flex shrink-0 gap-2">
          {onCancel && (
            <Button variant="ghost" size="sm" onClick={onCancel} disabled={submitting}>
              Cancel
            </Button>
          )}
          <IconButton
            variant="primary"
            size="lg"
            disabled={!canSubmit}
            onClick={() => {
              const body = toDraft(s);
              if (!body.name) body.name = fingerprintAutoName(body);
              onSubmit(body);
            }}
            label={submitting ? 'Saving…' : initial ? 'Save' : 'Create'}
            title={submitting ? 'Saving…' : initial ? 'Save' : 'Create'}
          >
            {submitting ? <SpinnerIcon /> : <SaveIcon />}
          </IconButton>
        </div>
      </div>
      {error && <p className="text-[11px] text-red">{error}</p>}
    </div>
  );
}
