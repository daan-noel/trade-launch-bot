// Compact multi-key sort header: a column title + a row of per-axis toggles
// that drive `SortCtx.toggleSort` (used by fingerprint / params merged cells).

import type { ReactNode } from 'react';

import { cn } from 'lib/cn';

import type { SortCtx } from './types';

export type MultiSortAxis = { key: string; label: string; title?: string };

function SortToggle({
  label,
  sortKey,
  title,
  ctx,
}: {
  label: string;
  sortKey: string;
  title?: string;
  ctx: SortCtx;
}) {
  const idx = ctx.sortKeys.findIndex((s) => s.col === sortKey);
  const entry = idx >= 0 ? ctx.sortKeys[idx] : null;
  return (
    <button
      type="button"
      title={title}
      className={cn(
        'rounded px-1 py-0.5 text-[10px] font-semibold uppercase tracking-wider',
        entry ? 'text-accent' : 'text-primary/70 hover:text-accent',
      )}
      onClick={(e) => {
        e.stopPropagation();
        ctx.toggleSort(sortKey);
      }}
    >
      {label}
      {entry && (
        <span className="ml-0.5 text-[9px] normal-case tracking-normal">
          {ctx.sortKeys.length > 1 && <span className="opacity-50">{idx + 1}</span>}
          {entry.dir === 'asc' ? '↑' : '↓'}
        </span>
      )}
    </button>
  );
}

/** Title + single-line axis toggles for a merged multi-sort column header. */
export function MultiSortHeader({
  title,
  axes,
  ctx,
}: {
  title: string;
  axes: readonly MultiSortAxis[];
  ctx: SortCtx;
}): ReactNode {
  return (
    <div className="flex flex-col items-start gap-0.5 whitespace-nowrap">
      <span>{title}</span>
      <div className="flex flex-nowrap items-center gap-x-0.5 font-normal normal-case tracking-normal">
        {axes.map((axis) => (
          <SortToggle key={axis.key} label={axis.label} sortKey={axis.key} title={axis.title} ctx={ctx} />
        ))}
      </div>
    </div>
  );
}
