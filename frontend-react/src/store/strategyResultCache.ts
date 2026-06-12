import { apiSlice, type StrategyRuleArg } from './apiSlice';
import type { AppDispatch } from './index';
import type {
  MatchedTokenRecord,
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
): Promise<MatchedTokenRecord[]> {
  return run(dispatch(apiSlice.endpoints.getStrategyMatched.initiate(arg, opts(force))));
}

export function fetchSimulateCached(
  dispatch: AppDispatch,
  arg: StrategyRuleArg,
  force = false,
): Promise<SimulatedTokenResult[]> {
  return run(dispatch(apiSlice.endpoints.getStrategySimulate.initiate(arg, opts(force))));
}

export function fetchPaperResultCached(
  dispatch: AppDispatch,
  arg: StrategyRuleArg,
  force = false,
): Promise<PaperResultResponse> {
  return run(dispatch(apiSlice.endpoints.getStrategyPaperResult.initiate(arg, opts(force))));
}

/** Drop the cached `matched` + `simulate` results for a rule (they derive from
 *  its entry criteria) so the next open re-runs against the edited rule. */
export function invalidateStrategyResult(dispatch: AppDispatch, arg: StrategyRuleArg): void {
  dispatch(
    apiSlice.util.invalidateTags([
      { type: 'StrategyResult', id: `${arg.strategy}:${arg.ruleId}` },
    ]),
  );
}
