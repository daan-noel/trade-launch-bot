/**
 * Barrel for the split RTK Query layer. The single `createApi` was decomposed
 * into a shell (`baseApi`) plus three `injectEndpoints` modules — shared /
 * deploy / analysis — so the frontend split (two Vite entries) can bundle only
 * the endpoints each mode needs. This barrel re-exports every hook + type + the
 * `apiErrorMessage` helper so existing `from 'store/apiSlice'` imports keep
 * compiling unchanged.
 *
 * Note: `apiSlice` here is the bare `baseApi` shell — its typed `.endpoints.X`
 * / `.util.updateQueryData('X')` accessors are NOT available. The few call
 * sites that need those import the owning typed api directly
 * (`sharedApi` / `deployApi` / `analysisApi`).
 */
export { baseApi as apiSlice, apiErrorMessage } from './baseApi';
export * from './sharedEndpoints';
// Mode endpoints are NOT re-exported here — deploy hooks live in
// `@deploy/store/deployEndpoints` and analysis hooks in
// `@analysis/store/analysisEndpoints`, imported directly by mode-only code so
// neither mode's `injectEndpoints` side effect leaks into the other's bundle.
// This barrel re-exports SHARED endpoints only.
