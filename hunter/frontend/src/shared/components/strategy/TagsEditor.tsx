// The fingerprint's tags: one card per tag. A trade carries the tag when ANY of its
// matchers holds (on the tag's side only); rules then read `@name` (the trades with
// it) and `@!name` (the rest). Every matcher and option is explained from the
// registry's one definition.

import { useState, type ReactNode } from 'react';

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

function B({ children }: { children: ReactNode }) {
  return <strong className="font-semibold text-text">{children}</strong>;
}

/** Matchers that read this trade's instruction list. */
const LIST_MATCHERS: readonly MatcherKey[] = ['program', 'ix_shape', 'ix_template', 'ix_contains', 'ix_lacks'];
/** Matchers that read who sent the trade. */
const WHO_MATCHERS: readonly MatcherKey[] = ['wallet', 'creator'];
/** What the volume pattern stores. Everything else on a volume tag stays visible, not dropped. */
const VOLUME_STORED: readonly MatcherKey[] = ['ix_shape', 'wallet', 'creator'];

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
  }
}

function startValue(k: MatcherKey): Partial<TagMatch> {
  switch (k) {
    case 'creator':
      return { creator: true };
    default:
      return { [k]: [] };
  }
}

function matcherGroups(unused: TagFieldSpec[]): { label: string; fields: TagFieldSpec[] }[] {
  const list = unused.filter((f) => LIST_MATCHERS.includes(f.key as MatcherKey));
  const who = unused.filter((f) => WHO_MATCHERS.includes(f.key as MatcherKey));
  const rest = unused.filter((f) => !list.includes(f) && !who.includes(f));
  return [
    { label: "This trade's list", fields: list },
    { label: 'Who', fields: who },
    { label: 'Other', fields: rest },
  ].filter((g) => g.fields.length > 0);
}

function MatcherSelect({
  unused,
  disabled,
  onPick,
}: {
  unused: TagFieldSpec[];
  disabled?: boolean;
  onPick: (k: MatcherKey) => void;
}) {
  if (unused.length === 0) return null;
  return (
    <Select
      className="w-56"
      value=""
      disabled={disabled}
      onChange={(e) => {
        const k = e.target.value as MatcherKey;
        if (k) onPick(k);
      }}
    >
      <option value="">+ add a matcher...</option>
      {matcherGroups(unused).map((g) => (
        <optgroup key={g.label} label={g.label}>
          {g.fields.map((f) => (
            <option key={f.key} value={f.key} title={f.summary}>
              {f.title}
            </option>
          ))}
        </optgroup>
      ))}
    </Select>
  );
}

function SideAndSticky({
  tag,
  onChange,
  reg,
  disabled,
}: {
  tag: TagDef;
  onChange: (t: TagDef) => void;
  reg: StrategyRegistry;
  disabled?: boolean;
}) {
  return (
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
  );
}

