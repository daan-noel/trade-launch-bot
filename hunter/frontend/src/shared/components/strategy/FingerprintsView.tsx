import { useCallback, useMemo, useState, type ReactNode } from 'react';
import { Link } from 'react-router-dom';

import { DataTable } from 'components/table/DataTable';
import type { ColumnDef } from 'components/table/types';
import { IconButton } from 'components/ui/IconButton';
import { IconButtonGroup } from 'components/ui/IconButtonGroup';
import {
  ChartIcon,
  DuplicateIcon,
  EditIcon,
  LinkIcon,
  PlusIcon,
  TrashIcon,
} from 'components/ui/icons';
import { Badge } from 'components/ui/Badge';
import { Modal } from 'components/ui/Modal';
import { EmptyState } from 'components/ui/EmptyState';
import { PageHeader } from 'components/ui/PageHeader';
import { IxLabelsDisplay } from 'components/ui/IxLabelsDisplay';
import { ModeBadge } from './ModeBadge';
import { FingerprintForm } from './FingerprintForm';
import { ruleParamsCell } from './RuleParamsSummary';
import { capsDisplayText } from './capsRuleColumns';
import { useSelectionSearchParam } from 'hooks/useSelectionSearchParam';
import { apiErrorMessage } from 'store/baseApi';
import {
  useGetFingerprintsQuery,
  useGetStrategyRulesQuery,
  useCreateFingerprintMutation,
  useUpdateFingerprintMutation,
  useDeleteFingerprintMutation,
} from 'store/sharedEndpoints';
import {
  configuredIxLabels,
  formatIxLabelsText,
  IX_LABELS_FILTER_PLACEHOLDER,
  IX_LABELS_FILTER_TITLE,
  ixLabelsMatchFilter,
} from 'lib/ixLabels';
import { computeSameValueCellClasses } from 'lib/sameValueCellColors';
import { ixPatternsFromConfig } from 'lib/strategy/registry';
import { ixPatternsActions } from 'lib/flow/volumePatterns';
import { FlowPatternsChip } from './FingerprintParamsSummary';
import { flowDiscoveryHref, rulesHref, STRATEGY_PARAMS } from 'lib/strategy/nav';
import { fingerprintAutoName } from 'lib/strategy/fingerprintNameFromGroupKey';
import {
  lamportsToSol,
  type Fingerprint,
  type FingerprintDraft,
  type StrategyRule,
} from 'lib/strategy/types';

import {
  AXES,
  axisDef,
  formatPredicate,
  predicateSpans,
  predicatesOverlap,
  type AxisDef,
} from 'lib/strategy/fingerprintAxes';
import { parseAxisPredicate } from 'lib/strategy/fingerprintGrammar';

const fingerprintRowKey = (r: Fingerprint) => r.id;

function dash(): ReactNode {
  return <span className="text-text-dim">—</span>;
}

/** A row's label sequence, or `null` when the axis is unset. */
function rowLabels(r: Fingerprint): string[] | null {
  const p = (r.criteria ?? {}).ix_labels;
  return p?.kind === 'sequence' ? configuredIxLabels(p.labels) : null;
}

/** A row's predicate on one numeric axis, rendered in that axis's display unit. */
function axisText(r: Fingerprint, def: AxisDef): string | null {
  const p = (r.criteria ?? {})[def.id];
  return p ? formatPredicate(def.id, p) : null;
}

/** The value a numeric axis SORTS by: its low bound, or its high bound when the
 *  gate is open below. Sorting by a window needs one number, and the low edge is
 *  where the window starts — the same order a reader scanning the column expects.
 *  An unset axis sorts as `null` (last), never `0`, which would rank it as the
 *  smallest amount instead of no criterion. */
function axisSortValue(r: Fingerprint, def: AxisDef): number | null {
  const p = (r.criteria ?? {})[def.id];
  if (p == null || p.kind === 'sequence') return null;
  // The FIRST span's edge: a column orders on one number, and where the accepted
  // set begins is where a reader scanning it expects that number to be.
  const [first] = predicateSpans(p);
  const b = first?.min ?? first?.max;
  if (b == null) return null;
  const n = Number(b);
  return Number.isFinite(n) ? (def.unit === 'lamports' ? n / 1e9 : n) : null;
}

