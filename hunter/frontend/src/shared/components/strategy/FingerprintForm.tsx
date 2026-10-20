import { useEffect, useMemo, useRef, useState } from 'react';

import { Input } from 'components/ui/Input';
import { IconButton } from 'components/ui/IconButton';
import { CloseIcon, RefreshIcon } from 'components/ui/icons';
import { Button } from 'components/ui/Button';
import { IxLabelsInput } from 'components/ui/IxLabelsInput';
import { Select } from 'components/ui/Select';
import { ToggleGroup } from 'components/ui/ToggleGroup';
import { configuredIxLabels, formatIxLabelsText, parseIxLabelsText } from 'lib/ixLabels';
import { WILDCARD_NAME, type Fingerprint, type FingerprintDraft } from 'lib/strategy/types';
import {
  AXES,
  axisDef,
  criteriaProblems,
  formatPredicate,
  type AxisDef,
  type AxisId,
  type Criteria,
} from 'lib/strategy/fingerprintAxes';
import { axisPredicateText } from 'lib/strategy/fingerprintGrammar';
import { useStrategyRegistry } from 'lib/strategy/registry';
import { tagsFromJson, tagsToJson, validateTags, type TagDef } from 'lib/strategy/tagsDoc';
import { fingerprintAutoName, isStaleAutoName } from 'lib/strategy/fingerprintNameFromGroupKey';
import type { HelpTip } from 'lib/strategy/strategyHelp';
import { LabelTip } from './LabelTip';
import { TagsEditor } from './TagsEditor';
import { VolumeGuideButton } from './VolumeGuide';
import {
  AxisConditionInput,
  axisConditionProblem,
  axisConditionState,
} from './AxisConditionInput';

export interface FingerprintFormProps {
  /** Existing fingerprint to edit; omit to create. */
  initial?: Fingerprint;
  /** Saved fingerprints a shape can be copied from ("copy from fingerprint"). */
  library?: readonly Fingerprint[];
  onSubmit: (draft: FingerprintDraft) => void;
  onCancel?: () => void;
  submitting?: boolean;
  error?: string | null;
}

/** One shape as the operator is editing it: every listed axis must hold. */
interface ShapeState {
  /** React key — stable across edits, never sent. */
  key: number;
  /** Per-axis condition expression, as the operator typed it, in the axis's own
   *  display unit. Kept as raw text so a half-typed `1.` is not silently rounded
   *  mid-keystroke and so an unparseable expression can be *shown* as an error
   *  rather than dropped — a dropped axis reads as "unconstrained", which widens
   *  the match. */
  conditions: Partial<Record<AxisId, string>>;
  /** Textarea text — pretty JSON string array (see `parseIxLabelsText`). */
  ix_labels: string;
  /** Axes listed in the card, including one just added and not typed yet. An axis
   *  never set is absent: blank is not a zero. */
  openAxes: AxisId[];
  ixOpen: boolean;
}

/** Which tokens the include side admits: every token, or only the shapes. */
type MatchMode = 'all' | 'shapes';

interface FormState {
  name: string;
  /** `all` is the wildcard. Exclusive with include shapes (backend `validate` + the
   *  `fingerprints_wildcard_excludes_axes` CHECK), so the include list is hidden and
   *  left out of the draft rather than leaving a contradiction on screen. Exclude
   *  shapes apply in both modes. */
  mode: MatchMode;
  include: ShapeState[];
  exclude: ShapeState[];
  /** The fingerprint's tags (named trade lists), written whole on save. */
  tags: TagDef[];
}

const NUMERIC_AXES: readonly AxisDef[] = AXES.filter((a) => a.kind === 'numeric');

let nextShapeKey = 1;

function shapeFromCriteria(criteria: Criteria = {}): ShapeState {
  const conditions: Partial<Record<AxisId, string>> = {};
  for (const def of NUMERIC_AXES) {
    conditions[def.id] = axisPredicateText(def.id, criteria[def.id]);
  }
  const labels = criteria.ix_labels;
  const ix_labels = formatIxLabelsText(labels?.kind === 'sequence' ? labels.labels : null);
  return {
    key: nextShapeKey++,
    conditions,
    ix_labels,
    openAxes: NUMERIC_AXES.filter((def) => (conditions[def.id] ?? '').trim() !== '').map((def) => def.id),
    ixOpen: ix_labels.trim() !== '',
  };
}

