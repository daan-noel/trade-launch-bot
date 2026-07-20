/** Fallback while a lazy route chunk loads — shown inside `AppLayout` main only. */
export function SuspenseFallback() {
  return (
    <div className="flex items-center justify-center py-24 text-sm text-text-dim">
      Loading…
    </div>
  );
}
