// The fingerprint's tags: one card per tag. A trade carries the tag when ANY of its
// matchers holds (on the tag's side only); rules then read `@name` (the trades with
// it) and `@!name` (the rest). Every matcher and option is explained from the
// registry's one definition.

import { useState } from 'react';

import { Button } from 'components/ui/Button';
import { IconButton } from 'components/ui/IconButton';
import { InfoTooltip } from 'components/ui/InfoTooltip';
import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { CloseIcon, PlusIcon } from 'components/ui/icons';
import { ixMarkers, tagField, type StrategyRegistry, type TagFieldSpec } from 'lib/strategy/registry';
import { freshName } from 'lib/strategy/ruleDoc';
import {
  emptyTag,
  hasTemplateView,
  TAG_NAME_RE,
  usedMatchers,
  type MatcherKey,
  tagSentence,
  type TagDef,
  type TagMatch,
} from 'lib/strategy/tagsDoc';
import { IxPatternRowsEditor } from './IxPatternsEditor';

function fieldTip(f: TagFieldSpec | undefined): string {
  return f ? `${f.summary}\n\nExample: ${f.example}` : '';
}

/** One line per value; blanks dropped. */
function LinesInput({
  value,
  onChange,
  placeholder,
  disabled,
  split = /\n/,
}: {
  value: string[];
  onChange: (v: string[]) => void;
  placeholder: string;
  disabled?: boolean;
  split?: RegExp;
}) {
  const [text, setText] = useState(value.join('\n'));
  return (
    <textarea
      className="h-16 w-full rounded border border-white/10 bg-white/4 p-1.5 font-mono text-[11px] text-text outline-none focus:border-primary/50"
      value={text}
      placeholder={placeholder}
      disabled={disabled}
      spellCheck={false}
      onChange={(e) => {
        setText(e.target.value);
        onChange(
          e.target.value
            .split(split)
            .map((s) => s.trim())
            .filter(Boolean),
        );
      }}
    />
  );
}

function MatcherEditor({
  k,
  match,
  onChange,
  reg,
  disabled,
}: {
  k: MatcherKey;
  match: TagMatch;
  onChange: (m: TagMatch) => void;
  reg: StrategyRegistry;
  disabled?: boolean;
}) {
  switch (k) {
    case 'program':
      return <LinesInput value={match.program ?? []} disabled={disabled} placeholder={'one program per line\nAxiom Trade'} onChange={(program) => onChange({ ...match, program })} />;
    case 'ix_template':
      return <LinesInput value={match.ix_template ?? []} disabled={disabled} placeholder={'one template per line\nAxiom Trade|CU|ATA|1|0|0'} onChange={(ix_template) => onChange({ ...match, ix_template })} />;
    case 'wallet':
      return <LinesInput value={match.wallet ?? []} disabled={disabled} split={/[\s,]+/} placeholder="wallet addresses, one per line" onChange={(wallet) => onChange({ ...match, wallet })} />;
    case 'ix_shape':
      return <IxPatternRowsEditor rows={match.ix_shape ?? []} onChange={(ix_shape) => onChange({ ...match, ix_shape })} disabled={disabled} />;
    case 'ix_contains':
    case 'ix_lacks': {
      const picked = match[k] ?? [];
      return (
        <div className="flex flex-wrap gap-x-3 gap-y-1">
          {ixMarkers(reg).map((mk) => (
            <label key={mk.name} className="flex items-center gap-1 text-[11px] text-text-mid" title={mk.router ? 'a retail router (a person clicked through it)' : 'a mechanism of the transaction'}>
              <input
                type="checkbox"
                className="accent-accent"
                disabled={disabled}
                checked={picked.includes(mk.name)}
                onChange={(e) =>
                  onChange({ ...match, [k]: e.target.checked ? [...picked, mk.name] : picked.filter((x) => x !== mk.name) })
                }
              />
              {mk.name}
              {mk.router && <span className="text-text-dim">(router)</span>}
            </label>
          ))}
        </div>
      );
    }
    case 'creator':
      return <p className="text-[11px] text-text-dim">Every trade by the coin's creator.</p>;
    case 'cluster': {
      const c = match.cluster ?? { min_prints: 3, sol_tol_pct: 10 };
      return (
        <div className="flex flex-wrap items-center gap-2 text-[11px] text-text-dim">
          the
          <Input fieldSize="sm" numeric integer className="w-14" numericValue={c.min_prints} disabled={disabled} onNumericChange={(n) => onChange({ ...match, cluster: { ...c, min_prints: n ?? NaN } })} />
          th or later print in one slot with the same ix shape, side and fees, its SOL within
          <Input fieldSize="sm" numeric integer unit="%" className="w-16" numericValue={c.sol_tol_pct} disabled={disabled} onNumericChange={(n) => onChange({ ...match, cluster: { ...c, sol_tol_pct: n ?? NaN } })} />
          of the first
        </div>
      );
    }
  }
}

