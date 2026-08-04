import { cn } from 'lib/cn';

/** Shared tooltip copy for the "bucketed input" marker on continuous SOL fields. */
const BUCKET_MATCH_HINT =
  'Matched by bucket, not exactly: a token matches when its value lands in the ' +
  'same [lo, hi) range as this one. Range width = Bucket Size (SOL).';

/** Tooltip copy for the exact-precision variant — the NULL-width mode. */
const EXACT_MATCH_HINT =
  'Matched on the exact SOL amount: one group per distinct value, no [lo, hi) ' +
  'range. This is the NULL bucket-width mode.';

/** The one bucket marker, shared by the grouped-sweep fingerprint picker and the
 *  strategy rule form so both surfaces flag bucketed SOL inputs identically. A
 *  continuous SOL amount matched by `[lo, hi)` bucket, not exact value.
 *
 *  Pass `width` where the bucket width is fixed (the grouped sweep bins at a
 *  constant `SOL_BUCKET_WIDTH`) to read `◎0.1 buckets`; omit it on the rule form,
 *  where the width is the editable "Bucket Size (SOL)" field, for a plain `bucket`
 *  chip. Pass `exact` for the NULL-width mode — it wins over `width`, so a surface
 *  that keeps a stale width around (the disabled input) can't render a bucket chip
 *  on a run that grouped exact amounts. */
export function BucketChip({
  width,
  exact = false,
  title,
  className,
}: {
  width?: number;
  /** Exact-amount precision (`SolPrecision::Exact` / NULL width) — overrides `width`. */
  exact?: boolean;
  /** Tooltip override; defaults to the shared match explanation for the mode. */
  title?: string;
  className?: string;
}) {
  return (
    <span
      className={cn(
        'shrink-0 rounded-sm bg-accent/12 px-1 font-mono text-[10px] leading-tight text-accent/80',
        className,
      )}
      title={title ?? (exact ? EXACT_MATCH_HINT : BUCKET_MATCH_HINT)}
    >
      {exact ? '◎ exact' : width != null ? `◎${width} buckets` : 'bucket'}
    </span>
  );
}
