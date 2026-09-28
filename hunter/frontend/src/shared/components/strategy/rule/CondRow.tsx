// One condition of a rule: `[metric] [whose trades] [span] [is]`, with its sentence
// written underneath (or, when it cannot be saved, the reason). A signal condition is
// `[signal] holds / does not hold`.

import { useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
import { createPortal } from 'react-dom';

import { ConditionInput } from '../ConditionInput';
import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { IconButton } from 'components/ui/IconButton';
import { CloseIcon } from 'components/ui/icons';
import { cn } from 'lib/cn';
import {
  builtinTagNames,
  defaultRef,
  parseSpan,
  spanKeys,
  type MetricRef,
  type SpanModel,
} from 'lib/strategy/metricRef';
import { familyShort } from 'lib/strategy/metricColors';
import { ROLE } from 'lib/strategy/roleColors';
import { familyName, findFamily, findMetric, type MetricSpec, type StrategyRegistry } from 'lib/strategy/registry';
import type { Cond, MetricCond } from 'lib/strategy/ruleDoc';
import { applySpanSpell, spanReading, spanSpells, type SpanSpell } from 'lib/strategy/spanEditor';
import { metricCondError } from 'lib/strategy/validate';

export interface CondContext {
  reg: StrategyRegistry;
  /** Tag names the rule's fingerprint defines. */
  tags: readonly string[];
  /** Signal names the rule defines (a signal condition picks from these). */
  signals: readonly string[];
  /** Before the buy: our position does not exist, so its metrics are not offered. */
  beforeBuy: boolean;
  disabled?: boolean;
}

export function CondRow({
  cond,
  onChange,
  onRemove,
  ctx,
}: {
  cond: Cond;
  onChange: (c: Cond) => void;
  onRemove: () => void;
  ctx: CondContext;
}) {
  const muted = cond.off && 'opacity-50';
  return (
    <div className={cn('flex min-w-0 flex-1 flex-col gap-0.5', muted)}>
      <div className="flex flex-wrap items-center gap-1.5">
        {cond.kind === 'metric' ? (
          <MetricCondFields cond={cond} onChange={onChange} ctx={ctx} />
        ) : (
          <>
            <Select
              className="w-40 font-mono"
              style={{ color: ROLE.signal, borderColor: ROLE.signal }}
              value={cond.signal}
              disabled={ctx.disabled}
              onChange={(e) => onChange({ ...cond, signal: e.target.value })}
            >
              {!ctx.signals.includes(cond.signal) && <option value={cond.signal}>{cond.signal} (missing)</option>}
              {ctx.signals.map((s) => (
                <option key={s} value={s}>
                  {s}
                </option>
              ))}
            </Select>
            <Select
              className="w-36"
              value={cond.not ? 'not' : 'holds'}
              disabled={ctx.disabled}
              onChange={(e) => onChange({ ...cond, not: e.target.value === 'not' })}
            >
              <option value="holds">holds</option>
              <option value="not">does not hold</option>
            </Select>
          </>
        )}
        <label className="ml-auto flex items-center gap-1 text-[11px] text-text-dim" title="Off: kept in the rule and checked for mistakes, but never used.">
          <input
            type="checkbox"
            className="accent-accent"
            checked={!cond.off}
            disabled={ctx.disabled}
            onChange={(e) => onChange({ ...cond, off: !e.target.checked })}
          />
          on
        </label>
        <IconButton variant="ghost" size="sm" onClick={onRemove} disabled={ctx.disabled} aria-label="Remove condition" title="Remove condition">
          <CloseIcon />
        </IconButton>
      </div>
      <CondFootnote cond={cond} ctx={ctx} />
    </div>
  );
}

function CondFootnote({ cond, ctx }: { cond: Cond; ctx: CondContext }) {
  if (cond.kind === 'signal') {
    if (ctx.signals.includes(cond.signal)) return null;
    return <p className="text-[11px] text-red">There is no signal `{cond.signal}`.</p>;
  }
  const err = metricCondError(ctx.reg, cond, ctx.tags);
  if (!err) return null;
  return <p className="text-[11px] text-red">{err}</p>;
}

function MetricCondFields({
  cond,
  onChange,
  ctx,
}: {
  cond: MetricCond;
  onChange: (c: Cond) => void;
  ctx: CondContext;
}) {
  const spec = findMetric(ctx.reg, cond.ref.metric);
  const setRef = (ref: MetricRef) => {
    // A different unit means a different scale: the old numbers would not mean the same.
    const next = findMetric(ctx.reg, ref.metric);
    const is = spec && next && next.unit === spec.unit ? cond.is : [];
    onChange({ ...cond, ref, is });
  };
  return (
    <>
      <RefFields r={cond.ref} onChange={setRef} ctx={ctx} />
      <ConditionInput
        className="w-44"
        value={cond.is}
        unit={spec?.unit ?? 'count'}
        eqTolerance={spec?.eq_tolerance ?? 0}
        disabled={ctx.disabled}
        onChange={(is) => onChange({ ...cond, is })}
        placeholder={spec?.unit === 'flag' ? '= 1' : '>= 2'}
      />
    </>
  );
}

/** The read of a condition: `[metric] [whose trades] [span]`. Shared by a rule
 *  condition and a sweep axis. Picking another metric keeps the tag and span when the
 *  new metric takes them, else starts from its default read. */
export function RefFields({
  r,
  onChange,
  ctx,
}: {
  r: MetricRef;
  onChange: (r: MetricRef) => void;
  ctx: CondContext;
}) {
  const spec = findMetric(ctx.reg, r.metric);
  const pickMetric = (path: string) => {
    const next = findMetric(ctx.reg, path);
    if (!next) return;
    const options = tagOptions(ctx, next);
    const tag =
      r.tag && options.some((o) => o.value === r.tag)
        ? r.tag
        : next.tags === 'required'
          ? options[0]?.value
          : undefined;
    const oldSpan = parseSpan(r.span, r.slice);
    const { span, slice } =
      typeof oldSpan !== 'string' && spanAllowed(next, oldSpan) ? spanKeys(oldSpan) : defaultRef(next);
    const ref: MetricRef = { metric: path };
    if (tag) ref.tag = tag;
    if (span) ref.span = span;
    if (slice) ref.slice = slice;
    onChange(ref);
  };
  return (
    <>
      <MetricSelect value={r.metric} onChange={pickMetric} ctx={ctx} />
      {spec && spec.tags !== 'none' && (
        <Select
          className="w-40"
          value={r.tag ?? ''}
          disabled={ctx.disabled}
          title="Whose trades this counts"
          onChange={(e) => onChange({ ...r, tag: e.target.value || undefined })}
        >
          {spec.tags === 'optional' && <option value="">all trades</option>}
          {spec.tags === 'required' && !r.tag && <option value="">pick a tag...</option>}
          {r.tag && !tagOptions(ctx, spec).some((o) => o.value === r.tag) && (
            <option value={r.tag}>@{r.tag} (not defined)</option>
          )}
          {tagOptions(ctx, spec).map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </Select>
      )}
      {spec && <SpanFields spec={spec} r={r} onChange={onChange} reg={ctx.reg} disabled={ctx.disabled} />}
    </>
  );
}

/** One metric's ink. Light, and a step apart from its neighbor, so each name reads alone. */
function metricInk(hue: number): { hue: number; color: string; border: string; background: string } {
  const h = ((Math.round(hue) % 360) + 360) % 360;
  const light = 66 + (h % 16);
  const color = `hsl(${h}, 86%, ${Math.min(92, light + 14)}%)`;
  return {
    hue: h,
    color,
    border: `hsla(${h}, 78%, ${Math.min(86, light + 6)}%, 0.75)`,
    background: `hsla(${h}, 70%, ${light}%, 0.22)`,
  };
}

function MetricChip({ family, hue, title }: { family: string; hue: number; title?: string }) {
  const ink = metricInk(hue);
  const style: CSSProperties = { color: ink.color, backgroundColor: ink.background, borderColor: ink.border };
  return (
    <span
      className="inline-flex shrink-0 items-center rounded border px-1 font-mono text-[10px] font-bold leading-4 tracking-wide"
      style={style}
      title={title || family}
    >
      {familyShort(family)}
    </span>
  );
}

/** Metric picker, grouped by family. The closed face is the family color and the
 *  name. The menu indents each metric under its family. The phrase sits on the
 *  row and on hover. Our position's metrics are left out before the buy. The menu
 *  is portaled so the editor modal does not clip it. */
export function MetricSelect({
  value,
  onChange,
  ctx,
}: {
  value: string;
  onChange: (path: string) => void;
  ctx: Pick<CondContext, 'reg' | 'beforeBuy' | 'disabled'>;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const btnRef = useRef<HTMLButtonElement>(null);
  const spec = findMetric(ctx.reg, value);
  const family = familyName(value);
  const title = spec ? `${spec.phrase}. ${spec.summary}` : undefined;
  return (
    <div>
      <button
        ref={btnRef}
        type="button"
        disabled={ctx.disabled}
        aria-expanded={open}
        aria-haspopup="listbox"
        title={title}
        onClick={() => {
          if (ctx.disabled) return;
          setOpen((v) => !v);
          setQuery('');
        }}
        className="inline-flex h-7 max-w-56 items-center gap-1 rounded-md border bg-white/2 px-1.5 hover:brightness-110 disabled:opacity-50"
        style={spec ? { borderColor: metricInk(spec.hue).border } : undefined}
      >
        {spec && <MetricChip family={family} hue={spec.hue} title={findFamily(ctx.reg, family)?.title} />}
        <span className="truncate font-mono text-[11px] font-semibold" style={spec ? { color: metricInk(spec.hue).color } : undefined}>
          {spec?.name ?? (value || 'pick a metric')}
        </span>
      </button>
      {open && btnRef.current && (
        <MetricMenu
          anchor={btnRef.current}
          reg={ctx.reg}
          beforeBuy={ctx.beforeBuy}
          value={value}
          query={query}
          onQuery={setQuery}
          onClose={() => setOpen(false)}
          onPick={(path) => {
            onChange(path);
            setOpen(false);
          }}
        />
      )}
    </div>
  );
}

/** The family's color: the middle hue of its metrics, so one outlier does not paint the group. */
function bandHue(hues: number[]): number {
  if (hues.length === 0) return 0;
  const sorted = [...hues].sort((a, b) => a - b);
  return sorted[Math.floor((sorted.length - 1) / 2)];
}

function MetricMenu({
  anchor,
  reg,
  beforeBuy,
  value,
  query,
  onQuery,
  onClose,
  onPick,
}: {
  anchor: HTMLElement;
  reg: StrategyRegistry;
  beforeBuy: boolean;
  value: string;
  query: string;
  onQuery: (q: string) => void;
  onClose: () => void;
  onPick: (path: string) => void;
}) {
  const menuRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  const [box, setBox] = useState({ top: 0, left: 0, maxH: 280 });
  const q = query.trim().toLowerCase();
  const groups = useMemo(() => {
    return reg.families
      .map((f) => ({
        family: f,
        metrics: f.metrics.filter((m) => {
          if (beforeBuy && m.position) return false;
          if (!q) return true;
          return m.name.includes(q) || m.path.toLowerCase().includes(q) || m.phrase.toLowerCase().includes(q) || m.summary.toLowerCase().includes(q);
        }),
      }))
      .filter((g) => g.metrics.length > 0);
  }, [reg, beforeBuy, q]);

  useLayoutEffect(() => {
    const place = () => {
      const r = anchor.getBoundingClientRect();
      const below = window.innerHeight - r.bottom;
      const maxH = Math.min(320, Math.max(180, below - 12));
      setBox({ top: r.bottom + 4, left: Math.min(r.left, window.innerWidth - 392), maxH });
    };
    place();
    window.addEventListener('scroll', place, true);
    window.addEventListener('resize', place);
    return () => {
      window.removeEventListener('scroll', place, true);
      window.removeEventListener('resize', place);
    };
  }, [anchor]);

  useEffect(() => {
    inputRef.current?.focus();
    menuRef.current?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' });
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' || e.key === 'Tab') {
        e.stopPropagation();
        e.preventDefault();
        closeRef.current();
      }
    };
    const onDown = (e: MouseEvent) => {
      const t = e.target as Node;
      if (!anchor.contains(t) && !menuRef.current?.contains(t)) closeRef.current();
    };
    document.addEventListener('keydown', onKey, true);
    document.addEventListener('mousedown', onDown);
    return () => {
      document.removeEventListener('keydown', onKey, true);
      document.removeEventListener('mousedown', onDown);
    };
  }, [anchor]);

  return createPortal(
    <div
      ref={menuRef}
      role="listbox"
      style={{ position: 'fixed', top: box.top, left: box.left, maxHeight: box.maxH, width: 380, zIndex: 260 }}
      className="flex flex-col overflow-hidden rounded-md border border-white/10 bg-bg-card shadow-lg"
    >
      <input
        ref={inputRef}
        value={query}
        placeholder="Find a metric"
        onChange={(e) => onQuery(e.target.value)}
        className="border-b border-white/10 bg-transparent px-2 py-1.5 font-mono text-[11px] text-text outline-none placeholder:text-text-dim/60"
      />
      <div className="min-h-0 flex-1 overflow-y-auto py-1">
        {groups.length === 0 && <p className="px-2 py-1.5 text-[11px] text-text-dim">No metric</p>}
        {groups.map((g) => {
          const band = metricInk(bandHue(g.metrics.map((m) => m.hue)));
          return (
            <div key={g.family.name} className="pb-1">
              <div className="sticky top-0 z-10 flex items-center gap-1.5 bg-bg-card px-2 py-1" title={g.family.summary}>
                <MetricChip family={g.family.name} hue={band.hue} title={g.family.title} />
                <span className="truncate text-[11px] font-semibold" style={{ color: band.color }}>{g.family.title}</span>
              </div>
              <div className="ml-3.5 border-l pl-1" style={{ borderLeftColor: band.color }}>
                {g.metrics.map((m) => {
                  const tone = metricInk(m.hue);
                  return (
                    <button
                      key={m.path}
                      type="button"
                      role="option"
                      aria-selected={m.path === value}
                      title={`${m.phrase}. ${m.summary}`}
                      onMouseDown={(e) => e.preventDefault()}
                      onClick={() => onPick(m.path)}
                      className={cn(
                        'flex w-full items-center gap-1.5 py-0.5 pr-2 pl-1.5 text-left hover:bg-white/6',
                        m.path === value && 'bg-primary/15',
                      )}
                    >
                      <span className="h-1.5 w-1.5 shrink-0 rounded-full" style={{ backgroundColor: tone.color }} />
                      <span className="shrink-0 font-mono text-[11px] font-semibold" style={{ color: tone.color }}>
                        {m.name}
                      </span>
                      <span className="truncate text-[11px] text-text-dim">{m.phrase}</span>
                    </button>
                  );
                })}
              </div>
            </div>
          );
        })}
      </div>
    </div>,
    document.body,
  );
}

/** The tag choices a metric offers: each fingerprint tag (and its negation, at the
 *  trade level), or the built-in wallet classes. */
function tagOptions(ctx: CondContext, spec: MetricSpec): { value: string; label: string }[] {
  if (spec.tag_level === 'wallet_class') {
    return builtinTagNames(ctx.reg).map((n) => ({ value: n, label: `@${n} wallets` }));
  }
  const out: { value: string; label: string }[] = [];
  for (const t of ctx.tags) {
    out.push({ value: t, label: spec.tag_level === 'template' ? `@${t} templates` : `@${t} (with)` });
    if (spec.tag_level === 'trade') out.push({ value: `!${t}`, label: `@!${t} (without)` });
  }
  return out;
}

function spanAllowed(spec: MetricSpec, m: SpanModel): boolean {
  if (m.kind === 'life') return spec.spans.life;
  if (m.kind === 'since_age') return spec.spans.since_age;
  return spec.spans.window && spec.spans.slice === !!m.slice;
}

/** The spellings, closed into one menu so the row stays as wide as the text box. */
function SpellMenu({
  spells,
  current,
  disabled,
  title,
  onPick,
}: {
  spells: SpanSpell[];
  current: string;
  disabled?: boolean;
  title: string;
  onPick: (spell: SpanSpell) => void;
}) {
  const [open, setOpen] = useState(false);
  const btnRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const [box, setBox] = useState({ top: 0, left: 0 });

  useLayoutEffect(() => {
    if (!open || !btnRef.current) return;
    const place = () => {
      const r = btnRef.current!.getBoundingClientRect();
      setBox({ top: r.bottom + 4, left: Math.min(r.left, window.innerWidth - 280) });
    };
    place();
    window.addEventListener('scroll', place, true);
    window.addEventListener('resize', place);
    return () => {
      window.removeEventListener('scroll', place, true);
      window.removeEventListener('resize', place);
    };
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false);
    };
    const onDown = (e: MouseEvent) => {
      const t = e.target as Node;
      if (!btnRef.current?.contains(t) && !menuRef.current?.contains(t)) setOpen(false);
    };
    document.addEventListener('keydown', onKey);
    document.addEventListener('mousedown', onDown);
    return () => {
      document.removeEventListener('keydown', onKey);
      document.removeEventListener('mousedown', onDown);
    };
  }, [open]);

  if (!spells.length) return null;
  return (
    <>
      <button
        ref={btnRef}
        type="button"
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
        title={title}
        onClick={() => setOpen((v) => !v)}
        className="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md border border-white/10 bg-white/4 text-[12px] text-text-dim hover:text-text disabled:opacity-50"
      >
        ▾
      </button>
      {open &&
        createPortal(
          <div
            ref={menuRef}
            role="listbox"
            style={{ position: 'fixed', top: box.top, left: box.left, zIndex: 260, width: 260 }}
            className="flex flex-col overflow-hidden rounded-md border border-white/10 bg-bg-card py-1 shadow-lg"
          >
            {spells.map((s) => (
              <button
                key={s.key}
                type="button"
                role="option"
                aria-selected={s.write === current}
                title={s.detail}
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => {
                  onPick(s);
                  setOpen(false);
                }}
                className={cn(
                  'flex items-baseline gap-2 px-2 py-1 text-left text-[12px] hover:bg-white/5',
                  s.write === current && 'bg-white/10',
                )}
              >
                <span className="w-16 shrink-0 font-mono text-text">{s.face}</span>
                <span className="text-text-dim">{s.label}</span>
              </button>
            ))}
          </div>,
          document.body,
        )}
    </>
  );
}