function fromFingerprint(fp?: Fingerprint): FormState {
  const include = fp?.wildcard ? [] : [fp?.criteria ?? {}, ...(fp?.alt_criteria ?? [])];
  return {
    name: fp?.name ?? '',
    mode: fp?.wildcard ? 'all' : 'shapes',
    include: (include.length > 0 ? include : [{}]).map(shapeFromCriteria),
    exclude: (fp?.exclude_criteria ?? []).map(shapeFromCriteria),
    tags: tagsFromJson(fp?.tags),
  };
}

/** The criteria a shape currently configures, plus every axis whose expression
 *  does not say something storable.
 *
 *  An unreadable expression is reported, never dropped: dropping it leaves the axis
 *  unconfigured, which matches MORE tokens than the operator asked for — the silent
 *  direction. Read through `axisConditionState`, the same interpreter the input
 *  renders its chips from, so the form can never save something other than what it
 *  shows. */
function shapeToCriteria(sh: ShapeState): { criteria: Criteria; badAxes: string[] } {
  const criteria: Criteria = {};
  const badAxes: string[] = [];
  for (const def of NUMERIC_AXES) {
    const state = axisConditionState(sh.conditions[def.id] ?? '', def);
    if (state.kind === 'ok') {
      criteria[def.id] = state.predicate;
      continue;
    }
    // Blank ⇒ the axis is not part of the shape. There is exactly one spelling of
    // that: absent from the map.
    const problem = axisConditionProblem(state, def);
    if (problem) badAxes.push(problem);
  }
  const { labels } = parseIxLabelsText(sh.ix_labels);
  const configured = configuredIxLabels(labels);
  if (configured) criteria.ix_labels = { kind: 'sequence', labels: configured };
  return { criteria, badAxes };
}

/** An include shape's letter (`A`, `B`, ...) and an exclude shape's (`X1`, `X2`,
 *  ...). Mirrors the Rust `shape_label` the backend's problems name. */
const includeLabel = (i: number) => (i < 26 ? String.fromCharCode(65 + i) : `#${i + 1}`);
const excludeLabel = (i: number) => `X${i + 1}`;

function toDraft(s: FormState): FingerprintDraft {
  const wildcard = s.mode === 'all';
  const include = wildcard ? [] : s.include.map((sh) => shapeToCriteria(sh).criteria);
  return {
    name: s.name.trim(),
    wildcard,
    criteria: include[0] ?? {},
    alt_criteria: include.slice(1),
    exclude_criteria: s.exclude.map((sh) => shapeToCriteria(sh).criteria),
    tags: tagsToJson(s.tags),
  };
}

/** Every reason the shapes cannot be saved, named by shape — mirrors the backend's
 *  `Fingerprint::problems`, so the form fails fast instead of on a 400. */
function shapeProblems(s: FormState): string[] {
  const out: string[] = [];
  const check = (sh: ShapeState, label: string) => {
    const { criteria, badAxes } = shapeToCriteria(sh);
    const { error } = parseIxLabelsText(sh.ix_labels);
    if (Object.keys(criteria).length === 0 && badAxes.length === 0 && !error) {
      out.push(`${label}: no axis yet. Give it one or remove it.`);
    }
    out.push(...[...badAxes, ...criteriaProblems(criteria)].map((p) => `${label}: ${p}`));
    if (error) out.push(`${label}: ${error}`);
  };
  if (s.mode === 'shapes') {
    if (s.include.length === 0) out.push('Add an include shape, or match every token.');
    s.include.forEach((sh, i) => check(sh, `Include ${includeLabel(i)}`));
  }
  s.exclude.forEach((sh, i) => check(sh, `Exclude ${excludeLabel(i)}`));
  return out;
}

/** How an axis box is typed, once for every axis tooltip.
 *
 *  The box is one condition string. The examples live in `figure` so they stay
 *  aligned; the body says what each mark means. */
