import type { Dispatch, ThunkDispatch, UnknownAction } from '@reduxjs/toolkit';

/**
 * Mode-agnostic dispatch type for **shared** components/pages that dispatch RTK
 * Query thunks (`endpoint.initiate`, `util.updateQueryData`) or plain actions
 * but never read a mode-specific slice. Both per-mode stores' `AppDispatch`
 * (live = `{syncToken, api}`, lab = `{swingDetection, api}`) are
 * assignable here, so a shared component compiles against either build.
 *
 * Mode-specific code that reads its own slice imports the concrete
 * `RootState` / `AppDispatch` from its own store (`@live/store` /
 * `@lab/store`) instead.
 */
// `any` for the thunk state generic (not `unknown`): RTK Query thunks such as
// `util.updateQueryData` are typed against the concrete api `RootState`, and a
// `ThunkDispatch<unknown, …>` rejects them. `any` keeps shared dispatch usable
// against either mode store without leaking a mode-specific state type.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type AppDispatch = ThunkDispatch<any, any, UnknownAction> & Dispatch<UnknownAction>;
