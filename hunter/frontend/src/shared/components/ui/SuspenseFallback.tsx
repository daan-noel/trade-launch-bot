/** Lightweight route-level loading fallback shown while a lazy page chunk loads. */
export function SuspenseFallback() {
  return (
    <div className="flex items-center justify-center py-24 text-sm text-text-dim">
      Loading…
    </div>
  );
}