function boundsHelp(def: AxisDef): HelpTip {
  const sol = def.unit === 'lamports';
  const [lo, hi] = sol ? ['1.5', '2'] : ['3', '5'];
  const unit = sol ? ' SOL' : '';
  const row = (typed: string, means: string) => `${typed.padEnd(14)}${means}`;
  return {
    title: def.label,
    // The axis's ONE definition, rendered from the registry — never a second copy
    // that can say something the matcher does not do.
    body: [
      def.definition,
      '',
      'Type a condition in the box. Empty leaves this axis out of the shape.',
      sol ? 'Type SOL.' : 'Type a whole number.',
      'A number alone is exact. = and == mean the same.',
      '.. includes both ends. - stops before the high number, which is how a group chip is written, so pasting a chip selects that chip\'s coins.',
      'A comma means both parts. A pipe means either part.',
      'Leaving the box rewrites the text to the spelling that is saved.',
      '',
      def.phase === 'first_slot'
        ? 'This value exists only after the creation slot closes, so a rule that uses it cannot fire at birth.'
        : 'Known as soon as the coin is created.',
    ].join('\n'),
    figure: [
      row(lo, `exactly ${lo}${unit}`),
      row(`${lo}..${hi}`, `${lo} to ${hi}${unit}, both ends in`),
      row(`${lo}-${hi}`, `${lo} up to, not including, ${hi}${unit}`),
      row(`>=${lo}`, `${lo}${unit} or more (also >, <, <=)`),
      row(`!=${lo}`, `anything but ${lo}${unit}`),
      row(`<=${lo} | >=${hi}`, `${lo}${unit} or less, or ${hi}${unit} or more`),
    ].join('\n'),
  };
}

const NAME_HELP: HelpTip = {
  title: 'Name',
  body:
    'Picker handle and log label. NOT identity — renaming changes nothing about what this fingerprint matches.\n\n' +
    'Left blank, or written in the generated grammar, it re-derives from the shapes on every edit. A nickname you type is never overwritten: it is the only record of WHY this fingerprint exists, and the shapes can always be re-read.',
};

const MATCH_HELP: HelpTip = {
  title: 'Match',
  body:
    'All tokens: arms on EVERY token, ignoring every axis. A rule deciding purely on what the tape is doing has no creation shape to name, and an empty shape means MATCH NOTHING, so "every token" is said out loud here.\n\n' +
    'Only these shapes: a token passes when it matches ANY include shape. Every axis inside one shape must hold.\n\n' +
    'Exclude shapes apply in both modes: a token matching ANY of them is skipped. A token whose value on an exclude axis is unknown is not skipped.',
};

const COPY_HELP =
  'Pastes a COPY of that fingerprint\'s include shapes. Editing that fingerprint later never changes this one.';

/** One shape in the words the boxes show, or `null` when it configures nothing. */
function shapeSentence(sh: ShapeState): string | null {
  const bits: string[] = [];
  for (const def of NUMERIC_AXES) {
    const state = axisConditionState(sh.conditions[def.id] ?? '', def);
    if (state.kind === 'ok') bits.push(`${def.label} ${formatPredicate(def.id, state.predicate)}`);
  }
  const { labels } = parseIxLabelsText(sh.ix_labels);
  if (configuredIxLabels(labels)) bits.push('an instruction-label sequence');
  return bits.length === 0 ? null : bits.join(', ');
}

/** What the form currently selects, in the same words the boxes show. */
function identitySentence(s: FormState): string {
  const side = (shapes: ShapeState[], label: (i: number) => string) =>
    shapes
      .map((sh, i) => {
        const t = shapeSentence(sh);
        return t == null ? null : shapes.length === 1 ? t : `(${label(i)}) ${t}`;
      })
      .filter((t): t is string => t != null);
  const include = s.mode === 'all' ? null : side(s.include, includeLabel);
  if (include != null && include.length === 0) return 'No axis yet. Add one, or match every token.';
  const head = include == null ? 'Matches every token' : `Matches coins where ${include.join(' OR ')}`;
  const exclude = side(s.exclude, excludeLabel);
  return exclude.length === 0 ? `${head}.` : `${head}, except where ${exclude.join(' OR ')}.`;
}

