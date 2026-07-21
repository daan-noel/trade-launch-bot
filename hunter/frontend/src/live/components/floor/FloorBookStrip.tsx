import { formatSigned, signedToneClass } from 'lib/signedTone';

export interface FloorBarRow {
  key: string;
  label: string;
  value: number;
}

/**
 * Compact horizontal bars for Floor page strips (open MTM by rule / recent PnL).
 */
export function FloorBookStrip({
  title,
  rows,
  empty,
}: {
  title: string;
  rows: FloorBarRow[];
  empty?: string;
}) {
  if (rows.length === 0) {
    return (
      <div className="rounded-lg border border-white/6 bg-white/[0.02] px-3 py-2 text-[11px] text-text-dim">
        {empty ?? `No ${title.toLowerCase()} yet.`}
      </div>
    );
  }

  const maxAbs = Math.max(...rows.map((r) => Math.abs(r.value)), 1e-9);

  return (
    <div className="rounded-lg border border-white/6 bg-white/[0.02] px-3 py-2">
      <div className="mb-1.5 text-[10px] font-semibold uppercase tracking-wider text-text-dim">
        {title}
      </div>
      <div className="flex flex-col gap-1">
        {rows.slice(0, 8).map((r) => {
          const w = Math.max(4, Math.round((Math.abs(r.value) / maxAbs) * 100));
          const pos = r.value >= 0;
          return (
            <div key={r.key} className="flex items-center gap-2 text-[11px]">
              <span className="w-24 shrink-0 truncate text-text-mid" title={r.label}>
                {r.label}
              </span>
              <div className="relative h-2 min-w-0 flex-1 rounded-sm bg-white/5">
                <div
                  className={`absolute inset-y-0 left-0 rounded-sm ${pos ? 'bg-green/70' : 'bg-red/70'}`}
                  style={{ width: `${w}%` }}
                />
              </div>
              <span className={`w-16 shrink-0 text-right tabular-nums ${signedToneClass(r.value)}`}>
                {formatSigned(r.value, 3)}◎
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
