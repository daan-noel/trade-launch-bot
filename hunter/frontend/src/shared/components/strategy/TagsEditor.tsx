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
import { cn } from 'lib/cn';
import { rowPinsFee } from 'lib/strategy/ixPatternRows';
import { ixMarkers, tagField, type StrategyRegistry, type TagFieldSpec } from 'lib/strategy/registry';
import { freshName } from 'lib/strategy/ruleDoc';
import {
  emptyTag,
  feePinWarning,
  hasTemplateView,
  TAG_NAME_RE,
  unclassifiableTagError,
  usedMatchers,
  type MatcherKey,
  tagSentence,
  type TagDef,
  type TagMatch,
} from 'lib/strategy/tagsDoc';
import { IxPatternRowsEditor } from './IxPatternsEditor';

const SIDES: { value: TagDef['side']; label: string }[] = [
  { value: null, label: 'Both' },
  { value: 'buy', label: 'Buys' },
  { value: 'sell', label: 'Sells' },
];

/** What the tag tab says before the card is open. */
function tagTabStatus(tag: TagDef): { tone: 'bad' | 'warn' | 'ok'; label: string } {
  if (!TAG_NAME_RE.test(tag.name)) return { tone: 'bad', label: 'bad name' };
  if (unclassifiableTagError(tag)) {
    const shapes = tag.match.ix_shape ?? [];
    const named = shapes.some((r) => r.labels.some((l) => l.trim()));
    return { tone: 'bad', label: shapes.length > 0 && !named ? 'needs a label' : 'needs a matcher' };
  }
  if ((tag.match.ix_shape ?? []).some(rowPinsFee)) return { tone: 'warn', label: 'fee pin' };
  return { tone: 'ok', label: 'ready' };
}