/**
 * One shape card: the axes that must all hold, an add-axis row, and a remove
 * button. Each numeric axis is ONE condition expression in its own display unit,
 * converted to the integer identity carries only at submit.
 */
function ShapeCard({
  label,
  shape,
  onChange,
  onRemove,
  disabled,
}: {
  label: string;
  shape: ShapeState;
  onChange: (next: ShapeState) => void;
  onRemove: () => void;
  disabled?: boolean;
}) {
  const ixParsed = useMemo(() => parseIxLabelsText(shape.ix_labels), [shape.ix_labels]);
  const setCondition = (id: AxisId, v: string) =>
    onChange({ ...shape, conditions: { ...shape.conditions, [id]: v } });
  const closedAxes = NUMERIC_AXES.filter((def) => !shape.openAxes.includes(def.id));
  const removeAxisButton = (title: string, onClick: () => void) => (
    <IconButton variant="ghost" size="sm" className="mt-5" disabled={disabled} title={title} aria-label={title} onClick={onClick}>
      <CloseIcon />
    </IconButton>
  );

  return (
    <div className="flex flex-col gap-2 rounded-md border border-white/10 bg-white/3 p-2.5">
      <div className="flex items-center justify-between gap-2">
        <span className="text-[12px] font-semibold text-text">Shape {label}</span>
        <Button variant="ghost" size="xs" disabled={disabled} onClick={onRemove} title={`Remove shape ${label}`}>
          Remove shape
        </Button>
      </div>
      {shape.openAxes.length === 0 && !shape.ixOpen && (
        <p className="text-[12px] text-text-dim">No axis yet.</p>
      )}
      <div className="grid gap-x-3 gap-y-2 sm:grid-cols-2">
        {NUMERIC_AXES.filter((def) => shape.openAxes.includes(def.id)).map((def) => (
          <div key={def.id} className="flex items-start gap-1">
            <div className="flex min-w-0 flex-1 flex-col gap-1 text-[11px] text-text-dim">
              <LabelTip tip={boundsHelp(def)}>{def.label}</LabelTip>
              <AxisConditionInput
                def={def}
                value={shape.conditions[def.id] ?? ''}
                onChange={(v) => setCondition(def.id, v)}
                disabled={disabled}
              />
            </div>
            {removeAxisButton(`Remove ${def.label}`, () =>
              onChange({
                ...shape,
                conditions: { ...shape.conditions, [def.id]: '' },
                openAxes: shape.openAxes.filter((id) => id !== def.id),
              }),
            )}
          </div>
        ))}
      </div>
      {shape.ixOpen && (
        <div className="flex items-start gap-1">
          <label className="flex min-w-0 flex-1 flex-col gap-1 text-[12px] text-text-dim">
            <LabelTip tip={{ title: axisDef('ix_labels').label, body: axisDef('ix_labels').definition }}>
              {axisDef('ix_labels').label}
            </LabelTip>
            <IxLabelsInput
              value={shape.ix_labels}
              onValueChange={(v) => onChange({ ...shape, ix_labels: v })}
              disabled={disabled}
              error={ixParsed.error}
            />
          </label>
          {removeAxisButton('Remove instruction labels', () => onChange({ ...shape, ix_labels: '', ixOpen: false }))}
        </div>
      )}
      {(closedAxes.length > 0 || !shape.ixOpen) && (
        <div className="flex flex-wrap items-center gap-1.5">
          {closedAxes.map((def) => (
            <Button
              key={def.id}
              variant="ghost"
              size="xs"
              disabled={disabled}
              onClick={() => onChange({ ...shape, openAxes: [...shape.openAxes, def.id] })}
            >
              + {def.label}
            </Button>
          ))}
          {!shape.ixOpen && (
            <Button variant="ghost" size="xs" disabled={disabled} onClick={() => onChange({ ...shape, ixOpen: true })}>
              + {axisDef('ix_labels').label}
            </Button>
          )}
        </div>
      )}
    </div>
  );
}

