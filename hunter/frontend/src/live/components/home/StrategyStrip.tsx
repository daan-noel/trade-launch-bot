import { useMemo } from 'react';
import { Link } from 'react-router-dom';
import { useSelector } from 'react-redux';
import { formatCompact } from 'utils/format';
import { floorHref, rulesHref } from 'lib/strategy/nav';
import { selectLiveOpen } from '@live/slices/liveStatusSlice';

interface RuleRow {
  ruleId: string;
  ruleName: string;
  open: number;
  deployedSol: number;
}

/** Compact per-rule real-money strip from Live Status SSOT (not a stale RTK refetch). */
export function StrategyStrip() {
  const openMap = useSelector(selectLiveOpen);

  const rows = useMemo<RuleRow[]>(() => {
    const byRule = new Map<string, RuleRow>();
    for (const p of Object.values(openMap)) {
      if (p.mode !== 'real') continue;
      if (!p.ruleId) continue;
      const row =
        byRule.get(p.ruleId) ??
        {
          ruleId: p.ruleId,
          ruleName: p.ruleName ?? p.ruleId.slice(0, 8),
          open: 0,
          deployedSol: 0,
        };
      row.open += 1;
      row.deployedSol += p.entrySol ?? 0;
      if (p.ruleName) row.ruleName = p.ruleName;
      byRule.set(p.ruleId, row);
    }
    return [...byRule.values()].sort((a, b) => b.open - a.open);
  }, [openMap]);

  return (
    <div className="rounded-lg border border-white/5 bg-white/2 p-3">
      <div className="mb-2 flex items-center justify-between">
        <h2 className="text-sm font-bold text-text">Real positions · by rule</h2>
        <Link
          to={floorHref({ tab: 'open' })}
          className="text-[11px] text-accent hover:text-primary hover:underline"
        >
          Floor →
        </Link>
      </div>
      {rows.length === 0 ? (
        <p className="py-4 text-center text-xs text-text-dim">No open real positions.</p>
      ) : (
        <div className="grid grid-cols-3 gap-2">
          {rows.map((r) => (
            <div
              key={r.ruleId}
              className="rounded-md border border-white/5 bg-white/2 px-2.5 py-1.5"
            >
              <Link
                to={floorHref({ tab: 'open', mode: 'real', ruleId: r.ruleId })}
                className="block transition hover:opacity-90"
              >
                <div className="truncate text-[11px] font-semibold text-text">{r.ruleName}</div>
                <div className="mt-0.5 flex items-baseline justify-between gap-1 text-xs tabular-nums">
                  <span className="text-primary">{r.open} open</span>
                  <span className="text-text-dim">◎{formatCompact(r.deployedSol, 2)}</span>
                </div>
              </Link>
              <Link
                to={rulesHref(r.ruleId)}
                className="mt-0.5 inline-block text-[10px] text-text-dim hover:text-accent"
              >
                Evidence →
              </Link>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
