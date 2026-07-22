import { useState, type ReactNode } from 'react';
import { Link } from 'react-router-dom';
import { Badge } from 'components/ui/Badge';
import { IconButton } from 'components/ui/IconButton';
import { IconButtonGroup } from 'components/ui/IconButtonGroup';
import { CloseIcon, ReuseIcon, SaveIcon, SpinnerIcon } from 'components/ui/icons';
import { Input } from 'components/ui/Input';
import { IxLabelsDisplay } from 'components/ui/IxLabelsDisplay';
import { useRenameGroupedSweepRunMutation } from '@lab/store/labEndpoints';
import { useGetFingerprintsQuery } from 'store/sharedEndpoints';
import { fingerprintsHref } from 'lib/strategy/nav';
import {
  GROUP_FIELD_LABELS,
  SOL_BUCKET_WIDTH,
  type GroupField,
  type GroupedSweepRunRecord,
} from './groupedTypes';
import { tidySolDecimal } from 'utils/format';

/** A read-only summary of the currently-selected sweep run's full launch config
 *  — what corpus/filters/grid it was swept over — so a saved run is legible at a
 *  glance long after it ran (especially which `ix_labels` set it was pinned to,
 *  which the result tables never show). Also hosts the inline rename + the
 *  "Use these settings" re-run trigger. Pure metadata from the runs query: it
 *  touches none of the heavy groups/results fetch. */

const STATUS_BADGE: Record<GroupedSweepRunRecord['status'], 'primary' | 'warning' | 'danger'> = {
  completed: 'primary',
  running: 'warning',
  cancelled: 'danger',
  partial: 'danger',
  // Kept (not deleted) so the attempt stays inspectable, but it folded no groups.
  failed: 'danger',
};

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex gap-2 text-xs">
      <span className="w-32 shrink-0 text-text-dim/70">{label}</span>
      <span className="min-w-0 flex-1 break-words font-mono text-text-mid">{children}</span>
    </div>
  );
}

/** Render the persisted per-field corpus filters as readable lines. Booleans are
 *  the cashback filter; everything else is a numeric value set. */
function fieldFilterLines(filters: Record<string, (number | boolean)[]>): string[] {
  return Object.entries(filters)
    .filter(([, vals]) => vals.length > 0)
    .map(([field, vals]) => {
      const label = GROUP_FIELD_LABELS[field as GroupField] ?? field;
      return `${label}: ${vals.join(', ')}`;
    });
}

export interface SelectedSweepHistoryProps {
  strategyId: string;
  run: GroupedSweepRunRecord;
  /** Tokens covered by the **persisted** groups (Σ group `token_count`) — for a
   *  partial run this is below the run's total `token_count`, so the Population row
   *  shows it as `done/total`. `null` when the groups aren't loaded yet (or the run
   *  is complete, where it equals `token_count` and the plain total is shown). */
  tokensDone?: number | null;
  /** Push this run's stored config back into the sweep form (re-run). */
  onReuse: () => void;
}

