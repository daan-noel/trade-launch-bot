import { useCallback, useMemo } from 'react';

import { DataTable } from 'components/table/DataTable';
import { parseNumericPredicate } from 'components/table/numericFilter';
import type { ColumnDef } from 'components/table/types';
import { Badge } from 'components/ui/Badge';
import { Checkbox } from 'components/ui/Checkbox';
import { IxLabelsDisplay } from 'components/ui/IxLabelsDisplay';
import {
  dominantFeeVariant,
  feeConcentration,
  feeSpreadBand,
  feeVariantTitle,
  formatFeeVariant,
} from 'lib/feeVariants';
import {
  formatIxLabelsText,
  ixLabelsMatchFilter,
  IX_LABELS_FILTER_PLACEHOLDER,
  IX_LABELS_FILTER_TITLE,
} from 'lib/ixLabels';
import { signalGradeClass } from 'lib/signedTone';
import { DISCOVERY_COL_HELP, type HelpTip } from 'lib/strategy/strategyHelp';
import type { FlowDiscoveryStructure } from 'types';

import {
  firstSlotPurity,
  isFirstSlotPresent,
  liftGrade,
  pctGrade,
  sideOf,
  suggestExplain,
  unitGrade,
  washGrade,
  type StructureSuggestion,
} from './flowDiscoverySuggest';

/** Stable structure identity — same key the draft/preview sets use. */
const structureRowKey = (s: FlowDiscoveryStructure) => JSON.stringify(s.ix_labels);

/** Column visibility + sort/page-size prefs and pins persist under this id. */
const TABLE_ID = 'flow-structures';

function fmt(n: number, digits = 1): string {
  if (!Number.isFinite(n)) return '—';
  return n.toFixed(digits);
}

/** Flatten a rich `{title, body}` help tip into the plain string DataTable's
 *  `ColumnDef.tooltip` (native title attr) expects — same convention as the
 *  generic sweep table's column tooltips. */
function helpText(tip: HelpTip): string {
  return `${tip.title}\n\n${tip.body}`;
}

/** A row's auto verdict as an exclusive filter value. Mirrors the three states the
 *  `Auto` cell renders (badge / bare % / gated dash); a row the run never scored
 *  has no value and matches no choice. */
function suggestOptionValue(sug: StructureSuggestion | undefined): string {
  if (!sug) return '';
  if (sug.gated) return 'gated';
  return sug.suggested ? 'suggested' : 'eligible';
}

/** Ranking-table columns for the ix-structure DataTable. `draftPatterns`/
 *  `contagionByStructure` are per-render (selection + checked-structure
 *  dependent), so this is rebuilt via `useMemo` rather than module-level. */
