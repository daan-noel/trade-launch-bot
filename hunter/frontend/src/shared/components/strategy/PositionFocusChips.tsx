import { cn } from 'lib/cn';
import {
  positionFocusKey,
  positionFocusLabel,
  type PositionFocusLens,
} from 'lib/strategy/positionFocus';

interface PositionFocusChipsProps {
  lenses: readonly PositionFocusLens[];
  onRemove: (lens: PositionFocusLens) => void;
  onClearAll: () => void;
  className?: string;
}

/** Stacked focus chip strip — dismiss one or Clear all. Never touches table filters. */
export function PositionFocusChips({
  lenses,
  onRemove,
  onClearAll,
  className,
}: PositionFocusChipsProps) {
  if (lenses.length === 0) return null;
  return (
    <div
      className={cn(
        'mb-3 flex flex-wrap items-center gap-1.5 rounded-md border border-primary/25 bg-primary/5 px-2.5 py-1.5',
        className,
      )}
      role="status"
      aria-label="Active focus filters"
    >
      <span className="text-[10px] font-bold uppercase tracking-wider text-primary">Focus</span>
      {lenses.map((lens) => (
        <button
          key={positionFocusKey(lens)}
          type="button"
          onClick={() => onRemove(lens)}
          title="Remove this focus"
          className="inline-flex items-center gap-1 rounded-md bg-primary/15 px-2 py-0.5 text-[11px] font-semibold text-primary hover:bg-primary/25"
        >
          {positionFocusLabel(lens)}
          <span aria-hidden className="text-primary/70">
            ×
          </span>
        </button>
      ))}
      <button
        type="button"
        onClick={onClearAll}
        className="ml-auto text-[11px] font-semibold text-text-dim hover:text-text"
      >
        Clear all
      </button>
    </div>
  );
}
