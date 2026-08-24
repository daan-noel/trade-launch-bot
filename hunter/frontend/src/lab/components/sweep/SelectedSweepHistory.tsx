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
import { COST_MODELS, FILL_MODELS, fillModelLabel } from 'lib/strategy/types';
import {
  GROUP_FIELD_LABELS,
  type GroupField,
  type GroupedSweepRunRecord,
} from './groupedTypes';
import type { AxisSpecWire } from './genericAxes';
import { formatDecimalTrim, tidySolDecimal } from 'utils/format';

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

/** Corpus lag past which the lake export was meaningfully behind the run — flag it.
 *  Below this the sweep and a same-day Simulate see effectively the same tape. */
const STALE_CORPUS_MS = 60 * 60 * 1000;

/** A rounded "4h 32m" / "18m" for a millisecond gap (never negative). */
function humanLag(ms: number): string {
  const mins = Math.max(0, Math.round(ms / 60_000));
  if (mins < 60) return `${mins}m`;
  const h = Math.floor(mins / 60);
  const m = mins % 60;
  return m ? `${h}h ${m}m` : `${h}h`;
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

/** One `AxisSpecWire.values` entry as text — `null` is the metric-axis `off`
 *  sentinel (that combo omits the condition entirely). */
function fmtAxisValues(values: (number | null)[]): string {
  return values.map((v) => (v == null ? 'off' : formatDecimalTrim(v, 4))).join(', ');
}

/** Render the persisted `axes_spec.axes` (the actual TP/SL values + metric
 *  conditions this run swept over) as short readable lines — so what a saved run
 *  swept is legible right here, without pushing it into the live editor via
 *  "Use these settings" (which would also discard any unsaved draft there). */
function axisSpecLines(axesSpec: unknown): string[] {
  const axes = (axesSpec as { axes?: AxisSpecWire[] } | null | undefined)?.axes;
  if (!Array.isArray(axes) || axes.length === 0) return [];
  return axes.map((a) => {
    if (a.kind === 'take_profit') return `TP: ${fmtAxisValues(a.values)} %`;
    if (a.kind === 'stop_loss') return `SL: ${fmtAxisValues(a.values)} %`;
    const window = a.window != null ? ` (${a.window}s)` : '';
    return `${a.side ?? 'entry'} · ${a.group ?? '?'}.${a.metric ?? '?'}${window} ${a.operator ?? ''} ${fmtAxisValues(a.values)}`;
  });
}

/** `volume_ix_patterns` (`string[][]`) as one line per pattern — the corpus-wide
 *  ix-name sequences a flow-axis run classified volume by. */
function volumeIxPatternLines(patterns: string[][] | null): string[] {
  if (!patterns || patterns.length === 0) return [];
  return patterns.map((p) => p.join(' → '));
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
  const axisLines = axisSpecLines(run.axes_spec);
  const flowLines = volumeIxPatternLines(run.volume_ix_patterns);
  // Name the scope fingerprint when it still exists; a deleted one keeps the run
  // honest by falling back to its id (the run row deliberately has no FK).
  const scopeFp = run.fingerprint_id
    ? fingerprints.find((f) => f.id === run.fingerprint_id)
    : undefined;
  // NULL = a legacy run, written before either model was selectable — it was
  // computed under what the sweep hardcoded then, so that is what we report.
  const fillModel = run.fill_model ?? 'worst_case';
  const costModel = run.cost_model ?? 'pumpfun_default';
  const fill = FILL_MODELS.find((m) => m.id === fillModel);
  const cost = COST_MODELS.find((m) => m.id === costModel);
  const fillLabel = fillModelLabel(fillModel);
  const costLabel = cost?.label ?? costModel;
  const fillHint = fill?.hint;
  const costHint = cost?.hint;
  // The pairing this whole selector exists to make visible: an explicit fill model
  // already prices execution slippage, so `pumpfun_default` charges it a second time.
  const doubleCounted = costModel === 'pumpfun_default';
  // How fresh the corpus was. A sweep reads the sealed Parquet lake ONLY; Simulate
  // splices the fresh PG tail on top. So an un-exported lake doesn't just make the run
  // "a bit old" — it freezes positions as `Open (est)` at prices a simulate of the same
  // rule watches die, and the two then disagree wildly with neither being wrong. The
  // lag is measured against the run's own start, not "now", because that is the gap
  // that existed when these numbers were computed.
  const corpusThrough = run.corpus_last_trade_at ? new Date(run.corpus_last_trade_at) : null;
  const corpusLagMs = corpusThrough ? new Date(run.created_at).getTime() - corpusThrough.getTime() : null;
  const corpusStale = corpusLagMs != null && corpusLagMs >= STALE_CORPUS_MS;

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
          {run.max_combos ?? 'default'} · buy {tidySolDecimal(run.buy_amount_sol ?? 1)} SOL ·{' '}
          {/* A NULL width IS the exact mode (Rust `SolPrecision::from_width`) — substituting
              the default here reported a 0.1 bucket on runs that grouped exact amounts. */}
          {run.bucket_width_sol == null
            ? 'exact SOL amounts'
            : `bucket ${tidySolDecimal(run.bucket_width_sol)} SOL`}
          {run.curve_only ? ' · curve-only' : ''}
        </Row>
        {/* Pricing is part of this run's IDENTITY, not a display preference: every
            PnL number below is only readable against these two. Shown on its own
            row — and warned about when the pair double-counts slippage — because a
            sweep whose fill model isn't visible next to its PnL is a trap. */}
        <Row label="Pricing">
          <span className="flex flex-wrap items-center gap-1.5">
            <Badge variant={fillModel === 'worst_case' ? 'warning' : 'info'} title={fillHint}>
              fill · {fillLabel}
            </Badge>
            <Badge variant={doubleCounted ? 'danger' : 'info'} title={costHint}>
              cost · {costLabel}
            </Badge>
            <span className="text-[10px] text-text-dim/60">
              {doubleCounted
                ? 'slippage charged twice — also priced into the fill'
                : 'not comparable with a run under different models'}
            </span>
          </span>
        </Row>
        {/* Corpus freshness. Sits next to Pricing for the same reason Pricing does:
            it changes what every PnL number below MEANS. A run swept over a lake that
            was hours behind will carry frozen `Open (est)` rows a Simulate closes. */}
        <Row label="Data through">
          {corpusThrough ? (
            <span className="flex flex-wrap items-center gap-1.5">
              <Badge
                variant={corpusStale ? 'warning' : 'info'}
                title={
                  corpusStale
                    ? 'The lake export was behind when this run started, so its corpus ends here. ' +
                      'Positions still open at this instant are frozen at these prices — a Simulate ' +
                      'of the same rule splices the fresher PG tail and will close some of them. ' +
                      'Re-export the lake (db-incremental-sync.ps1 -IncludeToday -ExportLake) and re-run to compare like for like.'
                    : 'Newest trade in the corpus this run scanned. The sweep reads the sealed lake only; ' +
                      'Simulate additionally splices the fresh PG tail.'
                }
              >
                {corpusThrough.toLocaleString()}
              </Badge>
              {corpusLagMs != null && (
                <span className={corpusStale ? 'text-[10px] text-warning' : 'text-[10px] text-text-dim/60'}>
                  {corpusStale
                    ? `lake was ${humanLag(corpusLagMs)} behind at run start — open rows are frozen here`
                    : `${humanLag(corpusLagMs)} behind run start`}
                </span>
              )}
            </span>
          ) : (
            <span className="text-text-dim/60">unknown (run predates this stamp)</span>
          )}
          {run.corpus_hash && (
            <span
              className="ml-1.5 text-[10px] text-text-dim/50"
              title={`Hash of the exact corpus slice this run scanned (${run.corpus_hash}). Two runs sharing this hash swept the same tape.`}
            >
              corpus {run.corpus_hash.slice(0, 8)}
            </span>
          )}
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
        {/* The actual TP/SL + metric-condition grid this run swept over — read-only
            here so it's legible without "Use these settings", which pushes it into
            the live editor and discards any unsaved draft sitting there. */}
        {axisLines.length > 0 && (
          <Row label="Sweep axes">
            <span className="flex flex-col">
              {axisLines.map((l, i) => (
                <span key={i}>{l}</span>
              ))}
            </span>
          </Row>
        )}
        {flowLines.length > 0 && (
          <Row label="Flow patterns">
            <span className="flex flex-col">
              {flowLines.map((l, i) => (
                <span key={i}>{l}</span>
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