function VolumePatternCard({
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
  const shown = (Object.keys(tag.match) as MatcherKey[]).filter((k) => tag.match[k] !== undefined);
  const extra = shown.filter((k) => !VOLUME_STORED.includes(k));
  const bare = unclassifiableTagError(tag);
  const pinWarning = feePinWarning([tag]);
  const setMatch = (match: TagMatch) => onChange({ ...tag, match });
  return (
    <div className="flex flex-col gap-3 rounded-md border border-white/10 bg-bg-card p-3">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-[12px] font-semibold text-text-dim">Name</span>
        <span className="font-mono text-[13px] text-text">volume</span>
        <span className="text-[12px] text-text-mid">
          The name stays volume. Rules read <code className="text-text">@volume</code> and <code className="text-text">@!volume</code>.
        </span>
        <IconButton className="ml-auto" variant="ghost" size="sm" disabled={disabled} onClick={onRemove} title="Remove tag" aria-label="Remove tag">
          <CloseIcon />
        </IconButton>
      </div>
      <p className="text-[12px] leading-snug text-text-dim">
        Volume pattern. The owner split builds it once from the launch group's coins. Nothing writes it while a rule runs. Dump stays a matcher list.
      </p>

      <section className="flex flex-col gap-2 rounded border border-white/10 p-2">
        <h4 className="text-[12px] font-semibold text-text">Stored</h4>
        <div className="flex flex-col gap-1">
          <span className="text-[11px] font-semibold text-text">Ix structures and ix cores</span>
          <span className="text-[11px] leading-snug text-text-dim">
            An <B>ix structure</B> is a transaction's whole instruction list, in order. An <B>ix core</B> is that list without compute budget, System, token-account, memo and Lighthouse instructions, so one row catches every variant the dev rotates. A trade on a stored row is <B>owner</B>.
          </span>
          <IxPatternRowsEditor rows={tag.match.ix_shape ?? []} onChange={(ix_shape) => setMatch({ ...tag.match, ix_shape })} disabled={disabled} />
          {pinWarning && (
            <p className="rounded-md border border-warning/40 bg-warning/10 px-2.5 py-2 text-[12px] leading-snug text-warning">{pinWarning}</p>
          )}
        </div>
        <div className="flex flex-col gap-1">
          <span className="text-[11px] font-semibold text-text">Wallets that come back</span>
          <div className="flex flex-col gap-1 text-[11px] leading-snug text-text-dim">
            <p>Chosen when the list is built. An address is stored only if it passes one test below and traded one of this dev's <B>recent launches</B>. Recent is the stretch where 90% of these wallets come back.</p>
            <p><B>Same slot.</B> Two buys in the same instant. One size is exactly <B>double</B> the other. The smaller wallet sells that bag about <B>60 seconds</B> later. Saved after <B>20 coins</B>.</p>
            <p><B>Below zero.</B> The wallet sold tokens it never bought, so the bag goes negative. Saved after <B>2 coins</B>.</p>
            <p><B>Never sells.</B> The wallet buys and does not take profit. <B>10 coins</B>, <B>20 buys</B>, and <B>no sell</B> anywhere.</p>
            <p><B>Stays in the group.</B> The wallet lives on this dev's launches. <B>3 coins</B> here, and <B>80%</B> of its coins are this dev.</p>
          </div>
          <LinesInput value={tag.match.wallet ?? []} disabled={disabled} split={/[\s,]+/} placeholder="one wallet per line" onChange={(wallet) => setMatch({ ...tag.match, wallet })} />
        </div>
        <label className="flex items-start gap-1.5 text-[12px] text-text-mid">
          <input
            type="checkbox"
            className="mt-0.5 accent-accent"
            disabled={disabled}
            checked={tag.match.creator === true}
            onChange={(e) => setMatch({ ...tag.match, creator: e.target.checked })}
          />
          <span>
            Creator
            <span className="mt-0.5 block text-[11px] leading-snug text-text-dim">The flag is on. The <B>creator</B> address is not stored. The wallet that creates the coin is owner on that coin.</span>
          </span>
        </label>
      </section>

      <section className="flex flex-col gap-1 rounded border border-white/10 p-2 text-[12px] leading-snug text-text-dim">
        <h4 className="text-[12px] font-semibold text-text">While a rule runs</h4>
        <p>A trade is <B>owner</B> when it uses a stored row, comes from a stored wallet or the creator, or comes from a wallet that is already <B>sticky</B> on this coin. Every other trade is <B>outsider</B>. A trade already printed stays as it was. Nothing is written.</p>
      </section>

      {extra.length > 0 && (
        <div className="flex flex-col gap-2">
          <span className="text-[11px] font-semibold text-text">Also stored on this tag</span>
          {extra.map((k) => {
            const f = tagField(reg, k);
            return (
              <div key={k} className="flex flex-col gap-1 rounded border border-white/5 p-1.5">
                <div className="flex items-center gap-1 text-[11px] text-text">
                  <span className="font-semibold">{f?.title ?? k}</span>
                  <IconButton
                    variant="ghost"
                    size="sm"
                    disabled={disabled}
                    title="Remove this matcher"
                    aria-label="Remove matcher"
                    onClick={() => {
                      const next = { ...tag.match };
                      delete next[k];
                      setMatch(next);
                    }}
                  >
                    <CloseIcon />
                  </IconButton>
                </div>
                <MatcherEditor k={k} match={tag.match} reg={reg} disabled={disabled} onChange={setMatch} />
              </div>
            );
          })}
        </div>
      )}
      {bare && <p className="text-[11px] text-red">{bare}</p>}
      <SideAndSticky tag={tag} onChange={onChange} reg={reg} disabled={disabled} />
      <p className="text-[11px] text-text-dim/80">{tagSentence(tag)}</p>
    </div>
  );
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
      <MatcherSelect
        unused={unused}
        disabled={disabled}
        onPick={(k) => onChange({ ...tag, match: { ...tag.match, ...startValue(k) } })}
      />
      <SideAndSticky tag={tag} onChange={onChange} reg={reg} disabled={disabled} />
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
    const name = freshName('tag', tags.map((t) => t.name));
    const next = emptyTag(name);
    onChange([...tags, next]);
    setActiveId(next.id);
  };
  const hasVolume = tags.some((t) => t.name === 'volume');
  const addVolume = () => {
    const next = emptyTag('volume');
    next.sticky = true;
    next.match = { creator: true };
    onChange([...tags, next]);
    setActiveId(next.id);
  };
  return (
    <div className="flex flex-col gap-2">
      <p className="text-[12px] leading-snug text-text-dim">
        {reg.tags.summary}
        <InfoTooltip className="ml-1" title="Tags" body={`${reg.tags.summary}\n\nExample: ${reg.tags.example}`} />
      </p>
      <p className="text-[12px] leading-snug text-text-dim">
        The volume tag stores the volume pattern. Open it for the workflow. Any other tag, including dump, is a matcher list.
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
        {!hasVolume && (
          <Button variant="subtle" size="xs" className="mb-1 shrink-0" disabled={disabled} onClick={addVolume}>
            <PlusIcon className="size-3" /> volume
          </Button>
        )}
      </div>
      {tags.length === 0 && (
        <p className="text-[12px] italic text-text-dim/70">
          No tags: metrics that need a tag (holdings, transaction counts, slot and wave tag reads) read nothing on this fingerprint.
        </p>
      )}
      {active && activeIndex >= 0 && (
        active.name === 'volume' ? (
          <VolumePatternCard
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
        ) : (
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
        )
      )}
      <p className="text-[11px] text-text-dim/60">
        Built-in wallet classes, no setup needed: {reg.tags.builtin.map((b) => `@${b.name} (${b.summary})`).join(' ')}
      </p>
    </div>
  );
}
