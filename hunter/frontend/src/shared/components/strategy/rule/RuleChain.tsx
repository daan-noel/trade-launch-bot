// The rule builder, in run order. Signal is its own row, above buy and sell.
// Buy when is On, then Only if. Keep looking is Tries, then Give up. Sell is a
// TP/SL chip, an Always chip, then one name per stage. Each chip keeps its role
// color. On, Only If, and Give Up are three greens. Tries is gray. A stage is
// accent. Always is a dashed info outline. A signal is a dashed secondary
// outline. TP/SL is a plain outline; the words carry buy and sell. The open
// chip fills in that color. The section under it stays plain, so the lines stay
// readable. One name is open at a time. The Rules list shows the same Signal,
// Buy gates, and Sell chain.

import { useState, useEffect, type ButtonHTMLAttributes, type ReactNode } from 'react';

import { Button } from 'components/ui/Button';
import { InfoTooltip } from 'components/ui/InfoTooltip';
import { Input } from 'components/ui/Input';
import { PlusIcon } from 'components/ui/icons';
import { formatDecimalTrim } from 'utils/format';
import { cn } from 'lib/cn';
import { LabelTip } from '../LabelTip';
import { RULE_FIELD_HELP } from 'lib/strategy/strategyHelp';
import { buyRoleColor, ROLE, sellRoleColor } from 'lib/strategy/roleColors';
import { BUY_WHEN_LAST, BUY_WHEN_MISS, KEEP_LOOKING_MISS, buyChips, buyGateConds, LOCK_CHOICES, sellChips, signalChip, type BuyChip, type BuyKey } from 'lib/strategy/ruleChain';
import { rulePart, type StrategyRegistry } from 'lib/strategy/registry';
import { freshName, newStage, renameSignal, renameStage, type RuleDoc } from 'lib/strategy/ruleDoc';
import type { CondContext } from './CondRow';
import { GateFaces } from './MetricMark';
import { CondList, LineList, PartHeader } from './parts';
import { SignalsEditor } from './SignalsEditor';
import { StagesEditor } from './StagesEditor';

/** A chip's own color. Open fills it; closed stays an outline. Inline color beats the ghost button's hover. */
function chipPaint(color: string, open: boolean, hot: boolean): { color: string; borderColor: string; backgroundColor: string } {
  const amount = open ? (hot ? 32 : 24) : hot ? 12 : 0;
  return {
    color,
    borderColor: color,
    backgroundColor: amount === 0 ? 'transparent' : `color-mix(in srgb, ${color} ${amount}%, transparent)`,
  };
}

function RoleChip({
  color,
  open = false,
  dashed = false,
  className,
  children,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { color: string; open?: boolean; dashed?: boolean }) {
  const [hot, setHot] = useState(false);
  return (
    <Button
      variant="ghost"
      size="xs"
      {...props}
      className={cn('font-semibold', dashed && 'border-dashed', open && 'font-bold', className)}
      style={chipPaint(color, open, hot)}
      onMouseEnter={() => setHot(true)}
      onMouseLeave={() => setHot(false)}
    >
      {children}
    </Button>
  );
}

/** The open section stays plain. The chip above it carries the color. */
function SellPane({ children }: { children: ReactNode }) {
  return <div className="min-w-0">{children}</div>;
}

function BuyRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-wrap items-center gap-1">
      <span className="w-24 shrink-0 text-[12px] text-text-dim">{label}</span>
      {children}
    </div>
  );
}

function BuyPane({ children }: { children: ReactNode }) {
  return <div className="ml-24 flex flex-col gap-2">{children}</div>;
}

function BuyGate({
  chip,
  first,
  open,
  doc,
  registry,
  onToggle,
}: {
  chip: BuyChip;
  first: boolean;
  open: boolean;
  doc: RuleDoc;
  registry: StrategyRegistry;
  onToggle: () => void;
}) {
  const color = buyRoleColor(chip.key, doc.enter.lock);
  return (
    <span className="flex items-center gap-1">
      {!first && <span className="text-text-dim">→</span>}
      <RoleChip color={color} open={open} onClick={onToggle}>
        <span>{chip.title}</span>
        {chip.key !== 'lock' && <GateFaces conds={buyGateConds(doc, chip.key)} reg={registry} />}
      </RoleChip>
    </span>
  );
}

