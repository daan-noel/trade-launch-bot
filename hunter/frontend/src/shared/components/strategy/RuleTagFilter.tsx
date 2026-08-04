import { useMemo } from 'react';

import { TagChip } from './TagChip';
import {
  cycleTag,
  isEmptyTagFilter,
  tagChipState,
  tagCounts,
  tagNamespace,
  type TagFilterState,
} from 'lib/strategy/tags';

export interface RuleTagFilterProps {
  /** The rules the counts describe — pass the set filtered by everything EXCEPT
   *  the tag filter, so a chip's count doesn't collapse the moment you click it. */
  rules: { tags?: string[] }[];
  filter: TagFilterState;
  onChange: (next: TagFilterState) => void;
}

/**
 * Tri-state tag chip bar over the Rules list. One click cycles a chip
 * off → include → exclude → off; include chips OR, exclude chips hide. Chips are
 * grouped by namespace (`fam:` / `src:` / …) with bare labels last, which is what
 * keeps a grown tag set navigable.
 *
 * Renders nothing when no rule is tagged, so the header doesn't grow an empty
 * control on a fresh install.
 */
export function RuleTagFilter({ rules, filter, onChange }: RuleTagFilterProps) {
  const counts = useMemo(() => tagCounts(rules), [rules]);

  // Chips still selected but no longer present in the visible set (e.g. every
  // rule carrying them is disabled) must stay clickable or the filter is
  // un-clearable from the bar.
  const orphaned = useMemo(() => {
    const present = new Set(counts.map((c) => c.tag));
    return [...filter.include, ...filter.exclude]
      .filter((t) => !present.has(t))
      .map((tag) => ({ tag, count: 0 }));
  }, [counts, filter]);

  const all = useMemo(() => [...counts, ...orphaned], [counts, orphaned]);
  if (all.length === 0) return null;

  const groups = new Map<string, typeof all>();
  for (const entry of all) {
    const key = tagNamespace(entry.tag) ?? '';
    const bucket = groups.get(key);
    if (bucket) bucket.push(entry);
    else groups.set(key, [entry]);
  }

  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
      {[...groups.entries()].map(([ns, entries]) => (
        <div key={ns || '_bare'} className="flex flex-wrap items-center gap-1">
          {ns && <span className="text-[10px] uppercase tracking-wide text-text-dim/70">{ns}</span>}
          {entries.map(({ tag, count }) => (
            <TagChip
              key={tag}
              tag={tag}
              count={count}
              state={tagChipState(filter, tag)}
              onClick={() => onChange(cycleTag(filter, tag))}
              title={
                {
                  off: `Show only rules tagged ${tag}`,
                  include: `Hide rules tagged ${tag}`,
                  exclude: `Clear the ${tag} filter`,
                }[tagChipState(filter, tag)]
              }
            />
          ))}
        </div>
      ))}
      {!isEmptyTagFilter(filter) && (
        <button
          type="button"
          onClick={() => onChange({ include: [], exclude: [] })}
          className="cursor-pointer rounded-md px-1.5 py-0.5 text-[10px] text-text-dim hover:bg-white/8 hover:text-text"
        >
          clear tags
        </button>
      )}
    </div>
  );
}
