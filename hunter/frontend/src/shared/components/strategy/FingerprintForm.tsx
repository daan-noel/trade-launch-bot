import { useEffect, useMemo, useRef, useState } from 'react';

import { Input } from 'components/ui/Input';
import { IconButton } from 'components/ui/IconButton';
import { CloseIcon, RefreshIcon } from 'components/ui/icons';
import { Button } from 'components/ui/Button';
import { Checkbox } from 'components/ui/Checkbox';
import { IxLabelsInput } from 'components/ui/IxLabelsInput';
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
import { GuideButton } from './StrategyGuide';
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
  /** The fingerprint's tags (named trade lists), written whole on save. */
  tags: TagDef[];
}


const NUMERIC_AXES: readonly AxisDef[] = AXES.filter((a) => a.kind === 'numeric');

function fromFingerprint(fp?: Fingerprint): FormState {
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
    tags: tagsFromJson(fp?.tags),
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
    tags: tagsToJson(s.tags),
  };
}

/** Why every axis input greys out under the wildcard. */
const AXIS_DISABLED_TITLE =
  'A wildcard fingerprint matches every token, so it carries no axes.' +
  '\nUncheck "match every token" to narrow it by creation shape.';

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
      'Type a condition in the box. Empty leaves this axis out of the match.',
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
    'Left blank, or written in the generated grammar, it re-derives from the axes on every edit. A nickname you type is never overwritten: it is the only record of WHY this fingerprint exists, and the axes can always be re-read.',
};

/** What the axes currently select, in the same words the boxes show. */
function identitySentence(s: FormState): string {
  if (s.wildcard) return 'Matches every token. No creation-shape axes.';
  const bits: string[] = [];
  for (const def of NUMERIC_AXES) {
    const state = axisConditionState(s.conditions[def.id] ?? '', def);
    if (state.kind === 'ok') bits.push(`${def.label} ${formatPredicate(def.id, state.predicate)}`);
  }
  const { labels } = parseIxLabelsText(s.ix_labels);
  if (configuredIxLabels(labels)) bits.push('an instruction-label sequence');
  if (bits.length === 0) return 'No axis yet. Add one, or match every token.';
  if (bits.length === 1) return `Matches coins where ${bits[0]}.`;
  return `Matches coins where ${bits.slice(0, -1).join(', ')}, and ${bits[bits.length - 1]}.`;
}

