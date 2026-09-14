import { useEffect, useRef, useState } from 'react';
import {
  ACTION_REANNOUNCE_GRACE_MS,
  connectActionProgressStream,
  type ActionProgress,
} from 'services/sse';

/**
 * Global footer strip for in-flight stop/sell actions pushed over `action_progress`.
 * Survives navigation (mounted in the live App shell) and mirrors across tabs via SSE.
 * Renders nothing when idle.
 */
export function RunningTasksIndicator() {
  const [tasks, setTasks] = useState<Record<string, ActionProgress>>({});
  /** Last frame per action (ms) — a reconnect drops the ones not re-heard since. */
  const seen = useRef(new Map<string, number>());

  useEffect(() => {
    let sweep: ReturnType<typeof setTimeout> | undefined;
    const h = connectActionProgressStream(
      (p) => {
        const terminal = p.status === 'done' || p.status === 'partial' || p.status === 'failed';
        if (terminal) seen.current.delete(p.action_id);
        else seen.current.set(p.action_id, Date.now());
        setTasks((prev) => {
          const next = { ...prev };
          if (terminal) delete next[p.action_id];
          else next[p.action_id] = p;
          return next;
        });
      },
      () => {
        const at = Date.now();
        clearTimeout(sweep);
        sweep = setTimeout(() => {
          setTasks((prev) => {
            const next: Record<string, ActionProgress> = {};
            for (const [id, t] of Object.entries(prev)) {
              if ((seen.current.get(id) ?? 0) >= at) next[id] = t;
              else seen.current.delete(id);
            }
            return next;
          });
        }, ACTION_REANNOUNCE_GRACE_MS);
      },
    );
    return () => {
      h.close();
      clearTimeout(sweep);
    };
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