/** An Include or Exclude list: its cards, then "+ add shape" and "copy from". */
function ShapeList({
  title,
  hint,
  tone,
  shapes,
  label,
  onChange,
  library,
  disabled,
}: {
  title: string;
  hint: string;
  tone: 'include' | 'exclude';
  shapes: ShapeState[];
  label: (i: number) => string;
  onChange: (next: ShapeState[]) => void;
  library: readonly Fingerprint[];
  disabled?: boolean;
}) {
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-baseline gap-2">
        <span className={tone === 'include' ? 'text-[12px] font-semibold text-green' : 'text-[12px] font-semibold text-red'}>
          {title}
        </span>
        <span className="text-[11px] text-text-dim">{hint}</span>
      </div>
      {shapes.length === 0 && <p className="text-[12px] text-text-dim">None.</p>}
      {shapes.map((sh, i) => (
        <ShapeCard
          key={sh.key}
          label={label(i)}
          shape={sh}
          disabled={disabled}
          onChange={(next) => onChange(shapes.map((x) => (x.key === sh.key ? next : x)))}
          onRemove={() => onChange(shapes.filter((x) => x.key !== sh.key))}
        />
      ))}
      <div className="flex flex-wrap items-center gap-2">
        <Button variant="ghost" size="xs" disabled={disabled} onClick={() => onChange([...shapes, shapeFromCriteria()])}>
          + add shape
        </Button>
        {library.length > 0 && (
          <Select
            className="w-auto max-w-[18rem]"
            value=""
            disabled={disabled}
            title={COPY_HELP}
            aria-label={`Copy shapes into ${title}`}
            onChange={(e) => {
              const src = library.find((f) => f.id === e.target.value);
              if (!src) return;
              const copied = [src.criteria ?? {}, ...(src.alt_criteria ?? [])].map(shapeFromCriteria);
              onChange([...shapes, ...copied]);
            }}
          >
            <option value="">+ copy from fingerprint…</option>
            {library.map((f) => (
              <option key={f.id} value={f.id}>
                {f.name || fingerprintAutoName(f)}
              </option>
            ))}
          </Select>
        )}
      </div>
    </div>
  );
}

/**
 * Create / edit a fingerprint.
 *
 * Left: which coins — the match mode, the include shapes (a token passes on ANY)
 * and the exclude shapes (a token matching ANY is skipped). Right: the tags. Blocks
 * submit until the draft would pass the backend's own gate, so the form never
 * discovers a rejection as a 400.
 */
