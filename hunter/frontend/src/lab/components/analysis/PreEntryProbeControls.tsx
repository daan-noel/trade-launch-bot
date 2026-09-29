import { Badge } from 'components/ui/Badge';
import { Input } from 'components/ui/Input';
import { Switch } from 'components/ui/Switch';
import { cn } from 'lib/cn';
import type { PreEntryShow, ProbeControlsModel } from './usePreEntryProbe';

const SHOW_OPTIONS: { value: PreEntryShow; label: string; title: string }[] = [
  {
    value: 'all',
    label: 'All',
    title: 'Every row. Keep this to see the misses too: they tell whether the hits mean anything.',
  },
  {
    value: 'matched',
    label: 'Before',
    title: 'Only rows where the lens structure traded in the window before his entry. The summary still counts every row.',
  },
  {
    value: 'no-match',
    label: 'Absent',
    title: 'Only rows where it did not trade in the window before his entry.',
  },
  {
    value: 'unknown',
    label: 'Unknown',
    title: 'Only rows the data cannot answer: too old for the stored history, or missing fee data the pattern set needs.',
  },
];

/**
 * The **pre-entry probe** strip under the flow lens: does a structure from this
 * set land on the tape before the trader's entry, across every token at once.
 *
 * The lens supplies the SET (and its narrowing, its side, the wallet it
 * excludes); this row only carries the question asked with it — how far back to
 * look, and how much has to be there.
 *
 * The summary is the point of the strip, not decoration. A presence count with
 * no denominator is not evidence, so the control window's count sits beside the
 * match count and the unknowns are named — a filter alone can only ever show
 * confirmations.
 */
export function PreEntryProbeControls({ probe }: { probe: ProbeControlsModel }) {
  return (
    <div className="mt-2 flex flex-wrap items-center gap-2 border-t border-white/8 pt-2">
      <Badge variant={probe.on ? 'accent' : 'neutral'} size="sm">
        Pre-entry
      </Badge>

      <label className="flex items-center gap-1.5 text-[11px] text-text-dim">
        <Switch checked={probe.on} onChange={probe.setOn} label="Probe pre-entry structures" />
        <span title="For each row: did the lens structure trade in the slots right before his entry? Adds the Pre-entry columns. Use Show to keep one answer.">
          Probe
        </span>
      </label>

      <ShowControl probe={probe} />

      <NumberKnob
        label="Window"
        suffix={probe.window.suffix}
        title={probe.window.title}
        value={probe.window.value}
        min={probe.window.min}
        max={probe.window.max}
        step={probe.window.step}
        onChange={probe.window.set}
        disabled={!probe.on}
      />
      <NumberKnob
        label="Min hits"
        title="How many of its transactions the window needs to count as Before. 1 = at least one. Raise it to require a burst."
        value={probe.minHits}
        min={1}
        max={999}
        step={1}
        onChange={probe.setMinHits}
        disabled={!probe.on}
      />
      <NumberKnob
        label="Min SOL"
        title="How much SOL its transactions need to move to count as Before. 0 = any amount."
        value={probe.minSol}
        min={0}
        max={10_000}
        step={0.1}
        onChange={probe.setMinSol}
        disabled={!probe.on}
      />

      {probe.on && (
        <>
          <span className="mx-1 h-4 w-px bg-white/10" />
          {probe.loading ? (
            <span className="text-[11px] text-text-dim">Probing the tape…</span>
          ) : probe.error ? (
            <span className="text-[11px] text-red">{probe.error}</span>
          ) : probe.blocked ? (
            <span className="text-[11px] text-text-dim">Idle — {probe.blocked}</span>
          ) : (
            probe.summary && (
              <span
                className="font-mono text-[11px] text-text"
                title="Before: rows where the target traded right before his buy. Earlier: rows where it also traded in the same number of slots just before that. Earlier close to Before = the target is always there, not tied to his buy."
              >
                {probe.summary}
              </span>
            )
          )}
        </>
      )}
    </div>
  );
}

/** Which verdict the TABLE keeps. A row filter, not a re-probe: the answers are
 *  already in hand, so every click is instant and the summary — computed over
 *  every probed row — does not move with it. */
function ShowControl({ probe }: { probe: ProbeControlsModel }) {
  return (
    <div className="flex items-center gap-1">
      <span className="text-[9px] font-bold uppercase tracking-widest text-text-dim">Show</span>
      <div className="flex overflow-hidden rounded-md border border-white/10">
        {SHOW_OPTIONS.map((o) => {
          const on = probe.show === o.value;
          return (
            <button
              key={o.value}
              type="button"
              disabled={!probe.on}
              onClick={() => probe.setShow(o.value)}
              title={o.title}
              className={cn(
                'px-2 py-0.5 text-[11px] transition-colors',
                !probe.on
                  ? 'text-text-dim/40'
                  : on
                    ? 'bg-accent/20 text-accent'
                    : 'text-text-dim hover:text-text',
              )}
            >
              {o.label}
              {probe.on && probe.counts && (
                <span className="ml-1 font-mono text-[9px] opacity-60">{probe.counts[o.value]}</span>
              )}
            </button>
          );
        })}
      </div>
    </div>
  );
}

/** One numeric knob. Blank input is left alone rather than coerced to a number:
 *  typing over a value clears it for an instant, and coercing there would fire a
 *  probe for a window nobody asked for. */
function NumberKnob({
  label,
  suffix,
  title,
  value,
  min,
  max,
  step,
  onChange,
  disabled,
}: {
  label: string;
  suffix?: string;
  title: string;
  value: number;
  min: number;
  max: number;
  step: number;
  onChange: (n: number) => void;
  disabled?: boolean;
}) {
  return (
    <label className="flex items-center gap-1 text-[11px] text-text-dim" title={title}>
      <span className="text-[9px] font-bold uppercase tracking-widest">{label}</span>
      <Input
        type="number"
        fieldSize="sm"
        value={String(value)}
        min={min}
        max={max}
        step={step}
        disabled={disabled}
        onChange={(e) => {
          const raw = e.target.value;
          if (raw === '') return;
          const n = Number(raw);
          if (Number.isFinite(n)) onChange(Math.min(Math.max(n, min), max));
        }}
        className="w-[72px] font-mono"
      />
      {suffix && <span>{suffix}</span>}
    </label>
  );
}