function startValue(k: MatcherKey): Partial<TagMatch> {
  switch (k) {
    case 'creator':
      return { creator: true };
    case 'cluster':
      return { cluster: { min_prints: 3, sol_tol_pct: 10 } };
    default:
      return { [k]: [] };
  }
}

function TagCard({
  tag,
  onChange,
  onRemove,
  reg,
  disabled,
}: {
  tag: TagDef;
  onChange: (t: TagDef) => void;
  onRemove: () => void;
  reg: StrategyRegistry;
  disabled?: boolean;
}) {
  // A matcher stays on screen while being filled in, even before it holds a value.
  const shown = (Object.keys(tag.match) as MatcherKey[]).filter((k) => tag.match[k] !== undefined);
  const matchFields = reg.tags.fields.filter((f) => f.kind === 'match');
  const unused = matchFields.filter((f) => !shown.includes(f.key as MatcherKey));
  const nameOk = TAG_NAME_RE.test(tag.name);
  return (
    <div className="flex flex-col gap-2 rounded-md border border-white/10 bg-bg-card p-2">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-[11px] font-semibold text-text-dim">Tag</span>
        <Input fieldSize="sm" className="w-36 font-mono" value={tag.name} disabled={disabled} onChange={(e) => onChange({ ...tag, name: e.target.value })} />
        <span className="text-[11px] text-text-dim">
          rules read <code>@{tag.name}</code> (trades with it) and <code>@!{tag.name}</code> (the rest)
        </span>
        <IconButton className="ml-auto" variant="ghost" size="sm" disabled={disabled} onClick={onRemove} title="Remove tag" aria-label="Remove tag">
          <CloseIcon />
        </IconButton>
      </div>
      {!nameOk && <p className="text-[11px] text-red">A tag name is 1 to 24 characters of a-z, 0-9 and _.</p>}

      <span className="text-[11px] font-semibold text-text">A trade has this tag if ANY of these holds:</span>
      {shown.length === 0 && <p className="text-[11px] italic text-text-dim/70">No matcher yet: no trade can carry this tag.</p>}
      {shown.map((k, i) => {
        const f = tagField(reg, k);
        return (
          <div key={k} className="flex flex-col gap-1 rounded border border-white/5 p-1.5">
            {i > 0 && <span className="text-[10px] font-semibold uppercase tracking-wide text-accent">or</span>}
            <div className="flex items-center gap-1 text-[11px] text-text">
              <span className="font-semibold">{f?.title ?? k}</span>
              {f && <InfoTooltip title={f.title} body={fieldTip(f)} />}
              <span className="min-w-0 flex-1 truncate text-text-dim">{f?.summary}</span>
              <IconButton
                variant="ghost"
                size="sm"
                disabled={disabled}
                title="Remove this matcher"
                aria-label="Remove matcher"
                onClick={() => {
                  const next = { ...tag.match };
                  delete next[k];
                  onChange({ ...tag, match: next });
                }}
              >
                <CloseIcon />
              </IconButton>
            </div>
            <MatcherEditor k={k} match={tag.match} reg={reg} disabled={disabled} onChange={(match) => onChange({ ...tag, match })} />
          </div>
        );
      })}
      {unused.length > 0 && (
        <Select
          className="w-56"
          value=""
          disabled={disabled}
          onChange={(e) => {
            const k = e.target.value as MatcherKey;
            if (k) onChange({ ...tag, match: { ...tag.match, ...startValue(k) } });
          }}
        >
          <option value="">+ add a matcher...</option>
          {unused.map((f) => (
            <option key={f.key} value={f.key} title={f.summary}>
              {f.title}: {f.summary}
            </option>
          ))}
        </Select>
      )}

      <div className="flex flex-wrap items-center gap-3 border-t border-white/5 pt-1.5 text-[11px] text-text-dim">
        <label className="flex items-center gap-1">
          {tagField(reg, 'side')?.title ?? 'Side'}
          <InfoTooltip body={fieldTip(tagField(reg, 'side'))} />
          <Select className="w-28" value={tag.side ?? ''} disabled={disabled} onChange={(e) => onChange({ ...tag, side: (e.target.value || null) as TagDef['side'] })}>
            <option value="">buys and sells</option>
            <option value="buy">buys only</option>
            <option value="sell">sells only</option>
          </Select>
        </label>
        {(['sticky', 'exclude_creation_slot'] as const).map((k) => (
          <label key={k} className="flex items-center gap-1">
            <input type="checkbox" className="accent-accent" disabled={disabled} checked={tag[k]} onChange={(e) => onChange({ ...tag, [k]: e.target.checked })} />
            {tagField(reg, k)?.title ?? k}
            <InfoTooltip body={fieldTip(tagField(reg, k))} />
          </label>
        ))}
      </div>
      <p className="text-[11px] text-text-dim/80">{tagSentence(tag)}</p>
      {usedMatchers(tag).length > 0 && !hasTemplateView(tag) && (
        <p className="text-[11px] text-text-dim/60">
          Slot and wave metrics (m_slot, m_wave, m_crowd.unique_ix_templates) read a tag through its Program and Ix template matchers only; this tag has neither, so those read nothing for it.
        </p>
      )}
    </div>
  );
}