function buildStructureColumns(opts: {
  draftKeys: ReadonlySet<string>;
  volKey: (s: FlowDiscoveryStructure) => string;
  contagionByStructure: Map<string, number | null>;
  suggestionByStructure: Map<string, StructureSuggestion>;
  liftDefined: boolean;
  onToggle: (labels: string[]) => void;
}): ColumnDef<FlowDiscoveryStructure>[] {
  const { draftKeys, volKey, contagionByStructure, suggestionByStructure, liftDefined, onToggle } =
    opts;

  return [
    {
      key: 'vol',
      label: 'Vol',
      tooltip: helpText(DISCOVERY_COL_HELP.vol),
      render: (s) => (
        <Checkbox
          checked={draftKeys.has(volKey(s))}
          onChange={() => onToggle(s.ix_labels)}
        />
      ),
      // Draft state is a slice of the table, not only a click target: filter to the
      // open rows to work through what is left, or to the staged ones to review
      // what a bulk button just added.
      filterOptions: [
        { value: 'staged', label: 'staged' },
        { value: 'open', label: 'not staged' },
      ],
      filterOptionValue: (s) => (draftKeys.has(volKey(s)) ? 'staged' : 'open'),
      filterTitle:
        'Show only the rows already in the draft ix_patterns, or only the ones still open',
      sortValue: (s) => (draftKeys.has(volKey(s)) ? 1 : 0),
      searchValue: () => '',
    },
    {
      key: 'suggested',
      label: 'Auto',
      tooltip: helpText(DISCOVERY_COL_HELP.suggested),
      render: (s) => {
        const sug = suggestionByStructure.get(JSON.stringify(s.ix_labels));
        if (!sug) return <span className="text-text-dim/50">—</span>;
        // One explanation for every state — a gated row says WHICH gate, and a
        // near-miss lists the families that fell short, not just the ones that
        // fired (a row with nothing strong would otherwise explain nothing).
        const why = suggestExplain(sug);
        if (sug.gated) {
          return (
            <span className="text-text-dim/40" title={why}>
              —
            </span>
          );
        }
        const pct = `${fmt(sug.score * 100, 0)}%`;
        if (sug.suggested) {
          return (
            <Badge variant="warning" size="sm" title={why}>
              vol {pct}
            </Badge>
          );
        }
        return (
          <span className={signalGradeClass(sug.score)} title={why}>
            {pct}
          </span>
        );
      },
      // Rank by band first (suggested → eligible-but-under → gated), then by
      // score within the band — so sorting surfaces the check-me rows on top
      // instead of interleaving badges with dim scores and "—" gated rows.
      sortValue: (s) => {
        const sug = suggestionByStructure.get(JSON.stringify(s.ix_labels));
        if (!sug) return -1;
        const band = sug.suggested ? 2 : sug.gated ? 0 : 1;
        return band + sug.score;
      },
      // The same three bands as an exclusive choice. `eligible` is the one worth a
      // filter of its own: those rows carry evidence but fell under the badge bar,
      // and they are invisible in a table sorted by anything else.
      filterOptions: [
        { value: 'suggested', label: 'suggested' },
        { value: 'eligible', label: 'eligible' },
        { value: 'gated', label: 'gated' },
      ],
      filterOptionValue: (s) =>
        suggestOptionValue(suggestionByStructure.get(JSON.stringify(s.ix_labels))),
      filterTitle:
        'suggested = badged (cleared the gates and scored over the bar); eligible = scored but under it; gated = excluded before any evidence was weighed',
      searchValue: () => '',
    },
    {
      key: 'structure',
      label: 'Structure',
      tooltip: helpText(DISCOVERY_COL_HELP.structure),
      render: (s) => <IxLabelsDisplay labels={s.ix_labels} copyJson />,
      searchValue: (s) => formatIxLabelsText(s.ix_labels),
      // The SAME grammar the fingerprint table's ix_labels column filters with, so
      // a shape copied out of one table selects its row in the other: a JSON array
      // is an ordered-exact match, plain text a substring-any over the labels.
      filterMatch: (s, raw) => ixLabelsMatchFilter(s.ix_labels, raw),
      filterPlaceholder: IX_LABELS_FILTER_PLACEHOLDER,
      filterTitle: IX_LABELS_FILTER_TITLE,
    },
    {
      key: 'budget',
      label: 'Budget',
      tooltip: helpText(DISCOVERY_COL_HELP.budget),
      render: (s) => {
        const top = dominantFeeVariant(s);
        if (!top) {
          // No variant at all: either the result predates the field, or every trade
          // of this build carries no reading. Both mean "nothing to pin", and the
          // unknown count is what tells them apart.
          return (
            <span className="text-text-dim/50" title={feeVariantTitle(s)}>
              {s.fee_unknown_trades ? 'unknown' : '—'}
            </span>
          );
        }
        const n = s.fee_variant_count ?? 1;
        return (
          <span className="whitespace-nowrap" title={feeVariantTitle(s)}>
            <span className={n === 1 ? 'text-text' : 'text-text-dim'}>
              {formatFeeVariant(top)}
            </span>
            {n > 1 && <span className="ml-1 text-text-dim/60">+{n - 1}</span>}
          </span>
        );
      },
      // Rank by how concentrated the build's budget is, not by the budget itself: a
      // single triple covering every trade is the pinnable row, and that is the
      // question this column exists to answer.
      sortValue: (s) => feeConcentration(s),
      filterOptions: [
        { value: 'single', label: 'one budget' },
        { value: 'few', label: '2-4 budgets' },
        { value: 'many', label: '5+ budgets' },
        { value: 'unknown', label: 'not captured' },
      ],
      filterOptionValue: (s) => feeSpreadBand(s),
      filterTitle:
        'one budget = a compiled-in preset, safe to pin; 5+ = a fee oracle, where pinning any single value fires once and never again; not captured = no fee reading on any trade of this build yet',
      searchValue: (s) => {
        const top = dominantFeeVariant(s);
        return top ? formatFeeVariant(top) : '';
      },
    },
    {
      key: 'side',
      label: 'Side',
      tooltip: helpText(DISCOVERY_COL_HELP.side),
      render: (s) => {
        const side = sideOf(s);
        if (side === 'buy') return <Badge variant="buy" size="sm">buy-only</Badge>;
        if (side === 'sell') return <Badge variant="sell" size="sm">sell-only</Badge>;
        return <Badge variant="neutral" size="sm">both sides</Badge>;
      },
      sortValue: (s) => sideOf(s),
      // Exclusive, not substring: `both` contains `b` and so does `buy`, so a text
      // filter here answers a question nobody asked.
      filterOptions: [
        { value: 'buy', label: 'buy-only' },
        { value: 'sell', label: 'sell-only' },
        { value: 'both', label: 'both sides' },
      ],
      filterOptionValue: (s) => sideOf(s),
      filterTitle:
        'A one-sided shape only ever buys or only ever sells; a two-sided one round-trips (read Wash next to it)',
      searchValue: (s) => sideOf(s),
    },
    {
      key: 'firstSlot',
      label: 'Launch%',
      tooltip: helpText(DISCOVERY_COL_HELP.firstSlot),
      render: (s) => {
        const purity = firstSlotPurity(s);
        if (purity == null) {
          return (
            <span
              className="text-text-dim/50"
              title={
                s.first_slot_gross_sol == null
                  ? 'Result cached before the launch split existed — re-run discovery'
                  : 'No scored volume on this shape'
              }
            >
              —
            </span>
          );
        }
        const pct = fmt(purity * 100, 0);
        // Any shape that traded in a creation slot gets a badge, same read-at-a-
        // glance convention the Auto column uses for a Suggested row — the badge
        // marks exactly the rows the button will take, so the % it shows is the
        // purity (informational), not the reason it's badged.
        if (isFirstSlotPresent(s)) {
          return (
            <Badge variant="info" size="sm" title={`${s.first_slot_trades ?? 0} creation-slot trades`}>
              launch {pct}%
            </Badge>
          );
        }
        return <span className={pctGrade(purity * 100)}>{pct}</span>;
      },
      sortValue: (s) => firstSlotPurity(s),
      filterNumber: (s) => {
        const purity = firstSlotPurity(s);
        return purity == null ? null : purity * 100;
      },
      // Two questions share this column and only one of them is the number: what
      // the launch buttons act on is PRESENCE (`first_slot_trades > 0`), while the
      // % beside it is the purity. So the box takes either — the two keywords, or
      // the ordinary numeric grammar, delegated to the shared parser rather than
      // re-implemented here.
      filterMatch: (s, raw) => {
        const text = raw.trim().toLowerCase();
        if (text === 'launch') return isFirstSlotPresent(s);
        if (text === 'none') return !isFirstSlotPresent(s);
        const numeric = parseNumericPredicate(text);
        if (!numeric) return false;
        const purity = firstSlotPurity(s);
        return purity != null && numeric(purity * 100);
      },
      filterPlaceholder: 'launch  none  >50',
      filterTitle:
        'launch = traded in some matched token creation slot (exactly what the launch buttons stage); none = did not. Anything else reads as the % of this shape gross that landed in a creation slot: >  >=  <  <=  =  != or a range like 20..80.',
      searchValue: () => '',
    },
    {
      key: 'lift',
      label: 'Lift ×',
      tooltip: helpText(DISCOVERY_COL_HELP.lift),
      // A run with no out-of-group baseline scores every shape at exactly 1.00.
      // Printing that reads as a verdict ("ambient everywhere") when it is really
      // "not measured here" — so show it as unmeasured.
      render: (s) =>
        liftDefined ? (
          <span className={liftGrade(s.group_lift)}>{fmt(s.group_lift, 2)}</span>
        ) : (
          <span
            className="text-text-dim/50"
            title="No baseline — this group is the whole scored corpus, so lift is 1.00 by construction. Group by a field (or drop the fingerprint scope) to measure it."
          >
            —
          </span>
        ),
      sortValue: (s) => (liftDefined ? s.group_lift : null),
      filterNumber: (s) => (liftDefined ? s.group_lift : null),
      searchValue: () => '',
    },
    {
      key: 'share',
      label: 'Share%',
      tooltip: helpText(DISCOVERY_COL_HELP.share),
      render: (s) => fmt(s.volume_share),
      sortValue: (s) => s.volume_share,
      filterNumber: (s) => s.volume_share,
      searchValue: () => '',
    },
    {
      key: 'wash',
      label: 'Wash 0–1',
      tooltip: helpText(DISCOVERY_COL_HELP.wash),
      render: (s) => {
        const side = sideOf(s);
        return (
          <span
            className={side === 'both' ? washGrade(s.wash_symmetry) : undefined}
            title={
              side === 'both'
                ? undefined
                : 'Wash ≈1 is trivial for a one-sided row — check Contagion% instead'
            }
          >
            {fmt(s.wash_symmetry, 2)}
          </span>
        );
      },
      sortValue: (s) => s.wash_symmetry,
      filterNumber: (s) => s.wash_symmetry,
      searchValue: () => '',
    },
    {
      key: 'recur',
      label: 'Recur%',
      tooltip: helpText(DISCOVERY_COL_HELP.recur),
      render: (s) => (
        <span className={pctGrade(s.cross_token_recurrence)}>
          {fmt(s.cross_token_recurrence)}
        </span>
      ),
      sortValue: (s) => s.cross_token_recurrence,
      filterNumber: (s) => s.cross_token_recurrence,
      searchValue: () => '',
    },
    {
      key: 'burst',
      label: 'Burst%',
      tooltip: helpText(DISCOVERY_COL_HELP.burst),
      render: (s) => <span className={pctGrade(s.slot_burst)}>{fmt(s.slot_burst)}</span>,
      sortValue: (s) => s.slot_burst,
      filterNumber: (s) => s.slot_burst,
      searchValue: () => '',
    },
    {
      key: 'reuse',
      label: 'Reuse 0–1',
      tooltip: helpText(DISCOVERY_COL_HELP.reuse),
      render: (s) => <span className={unitGrade(s.wallet_reuse)}>{fmt(s.wallet_reuse, 2)}</span>,
      sortValue: (s) => s.wallet_reuse,
      filterNumber: (s) => s.wallet_reuse,
      searchValue: () => '',
    },
    {
      key: 'overlap',
      label: 'Overlap 0–1',
      tooltip: helpText(DISCOVERY_COL_HELP.overlap),
      render: (s) => (
        <span className={unitGrade(s.wallet_overlap)}>{fmt(s.wallet_overlap, 2)}</span>
      ),
      sortValue: (s) => s.wallet_overlap,
      filterNumber: (s) => s.wallet_overlap,
      searchValue: () => '',
    },
    {
      key: 'gross',
      label: 'Gross◎',
      tooltip: helpText(DISCOVERY_COL_HELP.gross),
      render: (s) => fmt(s.gross_sol),
      sortValue: (s) => s.gross_sol,
      filterNumber: (s) => s.gross_sol,
      searchValue: () => '',
    },
    {
      key: 'contagion',
      label: 'Contagion%',
      tooltip: helpText(DISCOVERY_COL_HELP.contagion),
      render: (s) => {
        const c = contagionByStructure.get(JSON.stringify(s.ix_labels)) ?? null;
        if (c == null) return <span className="text-text-dim/50">—</span>;
        return <span className={pctGrade(c)}>{fmt(c)}</span>;
      },
      sortValue: (s) => contagionByStructure.get(JSON.stringify(s.ix_labels)) ?? null,
      // Defined against the CURRENT draft, so this filter moves as you stage rows —
      // which is the point: `>50` reads as "whose wallets I have already tagged".
      filterNumber: (s) => contagionByStructure.get(JSON.stringify(s.ix_labels)) ?? null,
      searchValue: () => '',
    },
  ];
}

