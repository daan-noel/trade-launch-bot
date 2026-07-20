import { LoadingState } from './LoadingState';

/** Fallback while a lazy route chunk loads — shown inside `AppLayout` main only. */
export function SuspenseFallback() {
  return <LoadingState variant="page" label="Loading…" />;
}