export function TagsEditor({
  tags,
  onChange,
  reg,
  disabled,
}: {
  tags: TagDef[];
  onChange: (t: TagDef[]) => void;
  reg: StrategyRegistry;
  disabled?: boolean;
}) {
  const set = (i: number, t: TagDef) => onChange(tags.map((x, j) => (j === i ? t : x)));
  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap items-start gap-2">
        <div className="flex min-w-0 flex-1 flex-col">
          <span className="inline-flex items-center gap-1 text-[12px] font-semibold text-text">
            Tags
            <InfoTooltip title="Tags" body={`${reg.tags.summary}\n\nExample: ${reg.tags.example}`} />
          </span>
          <span className="text-[11px] text-text-dim">{reg.tags.summary}</span>
        </div>
        <Button variant="subtle" size="xs" disabled={disabled} onClick={() => onChange([...tags, emptyTag(freshName(tags.length ? 'tag' : 'volume', tags.map((t) => t.name)))])}>
          <PlusIcon className="size-3" /> tag
        </Button>
      </div>
      {tags.length === 0 && (
        <p className="text-[11px] italic text-text-dim/70">
          No tags: metrics that need a tag (holdings, transaction counts, slot and wave tag reads) read nothing on this fingerprint.
        </p>
      )}
      {tags.map((t, i) => (
        <TagCard key={t.id} tag={t} reg={reg} disabled={disabled} onChange={(n) => set(i, n)} onRemove={() => onChange(tags.filter((_, j) => j !== i))} />
      ))}
      <p className="text-[11px] text-text-dim/60">
        Built-in wallet classes, no setup needed: {reg.tags.builtin.map((b) => `@${b.name} (${b.summary})`).join(' ')}
      </p>
    </div>
  );
}
