import { TagChip } from './TagChip';

/**
 * Rule labels drawn under the name. A chip click filters the table; the
 * container swallows the click so it never also selects the row.
 */
export function RuleLabels({
  tags,
  onTagClick,
}: {
  tags: readonly string[] | null | undefined;
  onTagClick?: (tag: string) => void;
}) {
  if (!tags || tags.length === 0) return null;
  return (
    <div
      className="flex max-w-56 flex-wrap items-center justify-center gap-1"
      onClick={(e) => e.stopPropagation()}
    >
      {tags.map((tag) => (
        <TagChip
          key={tag}
          tag={tag}
          title={onTagClick ? `Show only rules labelled ${tag}` : tag}
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
}