export function FingerprintForm({
  initial,
  library = [],
  onSubmit,
  onCancel,
  submitting,
  error,
}: FingerprintFormProps) {
  const [s, setS] = useState<FormState>(() => fromFingerprint(initial));
  const set = <K extends keyof FormState>(k: K, v: FormState[K]) => setS((p) => ({ ...p, [k]: v }));

  const { data: registry } = useStrategyRegistry();
  const draft = useMemo(() => toDraft(s), [s]);
  const autoName = useMemo(() => fingerprintAutoName(draft), [draft]);
  const prevAutoRef = useRef<string | null>(null);
  // A copy source must have an include shape to copy, and is never this row itself.
  const copySources = useMemo(
    () => library.filter((f) => !f.wildcard && f.id !== initial?.id && Object.keys(f.criteria ?? {}).length > 0),
    [library, initial?.id],
  );

  // `ALL` is the auto-name of two different drafts: a wildcard (which really is
  // named for the token set it matches) and an axis-less one (which has nothing to
  // name yet, and the backend refuses anyway). Only the first is a usable name.
  const autoNameIsReal = autoName !== WILDCARD_NAME || s.mode === 'all';

  // Keep the name glued to the auto-label while it is blank, still the previous
  // auto-name, or any auto-label that no longer matches the shapes (a retired shape —
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

  const tagErrors = useMemo(() => validateTags(s.tags, registry), [s.tags, registry]);
  const problems = useMemo(() => [...shapeProblems(s), ...tagErrors], [s, tagErrors]);
  const nameOk = s.name.trim().length > 0 || autoNameIsReal;
  const canSubmit = nameOk && !submitting && problems.length === 0;

  const save = () => {
    const body = toDraft(s);
    if (!body.name) body.name = fingerprintAutoName(body);
    onSubmit(body);
  };

  return (
    <div className="flex flex-col gap-3">
      <div className="grid items-start gap-3 lg:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
        <section className="flex max-h-[min(64vh,48rem)] flex-col gap-3 overflow-y-auto rounded-lg border border-white/10 bg-white/2 p-3">
          <h3 className="text-[13px] font-semibold text-text">
            <span className="mr-1.5 text-accent">1.</span>Which coins
          </h3>
          <label className="flex flex-col gap-1 text-[12px] text-text-dim">
            <LabelTip tip={NAME_HELP}>Name</LabelTip>
            <div className="flex items-center gap-1">
              <Input
                fieldSize="sm"
                className="min-w-0 flex-1"
                value={s.name}
                onChange={(e) => set('name', e.target.value)}
                placeholder={autoNameIsReal ? autoName : 'auto-filled from shapes'}
              />
              <IconButton
                variant="ghost"
                size="sm"
                disabled={submitting || !autoNameIsReal || s.name === autoName}
                onClick={() => {
                  prevAutoRef.current = autoName;
                  set('name', autoName);
                }}
                title="Reset to auto-name from shapes"
                aria-label="Reset to auto-name from shapes"
              >
                <RefreshIcon />
              </IconButton>
            </div>
            <span className="text-[11px] text-text-dim/80">
              Filled from the shapes. A name you type stays.
            </span>
          </label>

          <div className="flex items-center gap-2 text-[12px] text-text-dim">
            <LabelTip tip={MATCH_HELP}>Match</LabelTip>
            <ToggleGroup<MatchMode>
              aria-label="Match"
              size="sm"
              value={s.mode}
              onChange={(m) => set('mode', m)}
              options={[
                { value: 'all', label: 'All tokens' },
                { value: 'shapes', label: 'Only these shapes' },
              ]}
            />
          </div>

          {s.mode === 'shapes' && (
            <ShapeList
              title="Include"
              hint="a token matching ANY shape passes"
              tone="include"
              shapes={s.include}
              label={includeLabel}
              onChange={(v) => set('include', v)}
              library={copySources}
              disabled={submitting}
            />
          )}
          <ShapeList
            title="Exclude"
            hint="a token matching ANY shape is skipped"
            tone="exclude"
            shapes={s.exclude}
            label={excludeLabel}
            onChange={(v) => set('exclude', v)}
            library={copySources}
            disabled={submitting}
          />
          <p className="text-[12px] leading-snug text-text-mid">{identitySentence(s)}</p>
        </section>

        {registry && (
          <section className="flex max-h-[min(64vh,48rem)] flex-col gap-2 overflow-y-auto rounded-lg border border-white/10 bg-white/2 p-3">
            <h3 className="sticky top-0 z-10 -mx-3 -mt-3 border-b border-white/10 bg-bg-panel px-3 py-2 text-[13px] font-semibold text-text">
              <span className="mr-1.5 text-accent">2.</span>Tags: whose trades a rule can read
              <VolumeGuideButton className="ml-2" />
            </h3>
            <TagsEditor tags={s.tags} onChange={(t) => set('tags', t)} reg={registry} disabled={submitting} />
          </section>
        )}
      </div>

      <div className="flex items-start justify-between gap-3 border-t border-white/10 pt-3">
        <div className="flex min-w-0 flex-1 flex-col gap-0.5 text-[12px]">
          {problems.length > 0 ? (
            problems.map((p) => (
              <span key={p} className="text-red">
                {p}
              </span>
            ))
          ) : (
            <span className="text-text-dim">Ready to save.</span>
          )}
        </div>
        <div className="flex shrink-0 gap-2">
          {onCancel && (
            <Button variant="ghost" size="md" onClick={onCancel} disabled={submitting}>
              Cancel
            </Button>
          )}
          <Button variant="primary" size="md" disabled={!canSubmit} onClick={save}>
            {submitting ? 'Saving…' : initial ? 'Save' : 'Create'}
          </Button>
        </div>
      </div>
      {error && <p className="text-[12px] text-red">{error}</p>}
    </div>
  );
}
