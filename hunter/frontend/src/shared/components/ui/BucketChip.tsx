import { cn } from 'lib/cn';

/** Shared tooltip copy for the "bucketed input" marker on continuous SOL fields. */
const BUCKET_MATCH_HINT =
  'Matched by bucket, not exactly: a token matches when its value lands in the ' +
  'same [lo, hi) range as this one. Range width = Bucket Size (SOL).';

/** The one bucket marker, shared by the grouped-sweep fingerprint picker and the
 *  strategy rule form so both surfaces flag bucketed SOL inputs identically. A
 *  continuous SOL amount matched by `[lo, hi)` bucket, not exact value.
 *
 *  Pass `width` where the bucket width is fixed (the grouped sweep bins at a
 *  constant `SOL_BUCKET_WIDTH`) to read `◎0.1 buckets`; omit it on the rule form,
 *  where the width is the editable "Bucket Size (SOL)" field, for a plain `bucket`
 *  chip. */
export function BucketChip({
  width,
  title = BUCKET_MATCH_HINT,
  className,
}: {
  width?: number;
  /** Tooltip override; defaults to the shared bucket-match explanation. */
  title?: string;
  className?: string;
}) {
  return (
    <span
      className={cn(
        'shrink-0 rounded-sm bg-accent/12 px-1 font-mono text-[10px] leading-tight text-accent/80',
        className,
      )}
      title={title}
    >
      {width != null ? `◎${width} buckets` : 'bucket'}
    </span>
  );
}
