import type { MouseEvent } from 'react';

import { CloseIcon } from 'components/ui/icons';
import { cn } from 'lib/cn';
import { tagChipStyle, type TagChipState } from 'lib/strategy/tags';

export interface TagChipProps {
  tag: string;
  /** Tri-state rendering for the filter bar. Plain chips leave this `off`. */
  state?: TagChipState;
  /** Rule count, shown after the label in the filter bar. */
  count?: number;
  onClick?: (e: MouseEvent<HTMLButtonElement>) => void;
  /** Renders an × affordance (editor input). */
  onRemove?: () => void;
  title?: string;
  className?: string;
}

/**
 * One rule-tag pill. Colour is hashed from the tag string (`tagChipStyle`), so
 * the same label looks identical on the filter bar, in a table row, and in the
 * editor without any stored per-tag colour.
 *
 * Tri-state: `include` brightens and rings the chip, `exclude` strikes it
 * through and drains the colour — legible without relying on hue alone.
 */
export function TagChip({
  tag,
  state = 'off',
  count,
  onClick,
  onRemove,
  title,
  className,
}: TagChipProps) {
  const interactive = Boolean(onClick);
  return (
    <span
      className={cn(
        'inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-[10px] font-semibold tracking-wide',
        state === 'off' && 'opacity-70',
        state === 'include' && 'ring-1 ring-inset ring-current',
        state === 'exclude' && 'line-through opacity-45 grayscale',
        className,
      )}
      style={tagChipStyle(tag)}
    >
      {interactive ? (
        <button type="button" onClick={onClick} title={title} className="cursor-pointer">
          {tag}
        </button>
      ) : (
        <span title={title}>{tag}</span>
      )}
      {count != null && <span className="opacity-60 tabular-nums">{count}</span>}
      {onRemove && (
        <button
          type="button"
          onClick={onRemove}
          title={`Remove ${tag}`}
          aria-label={`Remove ${tag}`}
          className="-mr-0.5 cursor-pointer opacity-60 hover:opacity-100"
        >
          <CloseIcon className="h-2.5 w-2.5" />
        </button>
      )}
    </span>
  );
}
