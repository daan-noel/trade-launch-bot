import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';
import { useDispatch } from 'react-redux';
import {
  connectSimulationFinished,
  connectSweepFinished,
  sseSubscribe,
} from 'services/sse';
import { cancelGroupedSweep, cancelSimulation, getJobsStatus } from 'services/api';
import { apiSlice } from 'store/apiSlice';
import type { AppDispatch } from '../store';
import type { SimulationProgressEvent, SweepProgressEvent } from 'types';

/**
 * App-wide registry of long-running background jobs (grouped sweep, rule
 * simulation) and their live progress.
 *
 * Why a context (not page state): a sweep/backtest runs entirely on the backend
 * inside its still-open HTTP request — navigating away or refreshing the SPA
 * never stops it, but page-local `isLoading` + the page's SSE subscription die
 * with the unmounted page, so the UI loses the running job. Mounted once at the
 * app root, this provider:
 *   • keeps ONE set of SSE subscriptions alive for the whole session, so it sees
 *     every `*_progress` / `*_finished` frame regardless of the active route;
 *   • seeds itself from `GET /api/jobs/status` on first load, recovering a job
 *     that was already running before the page (re)loaded — SSE only replays
 *     future frames;
 *   • drives a global indicator (and any page's controls) from that single
 *     source of truth, so progress survives navigation.
 *
 * Keys: the sweep is single-flight → one fixed `'sweep'` slot. Simulations are
 * per-rule → keyed by `rule_id`. The map is only touched on sweep/sim SSE frames
 * (never on the SOL/USD or live-trade streams), so consumers don't re-render on
 * the high-frequency ticks.
 */
export type JobKind = 'sweep' | 'simulation';

export interface BackgroundJob {
  kind: JobKind;
  /** Map key + cancel target: the fixed `'sweep'` slot, or a simulation's rule_id. */
  id: string;
  /** Human label for the indicator (rule name when known, else a generic name). */
  label: string;
  /** Null until the first progress frame declares the total (indeterminate bar). */
  processed: number | null;
  total: number | null;
  /** A cancel has been requested but the cooperative abort hasn't landed yet. */
  cancelling: boolean;
}

/** Singleton key for the single-flight grouped sweep. */
const SWEEP_KEY = 'sweep';

interface BackgroundJobsValue {
  jobs: BackgroundJob[];
  /** Optimistically register a job the moment its request is fired, so the
   *  indicator shows before the first SSE frame (which only arrives once the
   *  backend finishes selecting/partitioning candidates). */
  markStarting: (kind: JobKind, id: string, label: string) => void;
  /** Whether a job of this kind/id is currently tracked (running). */
  isRunning: (kind: JobKind, id: string) => boolean;
  cancel: (job: BackgroundJob) => void;
}

const BackgroundJobsContext = createContext<BackgroundJobsValue | null>(null);

const keyOf = (kind: JobKind, id: string) => (kind === 'sweep' ? SWEEP_KEY : id);

export function BackgroundJobsProvider({ children }: { children: ReactNode }) {
  const dispatch = useDispatch<AppDispatch>();
  const [jobs, setJobs] = useState<Map<string, BackgroundJob>>(new Map());

  /** Insert or update one job, preserving fields the caller didn't supply. */
  const upsert = useCallback((kind: JobKind, id: string, patch: Partial<BackgroundJob>) => {
    setJobs((prev) => {
      const key = keyOf(kind, id);
      const existing = prev.get(key);
      const next = new Map(prev);
      next.set(key, {
        kind,
        id,
        label:
          patch.label ?? existing?.label ?? (kind === 'sweep' ? 'Grouped sweep' : 'Simulation'),
        processed: patch.processed !== undefined ? patch.processed : existing?.processed ?? null,
        total: patch.total !== undefined ? patch.total : existing?.total ?? null,
        cancelling: patch.cancelling ?? existing?.cancelling ?? false,
      });
      return next;
    });
  }, []);

  const remove = useCallback((kind: JobKind, id: string) => {
    setJobs((prev) => {
      const key = keyOf(kind, id);
      if (!prev.has(key)) return prev;
      const next = new Map(prev);
      next.delete(key);
      return next;
    });
  }, []);

  // One set of subscriptions for the whole session; seed from the status snapshot
  // so a job already running at load time is recovered. The progress streams are
  // subscribed raw (no id filter) so the registry sees every job's frames.
  useEffect(() => {
    let alive = true;
    getJobsStatus()
      .then((status) => {
        if (!alive) return;
        if (status.sweep) {
          upsert('sweep', SWEEP_KEY, {
            processed: status.sweep.processed,
            total: status.sweep.total,
          });
        }
        for (const s of status.simulations) {
          upsert('simulation', s.rule_id, { processed: s.processed, total: s.total });
        }
      })
      .catch(() => {
        /* no jobs / backend down — nothing to recover */
      });

    const offSweepProgress = sseSubscribe('sweep_progress', (e) => {
      if (typeof e.data !== 'string') return;
      try {
        const p = JSON.parse(e.data) as SweepProgressEvent;
        upsert('sweep', SWEEP_KEY, { processed: p.processed, total: p.total });
      } catch {
        /* ignore malformed frames */
      }
    });
    const offSimProgress = sseSubscribe('simulation_progress', (e) => {
      if (typeof e.data !== 'string') return;
      try {
        const p = JSON.parse(e.data) as SimulationProgressEvent;
        upsert('simulation', p.rule_id, { processed: p.processed, total: p.total });
      } catch {
        /* ignore malformed frames */
      }
    });
    const sweepFinished = connectSweepFinished(() => {
      remove('sweep', SWEEP_KEY);
      // A finished sweep persisted a new run — refresh the runs list app-wide.
      dispatch(apiSlice.util.invalidateTags(['GroupedSweep']));
    });
    const simFinished = connectSimulationFinished((ev) => remove('simulation', ev.rule_id));

    return () => {
      alive = false;
      offSweepProgress();
      offSimProgress();
      sweepFinished.close();
      simFinished.close();
    };
  }, [upsert, remove, dispatch]);

  const markStarting = useCallback(
    (kind: JobKind, id: string, label: string) => {
      upsert(kind, id, { label, processed: null, total: null, cancelling: false });
    },
    [upsert],
  );

  const isRunning = useCallback((kind: JobKind, id: string) => jobs.has(keyOf(kind, id)), [jobs]);

  const cancel = useCallback(
    (job: BackgroundJob) => {
      upsert(job.kind, job.id, { cancelling: true });
      const req = job.kind === 'sweep' ? cancelGroupedSweep() : cancelSimulation(job.id);
      req.catch(() => upsert(job.kind, job.id, { cancelling: false }));
    },
    [upsert],
  );

  const value = useMemo<BackgroundJobsValue>(
    () => ({ jobs: Array.from(jobs.values()), markStarting, isRunning, cancel }),
    [jobs, markStarting, isRunning, cancel],
  );

  return (
    <BackgroundJobsContext.Provider value={value}>{children}</BackgroundJobsContext.Provider>
  );
}

export function useBackgroundJobs() {
  const ctx = useContext(BackgroundJobsContext);
  if (!ctx) throw new Error('useBackgroundJobs must be used within BackgroundJobsProvider');
  return ctx;
}