/** Param columns that tint when ≥2 fingerprints share the same value. */
const COLOR_COLS: {
  key: string;
  valueOf: (r: Fingerprint) => string | null;
}[] = AXES.map((def) => ({
  key: def.id,
  valueOf: (r: Fingerprint) =>
    def.kind === 'sequence'
      ? (rowLabels(r) ? formatIxLabelsText(rowLabels(r)!) : null)
      : axisText(r, def),
}));

/** Expanded row detail: rules that reference this fingerprint, with the same
 *  params summary as the Rules table so you can tell them apart at a glance.
 *  Each card navigates to Rules with that rule selected (`?rule=`). Same-tab
 *  by default; Ctrl/middle-click still opens a new tab. */
function FingerprintUsedByDetail({ rules }: { rules: StrategyRule[] }) {
  if (rules.length === 0) {
    return (
      <EmptyState compact message="Not used by any rules — safe to delete." />
    );
  }
  return (
    <div className="flex flex-col gap-2.5">
      <div className="flex items-baseline justify-between gap-3">
        <p className="text-[11px] font-semibold uppercase tracking-wider text-primary">
          Used by {rules.length} rule{rules.length === 1 ? '' : 's'}
        </p>
        <Link
          to={rulesHref()}
          className="text-[11px] text-accent hover:text-primary hover:underline"
        >
          Open Rules →
        </Link>
      </div>
      <ul className="grid gap-2 sm:grid-cols-2">
        {rules.map((r) => (
          <li key={r.id}>
            <Link
              to={rulesHref(r.id)}
              className="flex flex-col gap-2 rounded-md border border-info/25 bg-info/8 px-3 py-2.5 text-left transition-colors hover:border-accent/40 hover:bg-info/14"
              title={`Open rule “${r.rule_name}”`}
            >
              <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
                <span className="min-w-0 flex-1 truncate text-[13px] font-semibold text-text">
                  {r.rule_name}
                </span>
                <LinkIcon className="h-3.5 w-3.5 shrink-0 text-accent" />
                <ModeBadge mode={r.trade_mode} />
                <Badge
                  variant={!r.is_enabled ? 'danger' : r.is_active ? 'success' : 'neutral'}
                  size="sm"
                >
                  {!r.is_enabled ? 'Disabled' : r.is_active ? 'Active' : 'Idle'}
                </Badge>
              </div>
              <div className="flex flex-wrap items-center gap-x-3 gap-y-0.5 text-[11px] tabular-nums text-text-dim">
                <span>buy {lamportsToSol(r.buy_amount_lamports)}◎</span>
                <span>caps {capsDisplayText(r)}</span>
              </div>
              {ruleParamsCell(r.params)}
            </Link>
          </li>
        ))}
      </ul>
    </div>
  );
}

/**
 * Fingerprint library, shared by the live and lab apps: the match specs rules
 * reference. List (used-by count) + create/edit form (SOL inputs, lamports at
 * the API boundary) + used-by-guarded delete. Selecting a row expands the
 * rules that reference it.
 */