const TAB_DOT = { bad: 'bg-red', warn: 'bg-warning', ok: 'bg-green' } as const;

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
  const bare = unclassifiableTagError(tag);
  const matchFields = reg.tags.fields.filter((f) => f.kind === 'match');
  const unused = matchFields.filter((f) => !shown.includes(f.key as MatcherKey));
  const nameOk = TAG_NAME_RE.test(tag.name);
  const pinWarning = feePinWarning([tag]);
  return (
    <div className="flex flex-col gap-2 rounded-md border border-white/10 bg-bg-card p-3">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-[12px] font-semibold text-text-dim">Name</span>
        <Input fieldSize="sm" className="w-48 font-mono" value={tag.name} disabled={disabled} onChange={(e) => onChange({ ...tag, name: e.target.value })} />
        <span className="text-[12px] text-text-mid">
          rules read <code className="text-text">@{tag.name || '…'}</code> and <code className="text-text">@!{tag.name || '…'}</code>
        </span>
        <IconButton className="ml-auto" variant="ghost" size="sm" disabled={disabled} onClick={onRemove} title="Remove tag" aria-label="Remove tag">
          <CloseIcon />
        </IconButton>
      </div>
      {!nameOk && <p className="text-[12px] text-red">A tag name is 1 to 24 characters of a-z, 0-9 and _.</p>}

      <span className="text-[11px] font-semibold text-text">A trade has this tag if ANY of these holds:</span>
      {shown.length === 0 && <p className="text-[11px] italic text-text-dim/70">No matcher yet: no trade can carry this tag.</p>}
      {bare && shown.length > 0 && <p className="text-[11px] text-red">{bare}</p>}
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
            {k === 'ix_shape' && pinWarning && (
              <p className="rounded-md border border-warning/40 bg-warning/10 px-2.5 py-2 text-[12px] leading-snug text-warning">
                {pinWarning}
              </p>
            )}
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

      <div className="flex flex-col gap-2 border-t border-white/5 pt-2 text-[12px] text-text-dim">
        <div className="flex flex-wrap items-center gap-2">
          <span className="inline-flex items-center gap-1">
            {tagField(reg, 'side')?.title ?? 'Side'}
            <InfoTooltip body={fieldTip(tagField(reg, 'side'))} />
          </span>
          {SIDES.map((opt) => (
            <Button
              key={opt.label}
              variant="ghost"
              size="xs"
              active={(tag.side ?? null) === opt.value}
              disabled={disabled}
              onClick={() => onChange({ ...tag, side: opt.value })}
            >
              {opt.label}
            </Button>
          ))}
        </div>
        <label className="flex items-start gap-1.5">
          <input
            type="checkbox"
            className="mt-0.5 accent-accent"
            disabled={disabled}
            checked={tag.sticky}
            onChange={(e) => onChange({ ...tag, sticky: e.target.checked })}
          />
          <span>
            <span className="inline-flex items-center gap-1 text-text-mid">
              {tagField(reg, 'sticky')?.title ?? 'Sticky'}
              <InfoTooltip body={fieldTip(tagField(reg, 'sticky'))} />
            </span>
            <span className="mt-0.5 block text-text-dim">{tagField(reg, 'sticky')?.summary}</span>
          </span>
        </label>
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
  const [activeId, setActiveId] = useState<string | null>(null);
  const active = tags.find((t) => t.id === activeId) ?? tags[0] ?? null;
  const activeIndex = active ? tags.findIndex((t) => t.id === active.id) : -1;
  const add = () => {
    const next = emptyTag(freshName(tags.length ? 'tag' : 'volume', tags.map((t) => t.name)));
    onChange([...tags, next]);
    setActiveId(next.id);
  };
  return (
    <div className="flex flex-col gap-2">
      <p className="text-[12px] leading-snug text-text-dim">
        {reg.tags.summary}
        <InfoTooltip className="ml-1" title="Tags" body={`${reg.tags.summary}\n\nExample: ${reg.tags.example}`} />
      </p>
      <div className="flex items-end gap-1 overflow-x-auto">
        {tags.map((t) => {
          const status = tagTabStatus(t);
          const on = t.id === active?.id;
          return (
            <button
              key={t.id}
              type="button"
              aria-pressed={on}
              onClick={() => setActiveId(t.id)}
              className={cn(
                'flex shrink-0 flex-col items-start rounded-md border px-2.5 py-1 text-left',
                on ? 'border-accent bg-accent/10' : 'border-white/10 hover:border-white/20',
              )}
            >
              <span className="flex items-center gap-1.5 font-mono text-[12px] text-text">
                <span className={cn('size-1.5 rounded-full', TAB_DOT[status.tone])} />
                {t.name || '(no name)'}
              </span>
              <span className={cn('text-[11px]', status.tone === 'bad' ? 'text-red' : status.tone === 'warn' ? 'text-warning' : 'text-text-dim')}>
                {status.label}
              </span>
            </button>
          );
        })}
        <Button variant="subtle" size="xs" className="mb-1 shrink-0" disabled={disabled} onClick={add}>
          <PlusIcon className="size-3" /> tag
        </Button>
      </div>
      {tags.length === 0 && (
        <p className="text-[12px] italic text-text-dim/70">
          No tags: metrics that need a tag (holdings, transaction counts, slot and wave tag reads) read nothing on this fingerprint.
        </p>
      )}
      {active && activeIndex >= 0 && (
        <TagCard
          key={active.id}
          tag={active}
          reg={reg}
          disabled={disabled}
          onChange={(n) => onChange(tags.map((x, j) => (j === activeIndex ? n : x)))}
          onRemove={() => {
            const rest = tags.filter((t) => t.id !== active.id);
            onChange(rest);
            setActiveId(rest[Math.max(0, activeIndex - 1)]?.id ?? null);
          }}
        />
      )}
      <p className="text-[11px] text-text-dim/60">
        Built-in wallet classes, no setup needed: {reg.tags.builtin.map((b) => `@${b.name} (${b.summary})`).join(' ')}
      </p>
    </div>
  );
}