/** The span box, a menu of the spellings this metric accepts, and the reading
 *  of the current text. A life-only metric has no span. */
function SpanFields({
  spec,
  r,
  onChange,
  reg,
  disabled,
}: {
  spec: MetricSpec;
  r: MetricRef;
  onChange: (r: MetricRef) => void;
  reg: StrategyRegistry;
  disabled?: boolean;
}) {
  if (!spec.spans.window && !spec.spans.since_age) return null;
  const spells = spanSpells(reg, spec);
  const reading = spanReading(spec, r);
  const pick = (spell: SpanSpell) => {
    const next = applySpanSpell(spec, r, spell);
    onChange({ ...r, span: next.span, slice: next.slice });
  };
  const placeholder = spec.spans.since_age ? 'age60s' : spec.spans.life ? '' : '10s';
  return (
    <div className="flex min-w-0 max-w-xl flex-col gap-0.5">
      <div className="flex flex-wrap items-center gap-1">
        <Input
          fieldSize="sm"
          className="w-24 font-mono"
          value={r.span ?? ''}
          disabled={disabled}
          placeholder={placeholder}
          title="Type a spelling, or pick one from the menu."
          onChange={(e) => onChange({ ...r, span: e.target.value.trim() || undefined, slice: spec.spans.slice ? r.slice : undefined })}
        />
        <SpellMenu
          spells={spells.span}
          current={r.span ?? ''}
          disabled={disabled}
          title="Spellings this box accepts"
          onPick={pick}
        />
        {spec.spans.slice && (
          <>
            <span className="text-[11px] text-text-dim">slice</span>
            <Input
              fieldSize="sm"
              className="w-16 font-mono"
              value={r.slice ?? ''}
              disabled={disabled}
              placeholder="2s"
              title="Shorter than the span, same unit, no @. The span's @ applies to both."
              onChange={(e) => onChange({ ...r, slice: e.target.value.trim() || undefined })}
            />
            <SpellMenu
              spells={spells.slice}
              current={r.slice ?? ''}
              disabled={disabled}
              title="Slice spellings: shorter, same unit, no @"
              onPick={pick}
            />
          </>
        )}
        <span className={cn('text-[11px]', reading.bad ? 'text-red' : 'text-text-dim')}>{reading.text}</span>
      </div>
    </div>
  );
}
