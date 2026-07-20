import { useEffect, useState } from 'react';
import { connectActionProgressStream, type ActionProgress } from 'services/sse';

/**
 * Global footer strip for in-flight stop/sell actions pushed over `action_progress`.
 * Survives navigation (mounted in the live App shell) and mirrors across tabs via SSE.
 * Renders nothing when idle.
 */
export function RunningTasksIndicator() {
  const [tasks, setTasks] = useState<Record<string, ActionProgress>>({});

  useEffect(() => {
    const h = connectActionProgressStream((p) => {
      setTasks((prev) => {
        const next = { ...prev };
        if (p.status === 'done' || p.status === 'partial' || p.status === 'failed') {
          delete next[p.action_id];
        } else {
          next[p.action_id] = p;
        }
        return next;
      });
    });
    return () => h.close();
  }, []);

  const list = Object.values(tasks);
  if (list.length === 0) return null;

  return (
    <div className="sticky bottom-0 z-20 border-t border-white/10 bg-surface/95 px-4 py-2 text-[12px] shadow-lg backdrop-blur">
      <div className="flex flex-wrap gap-3">
        {list.map((t) => (
          <span key={t.action_id} className="tabular-nums text-amber-400">
            {t.kind === 'stop' ? 'Stopping' : 'Working'} {t.done}/{t.total}
            {t.rule_id ? ` · rule ${t.rule_id.slice(0, 8)}` : ''}
          </span>
        ))}
      </div>
    </div>
  );
}
