// A staged owner-split row's pins as chips: the auto-tag list and its guide render
// the same component, so the guide's "read a row" key is the list's own picture.

import { cn } from 'lib/cn';
import { coreMarksHelp } from 'lib/strategy/ixCore';
import { FEE_FIELD_HELP, type IxPatternFeeField, type IxPatternRow } from 'lib/strategy/ixPatternRows';
import { EXACT_LEVEL, MATCH_LEVELS } from './autoTagRules';

const levelHelp = (level: number) =>
  `Match level ${level} of ${EXACT_LEVEL} (1 loosest, ${EXACT_LEVEL} strictest). This row pins ${MATCH_LEVELS[level - 1]?.help ?? 'an unknown set of fields'}; ` +
  'anything it does not pin matches any value.\n\n' +
  (level === EXACT_LEVEL
    ? 'Exact: every instruction in order, nothing added or dropped.'
    : 'Core: the ix list without the extras a sender adds, drops or moves (compute budget, nonce, memo, Lighthouse, system transfers), verbs merged, so it matches every variant of the build.');

export const FEE_PINS: { field: IxPatternFeeField; label: string }[] = [
  { field: 'cu_limit', label: 'CU limit' },
  { field: 'cu_price', label: 'CU price' },
  { field: 'tip_lamports', label: 'tip' },
];

/** One labelled pin: a dim name and its value, with its own tooltip. */
function Pin({ name, value, help, className }: { name?: string; value: string; help: string; className: string }) {
  return (
    <span className={cn('inline-flex cursor-help items-center gap-1 rounded border px-1.5 py-px font-mono text-[10px] leading-4', className)} title={help}>
      {name && <span className="font-sans text-[9px] uppercase tracking-wide opacity-60">{name}</span>}
      <span className="font-semibold">{value}</span>
    </span>
  );
}

/** A staged row's pins, one chip each: level, side, marks, then the fee fields. */
export function RowPins({ level, row }: { level: number; row: IxPatternRow }) {
  return (
    <div className="flex flex-wrap items-center gap-1">
      <Pin
        value={level === EXACT_LEVEL ? `EXACT L${level}` : `CORE L${level}`}
        help={levelHelp(level)}
        className="border-accent/40 bg-accent/15 text-accent"
      />
      {row.side && (
        <Pin
          value={row.side.toUpperCase()}
          help={`Side: matches ${row.side} trades only.`}
          className={row.side === 'buy' ? 'border-green/40 bg-green/10 text-green' : 'border-red/40 bg-red/10 text-red'}
        />
      )}
      {row.marks && (
        <Pin
          name="marks"
          value={row.marks}
          help={`Marks: the extras around the core, pinned as a set.\n\n${coreMarksHelp(row.marks).join('\n')}`}
          className="border-white/15 bg-white/5 text-text"
        />
      )}
      {FEE_PINS.map(({ field, label }) => {
        const v = row[field];
        if (v == null) return null;
        return (
          <Pin
            key={field}
            name={label}
            value={v.toLocaleString()}
            help={`${label}: pinned at ${v.toLocaleString()}.\n\n${FEE_FIELD_HELP[field]}`}
            className="border-warning/40 bg-warning/10 text-warning"
          />
        );
      })}
    </div>
  );
}