const WILDCARD_HELP: HelpTip = {
  title: 'Match every token',
  body:
    'Arms on EVERY token, ignoring every axis.\n\n' +
    'A rule always needs a fingerprint, but one deciding purely on what the tape is doing has no creation shape to name — and clearing every axis means MATCH NOTHING, because the matcher refuses a criterion-less row on purpose. So "every token" is said out loud here rather than inferred from a blank form.\n\n' +
    'Mutually exclusive with the axes: turning it on clears them.',
};

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
  // Axes stay listed once shown, including one the operator just added and has
  // not typed yet. An axis that was never set is absent: blank is not a zero.
  const [openAxes, setOpenAxes] = useState<AxisId[]>(() => {
    const start = fromFingerprint(initial);
    return NUMERIC_AXES.filter((def) => (start.conditions[def.id] ?? '').trim() !== '').map((def) => def.id);
  });
  const [ixOpen, setIxOpen] = useState(() => fromFingerprint(initial).ix_labels.trim() !== '');
  const set = <K extends keyof FormState>(k: K, v: FormState[K]) => setS((p) => ({ ...p, [k]: v }));
  const setCondition = (id: AxisId, v: string) =>
    setS((p) => ({ ...p, conditions: { ...p.conditions, [id]: v } }));

  const { data: registry } = useStrategyRegistry();
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
  const tagErrors = useMemo(() => validateTags(s.tags, registry), [s.tags, registry]);
  const problems = useMemo(
    () => [...(s.wildcard ? [] : [...badAxes, ...criteriaProblems(criteria)]), ...tagErrors],
    [s.wildcard, badAxes, criteria, tagErrors],
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

  const closedAxes = NUMERIC_AXES.filter((def) => !openAxes.includes(def.id));
  const save = () => {
    const body = toDraft(s);
    if (!body.name) body.name = fingerprintAutoName(body);
    onSubmit(body);
  };

  return (
    <div className="flex flex-col gap-3">
      <div className="grid items-start gap-3 md:grid-cols-[minmax(17rem,24rem)_minmax(0,1fr)]">
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
            <span className="text-[11px] text-text-dim/80">
              Filled from the axes. A name you type stays.
            </span>
          </label>

          <label className="flex cursor-pointer items-start gap-1.5 text-[12px] text-text-mid">
            <Checkbox
              className="mt-0.5"
              checked={s.wildcard}
              disabled={submitting}
              onChange={() => set('wildcard', !s.wildcard)}
            />
            <LabelTip tip={WILDCARD_HELP}>Match every token</LabelTip>
          </label>

          {!s.wildcard && (
            <div className="flex flex-col gap-2">
              <span className="text-[12px] font-semibold text-text">Axes</span>
              {openAxes.length === 0 && !ixOpen && (
                <p className="text-[12px] text-text-dim">No axis yet.</p>
              )}
              {NUMERIC_AXES.filter((def) => openAxes.includes(def.id)).map((def) => (
                <div key={def.id} className="flex items-start gap-1">
                  <div className="min-w-0 flex-1">{axisRow(def)}</div>
                  <IconButton
                    variant="ghost"
                    size="sm"
                    className="mt-5"
                    disabled={submitting}
                    title={`Remove ${def.label}`}
                    aria-label={`Remove ${def.label}`}
                    onClick={() => {
                      setCondition(def.id, '');
                      setOpenAxes((ids) => ids.filter((id) => id !== def.id));
                    }}
                  >
                    <CloseIcon />
                  </IconButton>
                </div>
              ))}
              {ixOpen && (
                <div className="flex items-start gap-1">
                  <label className="flex min-w-0 flex-1 flex-col gap-1 text-[12px] text-text-dim">
                    <LabelTip tip={{ title: axisDef('ix_labels').label, body: axisDef('ix_labels').definition }}>
                      {axisDef('ix_labels').label}
                    </LabelTip>
                    <IxLabelsInput
                      value={s.ix_labels}
                      onValueChange={(v) => set('ix_labels', v)}
                      disabled={submitting}
                      error={ixParsed.error}
                    />
                  </label>
                  <IconButton
                    variant="ghost"
                    size="sm"
                    className="mt-5"
                    disabled={submitting}
                    title="Remove instruction labels"
                    aria-label="Remove instruction labels"
                    onClick={() => {
                      set('ix_labels', '');
                      setIxOpen(false);
                    }}
                  >
                    <CloseIcon />
                  </IconButton>
                </div>
              )}
              {(closedAxes.length > 0 || !ixOpen) && (
                <div className="flex flex-wrap items-center gap-1.5">
                  {closedAxes.map((def) => (
                    <Button
                      key={def.id}
                      variant="ghost"
                      size="xs"
                      disabled={submitting}
                      onClick={() => setOpenAxes((ids) => (ids.includes(def.id) ? ids : [...ids, def.id]))}
                    >
                      + {def.label}
                    </Button>
                  ))}
                  {!ixOpen && (
                    <Button variant="ghost" size="xs" disabled={submitting} onClick={() => setIxOpen(true)}>
                      + {axisDef('ix_labels').label}
                    </Button>
                  )}
                </div>
              )}
            </div>
          )}
          <p className="text-[12px] leading-snug text-text-mid">{identitySentence(s)}</p>
        </section>

        {registry && (
          <section className="flex max-h-[min(64vh,48rem)] flex-col gap-2 overflow-y-auto rounded-lg border border-white/10 bg-white/2 p-3">
            <h3 className="sticky top-0 z-10 -mx-3 -mt-3 border-b border-white/10 bg-bg-panel px-3 py-2 text-[13px] font-semibold text-text">
              <span className="mr-1.5 text-accent">2.</span>Tags: whose trades a rule can read
              <GuideButton className="ml-2" />
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
          ) : criterionCount === 0 ? (
            <span className="text-red">Needs at least one match criterion.</span>
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
