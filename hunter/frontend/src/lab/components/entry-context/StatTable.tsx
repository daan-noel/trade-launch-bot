import type { ReactNode } from 'react';

export interface StatColumn {
  label: string;
  /** Hover text on the header. */
  tip?: string;
  className?: string;
}

export interface StatRow {
  key: string;
  label: string;
  /** Hover text on the row name. */
  tip?: string;
  cells: ReactNode[];
}

/** A small summary table: a row name (hover explains it), then right-aligned numbers. */
export function StatTable({ columns, rows }: { columns: readonly StatColumn[]; rows: readonly StatRow[] }) {
  return (
    <table className="w-full text-xs">
      <thead>
        <tr className="text-[10px] uppercase tracking-wider text-text-dim">
          <th className="pb-1 text-left font-bold" />
          {columns.map((c) => (
            <th key={c.label} className={`pb-1 pr-3 text-right font-bold ${c.className ?? ''}`} title={c.tip}>
              {c.label}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {rows.map((r) => (
          <tr key={r.key} className="border-t border-white/6">
            <td className="py-1 pr-3 text-text-dim" title={r.tip}>
              <span className={r.tip ? 'decoration-dotted underline-offset-4 hover:underline' : undefined}>
                {r.label}
              </span>
            </td>
            {r.cells.map((cell, i) => (
              <td key={i} className="py-1 pr-3 text-right font-mono tabular-nums">
                {cell}
              </td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** The one-line explanation every summary card shows under its title. */
export function CardIntro({ children }: { children: ReactNode }) {
  return <p className="text-[11px] leading-snug text-text-dim">{children}</p>;
}