function TpslFace({ doc }: { doc: RuleDoc }) {
  const tp = doc.take_profit != null ? formatDecimalTrim(doc.take_profit, 1) : null;
  const sl = doc.stop_loss != null ? formatDecimalTrim(doc.stop_loss, 1) : null;
  if (tp == null && sl == null) {
    return (
      <span>
        <span className="text-buy">TP</span>
        <span className="text-text-dim">/</span>
        <span className="text-sell">SL</span>
      </span>
    );
  }
  return (
    <span className="inline-flex items-center gap-1">
      {tp != null && <span className="font-semibold text-buy">TP {tp}</span>}
      {tp != null && sl != null && <span className="text-text-dim">·</span>}
      {sl != null && <span className="font-semibold text-sell">SL {sl}</span>}
    </span>
  );
}

function NumberField({
  reg,
  part,
  label,
  value,
  onChange,
  unit,
  integer,
  disabled,
  className = 'w-24',
  placeholder,
  labelClass,
}: {
  reg: StrategyRegistry;
  part?: string;
  label: string;
  value: number | null;
  onChange: (n: number | null) => void;
  unit?: string;
  integer?: boolean;
  disabled?: boolean;
  className?: string;
  placeholder?: string;
  labelClass?: string;
}) {
  const p = part ? rulePart(reg, part) : undefined;
  return (
    <label className="flex flex-col gap-1 text-[11px] text-text-dim">
      <span className={cn('inline-flex items-center gap-1', labelClass)}>
        {label}
        {p && <InfoTooltip title={p.title} body={`${p.summary}\n\nExample: ${p.example}`} />}
      </span>
      <Input
        fieldSize="sm"
        numeric
        integer={integer}
        unit={unit}
        placeholder={placeholder ?? 'off'}
        numericValue={value != null && Number.isFinite(value) ? value : null}
        onNumericChange={onChange}
        disabled={disabled}
        className={className}
      />
    </label>
  );
}

