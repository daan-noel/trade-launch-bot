// Stages: the steps after the buy. The position starts in the first stage; each stage
// has its own lines and may have a deadline, at which its at-deadline lines run once
// and the rule moves on.

import { Button } from 'components/ui/Button';
import { IconButton } from 'components/ui/IconButton';
import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { CloseIcon, PlusIcon } from 'components/ui/icons';
import { freshName, newStage, type DeadlineBasis, type RuleDoc, type Stage } from 'lib/strategy/ruleDoc';
import { deadlineSentence, stageNext } from 'lib/strategy/sentences';
import { nameError } from 'lib/strategy/validate';
import { LineList, PartHeader } from './parts';
import type { CondContext } from './CondRow';

const BASIS_LABEL: Record<DeadlineBasis, string> = {
  age_sec: 'coin age reaches',
  held_sec: 'time held reaches',
  stage_sec: 'time in this stage reaches',
};

export function StagesEditor({
  doc,
  onChange,
  onRename,
  ctx,
}: {
  doc: RuleDoc;
  onChange: (stages: Stage[]) => void;
  onRename: (from: string, to: string) => void;
  ctx: CondContext;
}) {
  const stages = doc.stages;
  const names = stages.map((s) => s.name);
  const set = (i: number, s: Stage) => onChange(stages.map((x, j) => (j === i ? s : x)));
  const move = (i: number, d: -1 | 1) => {
    const j = i + d;
    if (j < 0 || j >= stages.length) return;
    const next = [...stages];
    [next[i], next[j]] = [next[j], next[i]];
    onChange(next);
  };
  return (
    <div className="flex flex-col gap-2">
      <PartHeader ctx={ctx} part="stages">
        <Button variant="subtle" size="xs" disabled={ctx.disabled} onClick={() => onChange([...stages, newStage(freshName(stages.length ? 'stage' : 'start', names))])}>
          <PlusIcon className="size-3" /> stage
        </Button>
      </PartHeader>
      {stages.length === 0 ? (
        <p className="text-[11px] italic text-text-dim/70">
          No stages: after the buy, only the Always lines, take profit and stop loss can sell.
        </p>
      ) : (
        <StageStrip stages={stages} />
      )}
      {stages.map((s, i) => (
        <StageCard
          key={s.id}
          stage={s}
          index={i}
          stages={stages}
          ctx={ctx}
          onChange={(n) => set(i, n)}
          onRename={(to) => onRename(s.name, to)}
          onRemove={() => onChange(stages.filter((_, j) => j !== i))}
          onMove={stages.length > 1 ? (d) => move(i, d) : undefined}
        />
      ))}
    </div>
  );
}

/** `buy -> early (until age 20 s) -> late -> ride`. */
function StageStrip({ stages }: { stages: Stage[] }) {
  return (
    <div className="flex flex-wrap items-center gap-1 text-[11px]">
      <span className="rounded bg-white/5 px-1.5 py-0.5 text-text-dim">buy</span>
      {stages.map((s, i) => (
        <span key={s.id} className="flex items-center gap-1">
          <span className="text-text-dim">→</span>
          <span className="rounded bg-accent/15 px-1.5 py-0.5 font-mono text-text" title={s.ends ? `ends ${deadlineSentence(s.ends)}, then ${stageNext(stages, i) ?? '?'}` : 'no deadline'}>
            {s.name}
            {s.ends && <span className="text-text-dim"> ⏱{s.ends.secs}s</span>}
          </span>
        </span>
      ))}
    </div>
  );
}

function StageCard({
  stage,
  index,
  stages,
  ctx,
  onChange,
  onRename,
  onRemove,
  onMove,
}: {
  stage: Stage;
  index: number;
  stages: Stage[];
  ctx: CondContext;
  onChange: (s: Stage) => void;
  onRename: (to: string) => void;
  onRemove: () => void;
  onMove?: (d: -1 | 1) => void;
}) {
  const names = stages.map((s) => s.name);
  const nErr = nameError(stage.name, 'Name');
  const nextDefault = stages[index + 1]?.name;
  const inner: CondContext = { ...ctx, beforeBuy: false };
  return (
    <div className="flex flex-col gap-2 rounded-md border border-accent/25 bg-white/2 px-2 py-2">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-[11px] font-semibold text-text-dim">Stage {index + 1}</span>
        <Input fieldSize="sm" className="w-36 font-mono" value={stage.name} disabled={ctx.disabled} onChange={(e) => onRename(e.target.value)} />
        {index === 0 && <span className="text-[11px] text-text-dim">the position starts here</span>}
        <div className="ml-auto flex items-center gap-1">
          {onMove && (
            <>
              <IconButton variant="ghost" size="sm" disabled={ctx.disabled} onClick={() => onMove(-1)} title="Move up (the first stage is where the position starts)">
                ↑
              </IconButton>
              <IconButton variant="ghost" size="sm" disabled={ctx.disabled} onClick={() => onMove(1)} title="Move down">
                ↓
              </IconButton>
            </>
          )}
          <IconButton variant="ghost" size="sm" disabled={ctx.disabled} onClick={onRemove} title="Remove stage" aria-label="Remove stage">
            <CloseIcon />
          </IconButton>
        </div>
      </div>
      {nErr && <p className="text-[11px] text-red">{nErr}</p>}

      {/* Deadline */}
      <div className="flex flex-wrap items-center gap-2 text-[11px] text-text-dim">
        <PartHeader ctx={ctx} part="stage.ends" />
      </div>
      <div className="flex flex-wrap items-center gap-2 text-[11px] text-text-dim">
        <Select
          className="w-52"
          value={stage.ends?.basis ?? ''}
          disabled={ctx.disabled}
          onChange={(e) => {
            const b = e.target.value as DeadlineBasis | '';
            onChange(b ? { ...stage, ends: { basis: b, secs: stage.ends?.secs ?? 20 } } : { ...stage, ends: null, then: null });
          }}
        >
          <option value="">no deadline</option>
          {(Object.keys(BASIS_LABEL) as DeadlineBasis[]).map((b) => (
            <option key={b} value={b}>
              ends when {BASIS_LABEL[b]}
            </option>
          ))}
        </Select>
        {stage.ends && (
          <>
            <Input
              fieldSize="sm"
              className="w-20"
              numeric
              unit="s"
              numericValue={stage.ends.secs}
              disabled={ctx.disabled}
              onNumericChange={(n) => stage.ends && onChange({ ...stage, ends: { ...stage.ends, secs: n ?? NaN } })}
            />
            <span>then go to</span>
            <Select
              className="w-40"
              value={stage.then ?? ''}
              disabled={ctx.disabled}
              onChange={(e) => onChange({ ...stage, then: e.target.value || null })}
            >
              <option value="">{nextDefault ? `next stage (${nextDefault})` : 'pick a stage...'}</option>
              {stage.then && !names.includes(stage.then) && <option value={stage.then}>{stage.then} (missing)</option>}
              {names.map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </Select>
          </>
        )}
      </div>

      <div className="flex flex-col gap-1.5">
        <PartHeader ctx={ctx} part="stage.on" />
        <LineList lines={stage.on} onChange={(on) => onChange({ ...stage, on })} ctx={inner} stages={names} />
      </div>
      {(stage.ends || stage.at_end.length > 0) && (
        <div className="flex flex-col gap-1.5">
          <PartHeader ctx={ctx} part="stage.at_end" />
          <LineList lines={stage.at_end} onChange={(at_end) => onChange({ ...stage, at_end })} ctx={inner} stages={names} checkpoint addLabel="at-deadline line" />
        </div>
      )}
    </div>
  );
}
