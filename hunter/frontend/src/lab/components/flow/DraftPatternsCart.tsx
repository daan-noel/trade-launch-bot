import { Fragment, useState } from 'react';

import { VolumeIxPatternsEditor } from 'components/strategy/VolumeIxPatternsEditor';
import { LabelTip } from 'components/strategy/LabelTip';
import { Badge } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { EmptyState } from 'components/ui/EmptyState';
import { IconButton } from 'components/ui/IconButton';
import { CheckIcon, CloseIcon, EditIcon, LinkIcon, SpinnerIcon } from 'components/ui/icons';
import { DISCOVERY_FIELD_HELP } from 'lib/strategy/strategyHelp';
import { metricConfigWithVolumePatterns } from 'lib/strategy/registry';
import type { Fingerprint } from 'lib/strategy/types';

/** Staging "cart" for the volume_ix_patterns being assembled: an accent-elevated
 *  panel that reads as the page's deliverable, not just another box. Checked rows
 *  from the ranked table land here as chips; the primary Apply CTA writes them back
 *  to the fingerprint. Raw JSON editing is one toggle away. */
export function DraftPatternsCart({
  draftPatterns,
  onChange,
  currentPatterns,
  targetFp,
  applying,
  onApply,
}: {
  draftPatterns: string[][];
  onChange: (patterns: string[][]) => void;
  currentPatterns: string[][];
  targetFp: Fingerprint | null;
  applying: boolean;
  onApply: () => void;
}) {
  const [rawEdit, setRawEdit] = useState(false);

  const norm = (ps: string[][]) =>
    ps.map((p) => p.map((s) => s.trim()).filter(Boolean)).filter((p) => p.length > 0);
  const draftNorm = norm(draftPatterns);
  const savedNorm = norm(currentPatterns);
  const stagedCount = draftNorm.length;
  const dirty = JSON.stringify(draftNorm) !== JSON.stringify(savedNorm);

  const applyLabel = applying
    ? 'Applying…'
    : targetFp
      ? `Update “${targetFp.name}”`
      : 'Create & bind fingerprint';

  return (
    <div className="rounded-lg border border-accent/30 bg-accent/4 p-3 shadow-[0_0_0_1px_color-mix(in_srgb,var(--color-accent)_10%,transparent)]">
      <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
        <span className="inline-flex flex-wrap items-center gap-2">
          <LabelTip
            tip={DISCOVERY_FIELD_HELP.draftPatterns}
            className="text-xs font-semibold text-text"
          >
            Draft volume_ix_patterns
          </LabelTip>
          <Badge variant={stagedCount > 0 ? 'accent' : 'neutral'} size="sm" pill>
            {stagedCount} staged
          </Badge>
          {targetFp && <span className="text-[10px] text-text-dim">{savedNorm.length} saved</span>}
          {dirty && stagedCount > 0 && (
            <Badge variant="warning" size="sm" pill>
              unsaved
            </Badge>
          )}
        </span>
        <Button
          variant="link"
          size="xs"
          onClick={() => setRawEdit((v) => !v)}
          title={rawEdit ? 'Back to chip view' : 'Edit raw JSON label sequences'}
        >
          <EditIcon className="h-3 w-3" />
          {rawEdit ? 'Done editing' : 'Edit raw'}
        </Button>
      </div>

      {rawEdit ? (
        <div className="max-h-72 overflow-y-auto pr-1">
          <VolumeIxPatternsEditor patterns={draftPatterns} onChange={onChange} />
        </div>
      ) : stagedCount === 0 ? (
        <EmptyState
          compact
          message={
            <>
              No structures staged. Check rows in the ranked table below.
              <br />
              <span className="text-[10px] text-text-dim/70">
                Flow metrics stay NaN until at least one structure is staged.
              </span>
            </>
          }
          action={
            <button
              type="button"
              className="text-[11px] font-semibold text-accent hover:underline"
              onClick={() => {
                onChange([...draftPatterns, []]);
                setRawEdit(true);
              }}
            >
              or add one manually
            </button>
          }
        />
      ) : (
        <ul className="flex max-h-64 flex-col gap-1.5 overflow-y-auto pr-1">
          {draftPatterns.map((p, i) => {
            const labels = p.map((s) => s.trim()).filter(Boolean);
            if (labels.length === 0) return null;
            return (
              <li
                key={i}
                className="flex items-center gap-2 rounded border border-white/8 bg-white/3 px-2 py-1.5"
              >
                <span className="w-4 shrink-0 font-mono text-[9px] text-text-dim/60">{i + 1}</span>
                <div className="flex min-w-0 flex-1 flex-wrap items-center gap-1">
                  {labels.map((label, k) => (
                    <Fragment key={k}>
                      {k > 0 && <span className="text-[10px] text-text-dim/40">›</span>}
                      <span className="rounded bg-white/6 px-1.5 py-0.5 font-mono text-[10px] text-text-mid">
                        {label}
                      </span>
                    </Fragment>
                  ))}
                </div>
                <IconButton
                  variant="danger"
                  size="sm"
                  type="button"
                  onClick={() => onChange(draftPatterns.filter((_, j) => j !== i))}
                  title="Remove pattern"
                  aria-label="Remove pattern"
                >
                  <CloseIcon />
                </IconButton>
              </li>
            );
          })}
        </ul>
      )}

      <Button
        variant="primary"
        size="md"
        className="mt-3 w-full"
        disabled={stagedCount === 0 || applying}
        onClick={onApply}
        title={applyLabel}
      >
        {applying ? (
          <SpinnerIcon className="h-4 w-4" />
        ) : targetFp ? (
          <CheckIcon className="h-4 w-4" />
        ) : (
          <LinkIcon className="h-4 w-4" />
        )}
        {applyLabel}
      </Button>

      {targetFp && currentPatterns.length > 0 && (
        <details className="mt-2 text-[10px] text-text-dim">
          <summary className="cursor-pointer">Saved config</summary>
          <pre className="mt-1 overflow-x-auto rounded bg-black/20 p-2 font-mono">
            {JSON.stringify(metricConfigWithVolumePatterns(currentPatterns), null, 2)}
          </pre>
        </details>
      )}
    </div>
  );
}