export function RuleChain({
  doc,
  patch,
  registry,
  locked,
  buySol,
  onBuySol,
  maxConcurrent,
  onMaxConcurrent,
  maxTotal,
  onMaxTotal,
  tags,
}: {
  doc: RuleDoc;
  patch: (f: (d: RuleDoc) => RuleDoc) => void;
  registry: StrategyRegistry;
  locked: boolean;
  buySol: number | null;
  onBuySol: (n: number | null) => void;
  maxConcurrent: number | null;
  onMaxConcurrent: (n: number | null) => void;
  maxTotal: number | null;
  onMaxTotal: (n: number | null) => void;
  tags: readonly string[];
}) {
  const [open, setOpen] = useState<string | null>(null);
  const toggle = (key: string) => setOpen((cur) => (cur === key ? null : key));
  useEffect(() => {
    if (open?.startsWith('stage:') && !doc.stages.some((s) => `stage:${s.id}` === open)) setOpen(null);
  }, [doc.stages, open]);

  const setEnter = (e: Partial<RuleDoc['enter']>) => patch((d) => ({ ...d, enter: { ...d.enter, ...e } }));
  const signalNames = doc.signals.map((s) => s.name);
  const stageNames = doc.stages.map((s) => s.name);
  const buyCtx: CondContext = { reg: registry, tags, signals: signalNames, beforeBuy: true, disabled: locked };
  const sellCtx: CondContext = { ...buyCtx, beforeBuy: false };
  const buyOpen: BuyKey | null = open === 'on' || open === 'if' || open === 'giveup' || open === 'lock' ? open : null;
  const buy = buyChips(doc, true, buyOpen);
  const when = buy.filter((c) => c.group === 'when');
  const looking = buy.filter((c) => c.group === 'looking');
  const chips = sellChips(doc, true, open);
  const signalFace = signalChip(doc, true, open === 'signals');
  const focusId = open?.startsWith('stage:') ? open.slice('stage:'.length) : undefined;

  const addStage = () => {
    const stage = newStage(freshName(doc.stages.length ? 'stage' : 'start', stageNames));
    patch((d) => ({ ...d, stages: [...d.stages, stage] }));
    setOpen(`stage:${stage.id}`);
  };

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-col gap-1.5">
        <BuyRow label="Signal">
          {signalFace == null ? (
            <RoleChip color={ROLE.signal} dashed disabled={locked} onClick={() => setOpen('signals')}>
              <PlusIcon className="size-3" /> Add Signal
            </RoleChip>
          ) : (
            <RoleChip color={ROLE.signal} dashed open={open === 'signals'} onClick={() => toggle('signals')}>
              {signalFace}
            </RoleChip>
          )}
        </BuyRow>
        {open === 'signals' && (
          <SellPane>
            <SignalsEditor
              doc={doc}
              ctx={sellCtx}
              onChange={(signals) => patch((d) => ({ ...d, signals }))}
              onRename={(from, to) => patch((d) => renameSignal(d, from, to))}
            />
          </SellPane>
        )}
        <BuyRow label="Buy When">
          {when.map((c, i) => (
            <BuyGate key={c.key} chip={c} first={i === 0} open={open === c.key} doc={doc} registry={registry} onToggle={() => toggle(c.key)} />
          ))}
          {!when.some((c) => c.key === 'on') && (
            <RoleChip color={buyRoleColor('on', doc.enter.lock)} disabled={locked} onClick={() => setOpen('on')}>
              <PlusIcon className="size-3" /> Add On
            </RoleChip>
          )}
          {!when.some((c) => c.key === 'if') && (
            <RoleChip color={buyRoleColor('if', doc.enter.lock)} disabled={locked} onClick={() => setOpen('if')}>
              <PlusIcon className="size-3" /> Add Only If
            </RoleChip>
          )}
        </BuyRow>
        {open === 'on' && (
          <BuyPane>
            <PartHeader ctx={buyCtx} part="enter.event" />
            <CondList
              conds={doc.enter.event}
              onChange={(event) => setEnter({ event })}
              ctx={buyCtx}
              empty="No trigger: the rule buys on the first print or tick where Only if holds."
            />
          </BuyPane>
        )}
        {open === 'if' && (
          <BuyPane>
            <PartHeader ctx={buyCtx} part="enter.filters" />
            <p className="text-[11px] text-text-dim">{BUY_WHEN_MISS}</p>
            {doc.enter.lock === 'token' && <p className="text-[11px] text-text-dim">{BUY_WHEN_LAST}</p>}
            <CondList conds={doc.enter.filters} onChange={(filters) => setEnter({ filters })} ctx={buyCtx} empty="Add a condition." />
          </BuyPane>
        )}
        <BuyRow label="Keep Looking">
          {looking.map((c, i) => (
            <BuyGate key={c.key} chip={c} first={i === 0} open={open === c.key} doc={doc} registry={registry} onToggle={() => toggle(c.key)} />
          ))}
          {!looking.some((c) => c.key === 'giveup') && (
            <RoleChip color={buyRoleColor('giveup', doc.enter.lock)} disabled={locked} onClick={() => setOpen('giveup')}>
              <PlusIcon className="size-3" /> Add Give Up
            </RoleChip>
          )}
        </BuyRow>
        {open === 'lock' && (
          <BuyPane>
            <fieldset className="flex flex-col gap-1.5" disabled={locked}>
              <legend className="text-[11px] text-text-dim">Tries</legend>
              {LOCK_CHOICES.map((c) => {
                const on = (doc.enter.lock ?? '') === c.value;
                const color = buyRoleColor('lock', c.value || null);
                return (
                  <label
                    key={c.label}
                    className={cn('flex cursor-pointer gap-2 rounded-md border px-2 py-1.5', !on && 'border-white/10', locked && 'cursor-not-allowed')}
                    style={on ? { borderColor: color, backgroundColor: `color-mix(in srgb, ${color} 14%, transparent)` } : undefined}
                  >
                    <input
                      type="radio"
                      className="mt-0.5"
                      style={{ accentColor: color }}
                      name="tries"
                      checked={on}
                      disabled={locked}
                      onChange={() => setEnter({ lock: (c.value || null) as RuleDoc['enter']['lock'] })}
                    />
                    <span className="flex flex-col gap-0.5">
                      <span className={cn('text-[12px]', on ? 'font-semibold' : 'text-text-mid')} style={on ? { color } : undefined}>
                        {c.label}
                      </span>
                      <span className="text-[11px] leading-snug text-text-dim">{c.hint}</span>
                    </span>
                  </label>
                );
              })}
            </fieldset>
          </BuyPane>
        )}
        {open === 'giveup' && (
          <BuyPane>
            <PartHeader ctx={buyCtx} part="enter.final_filters" title="Give Up" />
            <p className="text-[11px] text-text-dim">{KEEP_LOOKING_MISS}</p>
            <CondList conds={doc.enter.final_filters} onChange={(final_filters) => setEnter({ final_filters })} ctx={buyCtx} empty="Add a condition." />
          </BuyPane>
        )}
        <div className="flex flex-wrap items-center gap-3 pl-24">
          <label className="flex h-8 items-center gap-1.5 text-[11px] text-text-dim">
            <input
              type="checkbox"
              className="accent-accent"
              checked={doc.exclusive}
              disabled={locked}
              onChange={(e) => patch((d) => ({ ...d, exclusive: e.target.checked }))}
            />
            {rulePart(registry, 'exclusive')?.title ?? 'Exclusive'}
            <InfoTooltip body={`${rulePart(registry, 'exclusive')?.summary ?? ''}\n\nExample: ${rulePart(registry, 'exclusive')?.example ?? ''}`} />
          </label>
          {doc.exclusive && (
            <NumberField
              reg={registry}
              label="Priority"
              integer
              placeholder="0"
              value={doc.priority}
              disabled={locked}
              className="w-20"
              onChange={(n) => patch((d) => ({ ...d, priority: n ?? 0 }))}
            />
          )}
        </div>
      </div>

      <div className="flex flex-wrap items-end gap-3">
        <span className="w-12 shrink-0 pb-1.5 text-[12px] text-text-dim">Size</span>
        {/* The two caps are the genuine `0 = off` sentinels: a stored 0 renders blank so
            "no cap" reads as unlimited. A new rule opens at 1 concurrent. */}
        <label className="flex flex-col gap-1 text-[11px] text-text-dim">
          <LabelTip tip={RULE_FIELD_HELP.buy}>Buy (◎)</LabelTip>
          <Input fieldSize="sm" numeric unit="◎" numericValue={buySol} onNumericChange={onBuySol} className="w-24" />
        </label>
        <label className="flex flex-col gap-1 text-[11px] text-text-dim">
          <LabelTip tip={RULE_FIELD_HELP.maxConcurrent}>At once</LabelTip>
          <Input fieldSize="sm" numeric integer blankZero placeholder="∞" numericValue={maxConcurrent} onNumericChange={onMaxConcurrent} className="w-20" />
        </label>
        <label className="flex flex-col gap-1 text-[11px] text-text-dim">
          <LabelTip tip={RULE_FIELD_HELP.maxTotal}>Total</LabelTip>
          <Input fieldSize="sm" numeric integer blankZero placeholder="∞" numericValue={maxTotal} onNumericChange={onMaxTotal} className="w-20" />
        </label>
        <NumberField
          reg={registry}
          part="enter.size_pct_of_pool"
          label="Size as % of pool"
          unit="%"
          placeholder="fixed"
          value={doc.enter.size_pct_of_pool}
          onChange={(n) => setEnter({ size_pct_of_pool: n })}
          disabled={locked}
        />
      </div>

      <div className="flex flex-col gap-2">
        <div className="flex flex-wrap items-center gap-1">
          <span className="w-12 shrink-0 text-[12px] text-text-dim">Sell</span>
          {chips.length === 0 && <span className="text-[12px] text-text-dim">no sell</span>}
          {chips.map((c, i) => (
            <span key={c.key} className="flex items-center gap-1">
              {i > 0 && <span className="text-text-dim">→</span>}
              <RoleChip color={sellRoleColor(c.key).color} dashed={sellRoleColor(c.key).dashed} open={open === c.key} onClick={() => toggle(c.key)}>
                {c.key === 'tpsl' ? <TpslFace doc={doc} /> : c.label}
              </RoleChip>
            </span>
          ))}
          {!chips.some((c) => c.key === 'tpsl') && (
            <RoleChip color={sellRoleColor('tpsl').color} disabled={locked} onClick={() => setOpen('tpsl')}>
              <PlusIcon className="size-3" /> Add <span className="text-buy">TP</span>/<span className="text-sell">SL</span>
            </RoleChip>
          )}
          {!chips.some((c) => c.key === 'always') && (
            <RoleChip color={sellRoleColor('always').color} dashed disabled={locked} onClick={() => setOpen('always')}>
              <PlusIcon className="size-3" /> Add Always
            </RoleChip>
          )}
          <RoleChip color={sellRoleColor('stage').color} disabled={locked} onClick={addStage}>
            <PlusIcon className="size-3" /> Add Stage
          </RoleChip>
        </div>

        {open === 'tpsl' && (
          <SellPane>
            <div className="flex flex-wrap items-end gap-3">
              <NumberField
                reg={registry}
                part="take_profit"
                label="TP"
                labelClass="font-semibold text-buy"
                unit="%"
                value={doc.take_profit}
                disabled={locked}
                onChange={(n) => patch((d) => ({ ...d, take_profit: n }))}
              />
              <NumberField
                reg={registry}
                part="stop_loss"
                label="SL"
                labelClass="font-semibold text-sell"
                unit="%"
                value={doc.stop_loss}
                disabled={locked}
                onChange={(n) => patch((d) => ({ ...d, stop_loss: n }))}
              />
            </div>
          </SellPane>
        )}
        {open === 'always' && (
          <SellPane>
            <div className="flex flex-col gap-1.5">
              <PartHeader ctx={sellCtx} part="always" />
              <LineList lines={doc.always} onChange={(always) => patch((d) => ({ ...d, always }))} ctx={sellCtx} stages={stageNames} addLabel="Always line" />
            </div>
          </SellPane>
        )}
        {focusId !== undefined && (
          <SellPane>
            <StagesEditor
              doc={doc}
              ctx={sellCtx}
              focusId={focusId}
              onChange={(stages) => patch((d) => ({ ...d, stages }))}
              onRename={(from, to) => patch((d) => renameStage(d, from, to))}
            />
          </SellPane>
        )}
      </div>

      <div className="flex flex-wrap items-end gap-3">
        <span className="w-12 shrink-0 pb-1.5 text-[12px] text-text-dim">Again</span>
        <label className="flex h-8 items-center gap-1.5 text-[11px] text-text-dim">
          <input
            type="checkbox"
            className="accent-accent"
            checked={doc.reentry != null}
            disabled={locked}
            onChange={(e) => patch((d) => ({ ...d, reentry: e.target.checked ? (d.reentry ?? { cooldown_sec: 5, max_per_coin: 10 }) : null }))}
          />
          {rulePart(registry, 'reentry')?.title ?? 'Buy again'}
          <InfoTooltip body={`${rulePart(registry, 'reentry')?.summary ?? ''}\n\nExample: ${rulePart(registry, 'reentry')?.example ?? ''}`} />
        </label>
        {doc.reentry && (
          <>
            <NumberField
              reg={registry}
              label="Wait after a sell"
              unit="s"
              placeholder=""
              value={doc.reentry.cooldown_sec}
              disabled={locked}
              className="w-20"
              onChange={(n) => patch((d) => ({ ...d, reentry: { cooldown_sec: n ?? NaN, max_per_coin: d.reentry?.max_per_coin ?? NaN } }))}
            />
            <NumberField
              reg={registry}
              label="Most buys per coin"
              integer
              placeholder=""
              value={doc.reentry.max_per_coin}
              disabled={locked}
              className="w-20"
              onChange={(n) => patch((d) => ({ ...d, reentry: { cooldown_sec: d.reentry?.cooldown_sec ?? NaN, max_per_coin: n ?? NaN } }))}
            />
          </>
        )}
        {!doc.reentry && <span className="pb-1.5 text-[12px] text-text-dim">off</span>}
      </div>
    </div>
  );
}
