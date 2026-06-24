import { useState, type ReactNode } from 'react';
import { Badge } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { Input } from 'components/ui/Input';
import { useRenameGroupedSweepRunMutation } from 'store/apiSlice';
import {
  GROUP_FIELD_LABELS,
  type GroupField,
  type GroupedSweepRunRecord,
} from './groupedTypes';

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
            <Button variant="primary" size="sm" onClick={save} disabled={renameState.isLoading}>
              {renameState.isLoading ? 'Saving…' : 'Save'}
            </Button>
            <Button variant="ghost" size="sm" onClick={() => setEditing(false)}>
              Cancel
            </Button>
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

        <Button variant="ghost" size="sm" className="ml-auto" onClick={onReuse}>
          Use these settings
        </Button>
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
          {run.max_combos ?? 'default'} · buy {run.buy_amount_sol ?? 1} SOL
          {run.curve_only ? ' · curve-only' : ''}
        </Row>
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
          <pre className="mt-1 max-h-40 overflow-auto rounded border border-white/10 bg-bg-card p-2 font-mono text-[11px] leading-relaxed text-text-mid">
            {JSON.stringify(run.ix_labels_filter, null, 2)}
          </pre>
        ) : (
          <p className="mt-1 text-xs text-text-dim/60">
            {run.grouping_spec.includes('ix_labels')
              ? 'grouped by instruction labels'
              : 'no filter — all label sets included'}
          </p>
        )}
      </div>
    </div>
  );
}