/** Ranking table itself, split out so its `columns` memo only recomputes on
 *  selection/checked-pattern changes, not on every `FlowDiscoveryPage` render. */
export function StructureTable({
  structures,
  draftKeys,
  volKey = structureRowKey,
  contagionByStructure,
  suggestionByStructure,
  liftDefined,
  previewKeys,
  onToggle,
  onFilteredRowsChange,
}: {
  structures: FlowDiscoveryStructure[];
  /** Membership the Vol checkbox tests — label keys (tagged/dump) or grain ids (working). */
  draftKeys: ReadonlySet<string>;
  /** How a ranked row maps into {@link draftKeys}. Default = exact ix_labels. */
  volKey?: (s: FlowDiscoveryStructure) => string;
  contagionByStructure: Map<string, number | null>;
  suggestionByStructure: Map<string, StructureSuggestion>;
  /** The group's `lift_defined` — false ⇒ lift is 1.00 by construction, render
   *  it as unmeasured rather than as a verdict. */
  liftDefined: boolean;
  /** `JSON.stringify(ix_labels)` of the rows a hovered bulk-select would add —
   *  outlined so the button's effect is visible BEFORE it's pressed. */
  previewKeys?: ReadonlySet<string>;
  onToggle: (labels: string[]) => void;
  /** Every row surviving the search + column filters. Pagination is off, so this
   *  is exactly what is on screen — which is what lets the page's stage/unstage
   *  buttons act on "what you are looking at". Pass a stable (useCallback)
   *  handler. */
  onFilteredRowsChange?: (rows: FlowDiscoveryStructure[]) => void;
}) {
  const draftKeysSig = [...draftKeys].join('|');
  const columns = useMemo(
    () =>
      buildStructureColumns({
        draftKeys,
        volKey,
        contagionByStructure,
        suggestionByStructure,
        liftDefined,
        onToggle,
      }),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- draftKeysSig/contagion/suggestion maps are the real identities
    [draftKeysSig, volKey, contagionByStructure, suggestionByStructure, liftDefined, onToggle],
  );
  const rowClassName = useCallback(
    (s: FlowDiscoveryStructure) => {
      const key = structureRowKey(s);
      if (previewKeys?.has(key)) return 'bg-accent/12 outline outline-1 outline-accent/50';
      return draftKeys.has(volKey(s)) ? 'bg-accent/8' : undefined;
    },
    [previewKeys, draftKeys, volKey],
  );

  return (
    <DataTable
      columns={columns}
      rows={structures}
      rowKey={structureRowKey}
      rowClassName={rowClassName}
      tableId={TABLE_ID}
      searchable
      searchPlaceholder="Search structures…"
      colFilters
      colToggle
      pinnable
      // Open on the check-me rows (Auto's band-then-score order). Cycling that
      // header past `desc` drops the key and restores the order `structures`
      // arrives in, which is the server's own rank.
      defaultSort={{ col: 'suggested', dir: 'desc' }}
      selectable={false}
      // The backend truncates each group to MAX_STRUCTURES_PER_GROUP, so the whole
      // group fits on one page — which is what lets "filtered" and "on screen" be
      // the same set for the stage/unstage-filtered buttons.
      paginate={false}
      emptyMessage="No structure matches these filters."
      onFilteredRowsChange={onFilteredRowsChange}
    />
  );
}
