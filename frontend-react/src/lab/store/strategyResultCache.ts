import { labApi, type StrategyRuleArg } from './labEndpoints';
import { store, type AppDispatch } from '@lab/store';
import { getJobsStatus, startSimulation } from 'services/api';
import { connectSimulationFinished } from 'services/sse';
import type {
  MatchedTokensResponse,
  PaperResultResponse,
  SimulatedTokenResult,
} from 'types';

/**
 * Imperative access to the per-rule strategy-result cache (matched / simulate /
 * paper) for the strategy pages, which keep their own open/toggle state rather
 * than subscribing via hooks.
 *
 * Each call dispatches the RTK Query endpoint (so the result is deduped and
 * retained for `keepUnusedDataFor`), unwraps it, then releases the subscription
 * so the entry expires on schedule instead of leaking. Re-opening a view or
 * switching rules and back within the retention window resolves from cache
 * instead of re-hitting the backend. Pass `force` to bypass a fresh cache entry
 * (used when the paper-finished SSE event says the data changed).
 */
async function run<T>(sub: {
  unwrap(): Promise<T>;
  unsubscribe(): void;
}): Promise<T> {
  try {
    return await sub.unwrap();
  } finally {
    sub.unsubscribe();
  }
}

const opts = (force: boolean) => (force ? { forceRefetch: true } : undefined);

export function fetchMatchedCached(
  dispatch: AppDispatch,
  arg: StrategyRuleArg,
  force = false,
): Promise<MatchedTokensResponse> {
  return run(dispatch(labApi.endpoints.getStrategyMatched.initiate(arg, opts(force))));
}

/**
 * Resolve once the backtest for `ruleId` has finished. Primary signal is the
 * `simulation_finished` SSE — the same frame that clears the progress bar — so a
 * fast run resolves on the event. A `jobs/status` poll is a backstop for a missed
 * frame (e.g. a brief SSE reconnect): once a rule we saw running drops off the
 * in-flight list, the run ended. No premature timeout — an uncapped sim may
 * legitimately run for many minutes; the generous ceiling only guards against a
 * wedged/dead backend so the caller can't hang forever.
 */
function waitForSimulationFinish(ruleId: string): Promise<void> {
  return new Promise((resolve, reject) => {
    let settled = false;
    let sawRunning = false;
    const cleanup = () => {
      handle.close();
      clearInterval(poll);
      clearTimeout(ceiling);
    };
    const finish = () => {
      if (settled) return;
      settled = true;
      cleanup();
      resolve();
    };
    const handle = connectSimulationFinished((ev) => {
      if (ev.rule_id === ruleId) finish();
    });
    const poll = window.setInterval(() => {
      getJobsStatus()
        .then((status) => {
          const running = status.simulations.some((s) => s.rule_id === ruleId);
          if (running) sawRunning = true;
          else if (sawRunning) finish(); // was running, now gone → finished
        })
        .catch(() => {
          /* backend blip — keep waiting for the SSE */
        });
    }, 2000);
    const ceiling = window.setTimeout(
      () => {
        if (settled) return;
        settled = true;
        cleanup();
        reject(new Error('Simulation timed out'));
      },
      30 * 60 * 1000,
    );
  });
}

/**
 * Run (or reuse) a rule's backtest and return its token-level result.
 *
 * The simulation is uncapped and can take minutes, so it runs as a detached
 * backend job: this starts it, waits for the `simulation_finished` SSE (with a
 * `jobs/status` poll backstop), then fetches the stored result — instead of
 * holding one HTTP connection open for the whole run, which made any mid-run drop
 * surface as a `FETCH_ERROR`.
 *
 * Cache: a fresh fulfilled result for this `(strategy, ruleId, range)` is reused
 * directly (the imperative analogue of RTK's dedupe within `keepUnusedDataFor`),
 * so re-opening a view or toggling rules within the retention window does NOT
 * re-run. `force` skips that and always starts a new run.
 */
export function fetchSimulateCached(
  dispatch: AppDispatch,
  arg: StrategyRuleArg,
  force = false,
): Promise<SimulatedTokenResult[] | { cancelled: true }> {
  if (!force) {
    const cached = labApi.endpoints.getStrategySimulateResult.select(arg)(store.getState());
    if (cached.status === 'fulfilled' && cached.data !== undefined) {
      return Promise.resolve(cached.data);
    }
  }
  return (async () => {
    await startSimulation(arg.strategy, arg.ruleId, arg);
    await waitForSimulationFinish(arg.ruleId);
    // Force-refetch: the result endpoint removes the entry on read (single
    // delivery), so a stale cache hit must not short-circuit this fresh run.
    return run(
      dispatch(labApi.endpoints.getStrategySimulateResult.initiate(arg, { forceRefetch: true })),
    );
  })();
}

export function fetchPaperResultCached(
  dispatch: AppDispatch,
  arg: StrategyRuleArg,
  force = false,
): Promise<PaperResultResponse> {
  return run(dispatch(labApi.endpoints.getStrategyPaperResult.initiate(arg, opts(force))));
}

/** Drop cached matched results for a rule (fingerprint changed). */
export function invalidateStrategyMatched(dispatch: AppDispatch, arg: StrategyRuleArg): void {
  dispatch(
    labApi.util.invalidateTags([
      { type: 'StrategyResult', id: `${arg.strategy}:${arg.ruleId}` },
    ]),
  );
}

/** Drop the cached `matched` + `simulate` results for a rule (they derive from
 *  its entry criteria) so the next open re-runs against the edited rule. */
export function invalidateStrategyResult(dispatch: AppDispatch, arg: StrategyRuleArg): void {
  invalidateStrategyMatched(dispatch, arg);
}

/** Drop the cached paper-result for a rule (after clearing its run history) so
 *  the next open re-fetches the now-empty result instead of stale rows. */
export function invalidatePaperResult(dispatch: AppDispatch, arg: StrategyRuleArg): void {
  dispatch(
    labApi.util.invalidateTags([
      { type: 'StrategyPaper', id: `${arg.strategy}:${arg.ruleId}` },
    ]),
  );
}
