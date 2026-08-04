import type { ColumnDef } from 'components/table/types';
import { TagChip } from './TagChip';
import type { StrategyRule } from 'lib/strategy/types';

export interface RuleTagsColumnOptions {
  /**
   * Click handler for a chip in a row. Rules gives this the tag filter
   * ("show only these"); tables with no filter bar omit it and render inert
   * chips. The cell stops propagation either way so a chip never also selects
   * the row.
   */
  onTagClick?: (tag: string) => void;
}

/**
 * The `tags` column, shared by every rule table (Rules, Rules Control, Simulate)
 * so a tag reads and filters identically wherever rules are listed — same shape
 * as `buildFingerprintRuleColumns` / `buildCapsColumns`.
 *
 * The filter-row grammar is space/comma-separated substring terms ANDed together,
 * with a `-` prefix to exclude — so `fam:` selects a whole namespace and
 * `-stage:experiment` hides a batch without touching the chip bar.
 */
export function buildRuleTagsColumn({
  onTagClick,
}: RuleTagsColumnOptions = {}): ColumnDef<StrategyRule> {
  return {
    key: 'tags',
    label: 'Tags',
    group: 'name',
    render: (r) => {
      const tags = r.tags ?? [];
      if (tags.length === 0) return <span className="text-text-dim">—</span>;
      return (
        <div className="flex flex-wrap items-center justify-center gap-1">
          {tags.map((tag) => (
            <TagChip
              key={tag}
              tag={tag}
              title={onTagClick ? `Show only rules tagged ${tag}` : tag}
              onClick={
                onTagClick
                  ? (e) => {
                      e.stopPropagation();
                      onTagClick(tag);
                    }
                  : undefined
              }
            />
          ))}
        </div>
      );
    },
    searchValue: (r) => (r.tags ?? []).join(' '),
    filterMatch: (r, raw) => {
      const tags = r.tags ?? [];
      const terms = raw.toLowerCase().split(/[\s,]+/).filter(Boolean);
      return terms.every((term) =>
        term.startsWith('-')
          ? !tags.some((t) => t.includes(term.slice(1)))
          : tags.some((t) => t.includes(term)),
      );
    },
    filterPlaceholder: 'tag… (-x hides)',
    filterTitle: 'Substring match per tag. Space/comma = AND, `-tag` excludes.',
    sortValue: (r) => (r.tags ?? []).join(' '),
    sortable: true,
  };
}
