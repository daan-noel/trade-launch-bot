// One condition of a rule: `[metric] [whose trades] [span] [is]`, with its sentence
// written underneath (or, when it cannot be saved, the reason). A signal condition is
// `[signal] holds / does not hold`.

import { ConditionInput } from '../ConditionInput';
import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { IconButton } from 'components/ui/IconButton';
import { InfoTooltip } from 'components/ui/InfoTooltip';
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
import { findMetric, metricHelp, type MetricSpec, type StrategyRegistry } from 'lib/strategy/registry';
import type { Cond, MetricCond } from 'lib/strategy/ruleDoc';
import { condSentence } from 'lib/strategy/sentences';
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
    <div className={cn('flex flex-col gap-0.5 rounded border border-white/5 bg-white/2 px-2 py-1.5', muted)}>
      <div className="flex flex-wrap items-center gap-1.5">
        {cond.kind === 'metric' ? (
          <MetricCondFields cond={cond} onChange={onChange} ctx={ctx} />
        ) : (
          <>
            <Select
              className="w-40"
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
    const missing = !ctx.signals.includes(cond.signal);
    return (
      <p className={cn('text-[11px]', missing ? 'text-red' : 'text-text-dim')}>
        {missing ? `There is no signal \`${cond.signal}\`.` : condSentence(ctx.reg, cond)}
      </p>
    );
  }
  const err = metricCondError(ctx.reg, cond, ctx.tags);
  return <p className={cn('text-[11px]', err ? 'text-red' : 'text-text-dim')}>{err ?? condSentence(ctx.reg, cond)}</p>;
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
      {spec && <InfoTooltip title={spec.path} body={metricHelp(spec)} />}
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
      {spec && <SpanFields spec={spec} r={r} onChange={onChange} disabled={ctx.disabled} />}
    </>
  );
}

/** Metric picker, grouped by family. Our position's metrics are left out before the buy. */
export function MetricSelect({
  value,
  onChange,
  ctx,
  className,
}: {
  value: string;
  onChange: (path: string) => void;
  ctx: Pick<CondContext, 'reg' | 'beforeBuy' | 'disabled'>;
  className?: string;
}) {
  return (
    <Select
      className={cn('w-56', className)}
      value={value}
      disabled={ctx.disabled}
      onChange={(e) => onChange(e.target.value)}
      title={findMetric(ctx.reg, value)?.summary}
    >
      {!findMetric(ctx.reg, value) && <option value={value}>{value || 'pick a metric...'}</option>}
      {ctx.reg.families.map((f) => {
        const ms = f.metrics.filter((m) => !(ctx.beforeBuy && m.position));
        if (!ms.length) return null;
        return (
          <optgroup key={f.name} label={`${f.title} (${f.name})`}>
            {ms.map((m) => (
              <option key={m.path} value={m.path} title={m.summary}>
                {m.path} : {m.phrase}
              </option>
            ))}
          </optgroup>
        );
      })}
    </Select>
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

type SpanKindChoice = 'life' | 'window' | 'since_age';

function spanAllowed(spec: MetricSpec, m: SpanModel): boolean {
  if (m.kind === 'life') return spec.spans.life;
  if (m.kind === 'since_age') return spec.spans.since_age;
  return spec.spans.window && spec.spans.slice === !!m.slice;
}

/** The span controls: which kind (life / a trailing window / since an age), then its
 *  text. Only the kinds the metric accepts are offered. */
function SpanFields({
  spec,
  r,
  onChange,
  disabled,
}: {
  spec: MetricSpec;
  r: MetricRef;
  onChange: (r: MetricRef) => void;
  disabled?: boolean;
}) {
  const kinds: SpanKindChoice[] = [];
  if (spec.spans.life) kinds.push('life');
  if (spec.spans.window) kinds.push('window');
  if (spec.spans.since_age) kinds.push('since_age');
  if (kinds.length === 1 && kinds[0] === 'life') return null;
  const parsed = parseSpan(r.span, r.slice);
  const kind: SpanKindChoice =
    typeof parsed === 'string' ? (r.span?.startsWith('age') ? 'since_age' : 'window') : parsed.kind;
  const setKind = (k: SpanKindChoice) => {
    if (k === 'life') onChange({ ...r, span: undefined, slice: undefined });
    else if (k === 'since_age') onChange({ ...r, span: 'age0s', slice: undefined });
    else onChange({ ...r, span: '10s', slice: spec.spans.slice ? '2s' : undefined });
  };
  const label: Record<SpanKindChoice, string> = { life: 'whole life', window: 'last ...', since_age: 'since age ...' };
  const ageSecs = kind === 'since_age' && typeof parsed !== 'string' && parsed.kind === 'since_age' ? parsed.secs : null;
  return (
    <>
      {kinds.length > 1 ? (
        <Select className="w-28" value={kind} disabled={disabled} title="Over what stretch" onChange={(e) => setKind(e.target.value as SpanKindChoice)}>
          {kinds.map((k) => (
            <option key={k} value={k}>
              {label[k]}
            </option>
          ))}
        </Select>
      ) : (
        <span className="text-[11px] text-text-dim">{label[kind]}</span>
      )}
      {kind === 'window' && (
        <>
          <Input
            fieldSize="sm"
            className="w-20 font-mono"
            value={r.span ?? ''}
            disabled={disabled}
            placeholder="10s"
            title="s = seconds, sl = slots (about 0.4 s), p = prints; @2 = the window ending 2 units ago. 10s, 20sl, 5p, 10s@2."
            onChange={(e) => onChange({ ...r, span: e.target.value.trim() || undefined })}
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
                title="The shorter, most recent part of the window, same unit."
                onChange={(e) => onChange({ ...r, slice: e.target.value.trim() || undefined })}
              />
            </>
          )}
        </>
      )}
      {kind === 'since_age' && (
        <Input
          fieldSize="sm"
          numeric
          unit="s"
          className="w-20"
          numericValue={ageSecs}
          disabled={disabled}
          title="Counts from this coin age (seconds) until now. 0 = since creation."
          onNumericChange={(n) => onChange({ ...r, span: `age${n ?? 0}s`, slice: undefined })}
        />
      )}
    </>
  );
}