export function SelectedSweepHistory({ strategyId, run, tokensDone, onReuse }: SelectedSweepHistoryProps) {
  const [rename, renameState] = useRenameGroupedSweepRunMutation();
  const { data: fingerprints = [] } = useGetFingerprintsQuery();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState('');

  function startEdit() {
    setDraft(run.label ?? '');
    setEditing(true);
  }
  async function save() {
    try {
      await rename({ strategyId, runId: run.id, label: draft }).unwrap();
      setEditing(false);
    } catch {
      // Surfaced inline below; keep edit mode open so the value isn't lost.
    }
  }

  const grouping = run.grouping_spec.length
    ? run.grouping_spec.map((f) => GROUP_FIELD_LABELS[f] ?? f).join(' + ')
    : 'ALL (single group)';
  const range =
    run.created_after || run.created_before
      ? `${run.created_after ? new Date(run.created_after).toLocaleString() : '−∞'} → ${
          run.created_before ? new Date(run.created_before).toLocaleString() : 'now'
        }`
      : 'any';
  const fieldLines = run.field_filters ? fieldFilterLines(run.field_filters) : [];
  // Name the scope fingerprint when it still exists; a deleted one keeps the run
  // honest by falling back to its id (the run row deliberately has no FK).
  const scopeFp = run.fingerprint_id
    ? fingerprints.find((f) => f.id === run.fingerprint_id)
    : undefined;

  return (
    <div className="mb-4 bg-surface">
      <div className="mb-2.5 flex flex-wrap items-center gap-2">
        <span className="text-[9px] font-bold uppercase tracking-wider text-text-dim/80">
          Selected sweep history
        </span>
        <Badge variant={STATUS_BADGE[run.status]} className="font-mono">
          {run.status}
        </Badge>

        {editing ? (
          <span className="flex items-center gap-1.5">
            <Input
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              placeholder="Name this run…"
              className="w-56"
              autoFocus
              onKeyDown={(e) => {
                if (e.key === 'Enter') save();
                if (e.key === 'Escape') setEditing(false);
              }}
            />
            <IconButtonGroup className="gap-1.5">
              <IconButton
                variant="primary"
                size="md"
                onClick={save}
                disabled={renameState.isLoading}
                title={renameState.isLoading ? 'Saving…' : 'Save'}
                aria-label={renameState.isLoading ? 'Saving…' : 'Save'}
              >
                {renameState.isLoading ? <SpinnerIcon /> : <SaveIcon />}
              </IconButton>
              <IconButton
                variant="ghost"
                size="md"
                onClick={() => setEditing(false)}
                title="Cancel"
                aria-label="Cancel"
              >
                <CloseIcon />
              </IconButton>
            </IconButtonGroup>
          </span>
        ) : (
          <button
            type="button"
            onClick={startEdit}
            title="Rename this run"
            className="group flex items-center gap-1.5 text-sm"
          >
            <span className={run.label ? 'font-semibold text-primary' : 'italic text-text-dim'}>
              {run.label ?? 'Unnamed run'}
            </span>
            <span className="text-text-dim/50 group-hover:text-text-dim">✎</span>
          </button>
        )}

        <IconButton
          variant="ghost"
          size="md"
          className="ml-auto"
          onClick={onReuse}
          title="Use these settings"
          aria-label="Use these settings"
        >
          <ReuseIcon />
        </IconButton>
      </div>

      {renameState.error && (
        <p className="mb-2 text-[10px] text-red">Rename failed — try again.</p>
      )}

      <div className="grid gap-1 md:grid-cols-2">
        <Row label="Created">{new Date(run.created_at).toLocaleString()}</Row>
        <Row label="Token range">{range}</Row>
        <Row label="Grouping">{grouping}</Row>
        <Row label="Method">{run.method}</Row>
        <Row label="Population">
          {run.status !== 'completed' && tokensDone != null
            ? `${tokensDone}/${run.token_count}`
            : run.token_count}{' '}
          tokens · {run.groups_done}/{run.group_count} groups · {run.combo_count} combos
        </Row>
        <Row label="Caps / gates">
          min {run.min_tokens} tok/grp · token cap {run.token_cap ?? '—'} · max combos{' '}
          {run.max_combos ?? 'default'} · buy {tidySolDecimal(run.buy_amount_sol ?? 1)} SOL · bucket{' '}
          {tidySolDecimal(run.bucket_width_sol ?? SOL_BUCKET_WIDTH)} SOL
          {run.curve_only ? ' · curve-only' : ''}
        </Row>
        {run.fingerprint_id && (
          <Row label="Corpus scope">
            <Link
              to={fingerprintsHref(run.fingerprint_id)}
              className="inline-flex items-center gap-1 hover:opacity-90"
              title="Open the fingerprint this run was scoped to"
            >
              <Badge variant="info">
                engine match · {scopeFp?.name ?? `${run.fingerprint_id.slice(0, 8)} (deleted)`}
              </Badge>
            </Link>
          </Row>
        )}
        {fieldLines.length > 0 && (
          <Row label="Field filters">
            <span className="flex flex-col">
              {fieldLines.map((l) => (
                <span key={l}>{l}</span>
              ))}
            </span>
          </Row>
        )}
      </div>

      <div className="mt-2.5">
        <span className="text-[9px] font-bold uppercase tracking-wider text-text-dim/80">
          Instruction-label filter
        </span>
        {run.ix_labels_filter && run.ix_labels_filter.length > 0 ? (
          <div className="mt-1 max-h-40 overflow-auto">
            <IxLabelsDisplay labels={run.ix_labels_filter} copyJson />
          </div>
        ) : (
          <p className="mt-1 text-xs text-text-dim/60">
            {run.grouping_spec.includes('ix_labels')
              ? 'grouped by instruction labels'
              : run.fingerprint_id
                ? 'scoped by fingerprint — its own ix_labels axis decided the match'
                : 'no filter — all label sets included'}
          </p>
        )}
      </div>
    </div>
  );
}
