import { useEffect, useState } from 'react';
import { Switch } from 'components/ui/Switch';
import { cn } from 'lib/cn';
import type { LogicCondition } from '@lab/lib/entryContext/logic';
import { HelpTip, Term } from './HelpText';
import { FILTER_LINE_HELP, type Help } from './summaryHelp';

/**
 * The Filters section: the buys table's filter conditions as chips, each one
 * editable (applies on Enter or when the box loses focus) and removable. New
 * conditions are added in the buys table's filter row. The chips sit on two
 * lines by `LogicCondition.needsIxs`: the ones only the In pool rows ask, and the
 * ones every row asks. The switch turns them all off and back on: off, every buy
 * counts as passing, so the page shows the pool.
 */
export function IdeaFilters({
  conditions,
  onEdit,
  on,
  onToggle,
}: {
  conditions: readonly LogicCondition[];
  /** New text for a column's condition; `''` removes it. */
  onEdit: (key: string, text: string) => void;
  on: boolean;
  onToggle: (on: boolean) => void;
}) {
  return (
    <div className="mb-3 flex flex-col gap-2 rounded-md border border-white/8 bg-white/2 p-2.5">
      <div className="flex flex-wrap items-center gap-2 text-[11px] text-text-dim">
        <span
          className="flex items-center gap-1.5"
          title="Off: the conditions are kept but not applied, so every buy in the pool passes."
        >
          <Switch checked={on} onChange={onToggle} label="Apply the filters" />
          {on ? 'On' : 'Off'}
        </span>
        {on && conditions.length === 0 && <span>Add a condition in the buys table's filter row.</span>}
      </div>
      <Line
        label="Target IXs"
        note="In pool rows only"
        help={FILTER_LINE_HELP.ix}
        conditions={conditions.filter((c) => c.needsIxs)}
        onEdit={onEdit}
        on={on}
      />
      <Line
        label="Any IXs"
        note="every row"
        help={FILTER_LINE_HELP.any}
        conditions={conditions.filter((c) => !c.needsIxs)}
        onEdit={onEdit}
        on={on}
      />
    </div>
  );
}

/** One kind of filter: its name, which summary rows ask it, and its chips. */
function Line({
  label,
  note,
  help,
  conditions,
  onEdit,
  on,
}: {
  label: string;
  note: string;
  help: Help;
  conditions: readonly LogicCondition[];
  onEdit: (key: string, text: string) => void;
  on: boolean;
}) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      <span className="flex w-44 shrink-0 items-center gap-1 text-[11px]">
        <Term>{label}</Term>
        <span className="text-text-dim">({note})</span>
        <HelpTip title={`${label} filters`} help={help} />
      </span>
      {conditions.length === 0 ? (
        <span className="text-[11px] text-text-dim">None.</span>
      ) : (
        <span className={cn('flex flex-wrap items-center gap-2', !on && 'opacity-50')}>
          {conditions.map((c) => (
            <Chip key={c.key} c={c} onEdit={onEdit} />
          ))}
        </span>
      )}
    </div>
  );
}

function Chip({ c, onEdit }: { c: LogicCondition; onEdit: (key: string, text: string) => void }) {
  const [draft, setDraft] = useState(c.text);
  useEffect(() => setDraft(c.text), [c.text]);
  const commit = () => {
    if (draft.trim() !== c.text) onEdit(c.key, draft.trim());
  };
  return (
    <span className="inline-flex h-7 items-center gap-2.5 rounded border border-primary/40 bg-primary/8 pl-2.5 pr-1.5 text-xs">
      <span className="text-text-dim">{c.label}</span>
      <input
        type="text"
        value={draft}
        aria-label={`${c.label} condition`}
        title="Edit, then Enter. Examples: >50, <=10, 1..5"
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') commit();
          if (e.key === 'Escape') setDraft(c.text);
        }}
        style={{ width: `calc(${Math.max(2, draft.length)}ch + 1rem + 2px)` }}
        className="h-full border-x border-primary/40 bg-primary/12 px-2 font-mono font-semibold text-primary outline-none focus:bg-primary/20"
      />
      <button
        type="button"
        onClick={() => onEdit(c.key, '')}
        aria-label={`Remove ${c.label}`}
        title="Remove this condition"
        className="px-1 text-text-dim hover:text-red"
      >
        x
      </button>
    </span>
  );
}