export function FingerprintsView({
  linkToFlowDiscovery = false,
  onViewMatches,
}: {
  /** Lab-only: show a per-row deep-link into Flow discovery scoped to that
   *  fingerprint. Off in the live app (Flow discovery is a lab surface). */
  linkToFlowDiscovery?: boolean;
  /** Lab-only: opens that fingerprint's matched tokens (creation-stats heatmap +
   *  trend + token table). Omitted ⇒ no such row button, because the endpoints
   *  behind it are lab-only and `shared ⊬ @lab`, so the page owns the modal. */
  onViewMatches?: (fingerprint: Fingerprint) => void;
} = {}) {
  const { data: fps = [], isLoading } = useGetFingerprintsQuery();
  const { data: rules = [] } = useGetStrategyRulesQuery();
  const [createFp, { isLoading: creating }] = useCreateFingerprintMutation();
  const [updateFp, { isLoading: updating }] = useUpdateFingerprintMutation();
  const [deleteFp] = useDeleteFingerprintMutation();

  const [selectedKey, setSelectedKey] = useSelectionSearchParam(STRATEGY_PARAMS.fingerprint);
  const [editing, setEditing] = useState<Fingerprint | 'new' | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const rulesByFp = useMemo(() => {
    const map = new Map<string, StrategyRule[]>();
    for (const r of rules) {
      const list = map.get(r.fingerprint_id);
      if (list) list.push(r);
      else map.set(r.fingerprint_id, [r]);
    }
    return map;
  }, [rules]);

  const rowDetail = useCallback(
    (fp: Fingerprint) => (
      <FingerprintUsedByDetail rules={rulesByFp.get(fp.id) ?? []} />
    ),
    [rulesByFp],
  );

  const valueColors = useMemo(
    () => computeSameValueCellClasses(fps, (r) => r.id, COLOR_COLS),
    [fps],
  );

  const cellTint =
    (colKey: string) =>
    (row: Fingerprint): string | undefined =>
      valueColors.get(`${row.id}\0${colKey}`);

  const editingId =
    editing && editing !== 'new' && editing.id ? editing.id : undefined;

  const submit = async (draft: FingerprintDraft) => {
    setErr(null);
    try {
      if (editingId) await updateFp({ id: editingId, body: draft }).unwrap();
      else await createFp(draft).unwrap();
      setEditing(null);
    } catch (e) {
      setErr(apiErrorMessage(e as never) ?? 'Save failed');
    }
  };

  const duplicate = (fp: Fingerprint) => {
    setErr(null);
    setEditing({
      ...fp,
      id: '',
      name: `${fp.name || fp.id.slice(0, 8)} copy`,
      used_by: 0,
    });
  };

  const remove = async (fp: Fingerprint) => {
    if (fp.used_by && fp.used_by > 0) return;
    if (!window.confirm(`Delete fingerprint "${fp.name || fp.id.slice(0, 8)}"?`)) return;
    try {
      await deleteFp(fp.id).unwrap();
    } catch (e) {
      setErr(apiErrorMessage(e as never) ?? 'Delete failed');
    }
  };

  const columns: ColumnDef<Fingerprint>[] = useMemo(
    () => [
      {
        key: 'name',
        label: 'Name',
        group: 'name',
        render: (r) => (
          <div className="flex items-center gap-1">
            <span className="font-medium text-text">{r.name || r.id.slice(0, 8)}</span>
            {/* The machine identity, shown whenever the stored label is not already
                it. A nickname is the only record of WHY a fingerprint exists, so it
                stays — but two nicknames over one auto-name are one fingerprint
                stored twice, and nothing else on the row says so. */}
            {r.name.trim() !== fingerprintAutoName(r) && (
              <span
                title={'Auto-name from the match axes.\nTwo rows showing the same one match the same tokens, whatever they are called.'}
                className="shrink-0 truncate font-mono text-[10px] text-text-dim"
              >
                {fingerprintAutoName(r)}
              </span>
            )}
            {/* Every axis column below is a dash on a wildcard row — without this
                the one fingerprint that arms on EVERY token is the one that looks
                least configured. */}
            {r.wildcard && (
              <span
                title="Wildcard — matches every token, ignoring every creation-shape axis"
                className="shrink-0 rounded border border-white/10 px-1 py-px text-[10px] font-medium uppercase tracking-wide text-accent"
              >
                all
              </span>
            )}
            {linkToFlowDiscovery && (
              <Link
                to={flowDiscoveryHref(r.id)}
                title={`Flow discovery for “${r.name || r.id.slice(0, 8)}”`}
                aria-label={`Flow discovery for ${r.name || r.id.slice(0, 8)}`}
                className="inline-flex shrink-0 rounded p-0.5 text-accent hover:bg-accent/15 hover:text-primary"
                onClick={(e) => e.stopPropagation()}
              >
                <LinkIcon className="h-3.5 w-3.5" />
              </Link>
            )}
          </div>
        ),
        // `wildcard` has no column of its own (it is the absence of every axis), so
        // the name column is where it has to be findable.
        searchValue: (r) => (r.wildcard ? `${r.name} wildcard all tokens` : r.name),
      },
      // One column per registry axis, generated — so a new axis is a table column,
      // searchable, sortable and filterable, with no edit here.
      //
      // A cell shows the axis's PREDICATE (`1.515`, `1.5–2`, `≥1.5`), not a bare
      // value, because that is what the row matches on. The numeric filter reads
      // the typed text through the SAME parser the form and the filter box use, so
      // typing `1.5-2` selects the rows whose window contains that window's floor.
      ...AXES.filter((d) => d.kind === 'numeric').map((def) => ({
        key: def.id,
        label: def.label,
        group: def.id,
        render: (r: Fingerprint) => {
          const t = axisText(r, def);
          if (t == null) return dash();
          return (
            <span className="font-mono tabular-nums" title={def.definition}>
              {t}
              {def.unit === 'lamports' ? '◎' : ''}
            </span>
          );
        },
        searchValue: (r: Fingerprint) => axisText(r, def) ?? '',
        sortValue: (r: Fingerprint) => axisSortValue(r, def),
        filterMatch: (r: Fingerprint, raw: string) => {
          const p = (r.criteria ?? {})[def.id];
          if (p == null || p.kind === 'sequence') return false;
          // Read through the SAME grammar the axis form saves with, so a condition
          // pasted from a row selects that row. Unparseable input matches nothing
          // rather than everything: a dropped filter reads as "no filter", which
          // widens the table silently.
          const want = parseAxisPredicate(raw, def.unit);
          if (want == null) return false;
          // Overlap, not containment — "could this row match anything I typed". For
          // a bare value the two are the same, so this only widens what the box can
          // ask, never what a plain amount answers.
          return predicatesOverlap(p, want);
        },
        filterPlaceholder: def.unit === 'lamports' ? 'e.g. 1.515' : 'e.g. 3',
        filterTitle: `${def.label} — ${def.definition}\n\nType a value or a condition (1..5, >=2, !=3, <=2 | >=7); a row matches when its own condition can accept something you typed.`,
        sortable: true,
        cellClassName: cellTint(def.id),
      })),
      {
        key: 'ix_count_axis',
        label: 'ix len',
        group: 'ix',
        render: (r: Fingerprint) => {
          const labels = rowLabels(r);
          if (!labels) return dash();
          return <span className="font-mono tabular-nums">{labels.length}</span>;
        },
        searchValue: (r: Fingerprint) => String(rowLabels(r)?.length ?? ''),
        sortValue: (r: Fingerprint) => rowLabels(r)?.length ?? null,
        filterNumber: (r: Fingerprint) => rowLabels(r)?.length ?? null,
        sortable: true,
      },
      {
        key: 'ix_labels',
        label: axisDef('ix_labels').label,
        group: 'ix',
        width: '220px',
        render: (r: Fingerprint) => {
          const labels = rowLabels(r);
          return labels ? <IxLabelsDisplay labels={labels} copyJson /> : dash();
        },
        searchValue: (r: Fingerprint) => {
          const labels = rowLabels(r);
          return labels ? formatIxLabelsText(labels) : '';
        },
        filterMatch: (r: Fingerprint, raw: string) => ixLabelsMatchFilter(rowLabels(r), raw),
        filterPlaceholder: IX_LABELS_FILTER_PLACEHOLDER,
        filterTitle: IX_LABELS_FILTER_TITLE,
        cellClassName: cellTint('ix_labels'),
      },
      {
        key: 'flow_patterns',
        label: 'flow patterns',
        group: 'ix',
        render: (r) => {
          const patterns = ixPatternsFromConfig(r.metric_config);
          // The chip, not a bare count Badge: `1` and `1` are different criteria
          // when the sequences differ, and the ribbon + tooltip is where that
          // shows. The numeric sort/filter below still reads the count.
          return patterns.length === 0 ? dash() : <FlowPatternsChip patterns={patterns} />;
        },
        searchValue: (r) => {
          const patterns = ixPatternsFromConfig(r.metric_config);
          return `${patterns.length} ${ixPatternsActions(patterns)}`;
        },
        sortValue: (r) => ixPatternsFromConfig(r.metric_config).length,
        filterNumber: (r) => ixPatternsFromConfig(r.metric_config).length || null,
        sortable: true,
      },
      {
        key: 'used_by',
        label: 'Used by',
        group: 'used',
        render: (r) => (
          <Badge className="text-lg" variant={r.used_by ? 'info' : 'neutral'}>
            {r.used_by ?? 0}
          </Badge>
        ),
        searchValue: (r) => String(r.used_by ?? 0),
        sortValue: (r) => r.used_by ?? 0,
        filterNumber: (r) => r.used_by ?? 0,
        sortable: true,
      },
    ],
    [valueColors, linkToFlowDiscovery],
  );

  const editingRules = editingId ? (rulesByFp.get(editingId) ?? []) : [];

  return (
    <div className="flex flex-col gap-3 p-4">
      <PageHeader
        className="mb-0"
        title="Fingerprints"
        description="Match specs · select a row to see which rules use it"
        actions={
          <>
            <div className="grow" />

            <IconButton
              variant="success"
              size="lg"
              label="New fingerprint"
              title="New fingerprint"
              onClick={() => setEditing('new')}
            >
              <PlusIcon />
            </IconButton>
          </>
        }
      />
      {err && <p className="text-xs text-red">{err}</p>}
      <DataTable
        columns={columns}
        rows={fps}
        rowKey={fingerprintRowKey}
        loading={isLoading}
        searchable
        colFilters
        colToggle
        tableId="fingerprints-v2"
        pinnable
        emptyMessage="No fingerprints yet — create one to start authoring rules."
        selectedKey={selectedKey}
        onSelect={setSelectedKey}
        rowDetail={rowDetail}
        rowActions={(r) => (
          <IconButtonGroup>
            {onViewMatches && (
              <IconButton
                variant="primary"
                size="md"
                title="Matched tokens — creation heatmap, trend and table"
                aria-label="Matched tokens"
                onClick={() => onViewMatches(r)}
              >
                <ChartIcon />
              </IconButton>
            )}
            <IconButton
              variant="accent"
              size="md"
              title="Edit"
              aria-label="Edit"
              onClick={() => setEditing(r)}
            >
              <EditIcon />
            </IconButton>
            <IconButton
              variant="ghost"
              size="md"
              title="Duplicate"
              aria-label="Duplicate"
              onClick={() => duplicate(r)}
            >
              <DuplicateIcon />
            </IconButton>
            <IconButton
              variant="danger"
              size="md"
              disabled={Boolean(r.used_by && r.used_by > 0)}
              title={r.used_by ? `Used by ${r.used_by} rule(s)` : 'Delete'}
              aria-label={r.used_by ? `Used by ${r.used_by} rule(s)` : 'Delete'}
              onClick={() => remove(r)}
            >
              <TrashIcon />
            </IconButton>
          </IconButtonGroup>
        )}
      />
      <Modal
        title={editingId ? 'Edit fingerprint' : 'New fingerprint'}
        open={editing !== null}
        onClose={() => setEditing(null)}
      >
        {editing !== null && (
          <div className="flex flex-col gap-3">
            {editingId && <FingerprintUsedByDetail rules={editingRules} />}
            <FingerprintForm
              initial={editing === 'new' ? undefined : editing}
              onSubmit={submit}
              onCancel={() => setEditing(null)}
              submitting={creating || updating}
              error={err}
            />
          </div>
        )}
      </Modal>
    </div>
  );
}
