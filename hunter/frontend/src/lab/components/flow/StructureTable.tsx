import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from 'react';

import { DataTable } from 'components/table/DataTable';
import { LENS_COLORS } from 'components/token-price-chart/constants';
import { LensButton } from 'components/tokens/LensControls';
import { parseNumericPredicate } from 'components/table/numericFilter';
import type { ColumnDef } from 'components/table/types';
import { Badge } from 'components/ui/Badge';
import { Checkbox } from 'components/ui/Checkbox';
import { IxLabelsDisplay } from 'components/ui/IxLabelsDisplay';
import {
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
import { tagLabel } from 'lib/flow/tapeClassify';
import { DISCOVERY_COL_HELP, type HelpTip } from 'lib/strategy/strategyHelp';
import type { FlowDiscoveryFeeVariant, FlowDiscoveryStructure } from 'types';

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

/** How the draft holds a ranked row: `on` = that row or its family's broad row is
 *  listed, `part` = only other narrow rows of its family are, `off` = none. */
export type RankedState = 'on' | 'part' | 'off';

const STATE_FILTER: Record<RankedState, string> = { on: 'staged', part: 'part', off: 'open' };
const STATE_SORT: Record<RankedState, number> = { on: 2, part: 1, off: 0 };

/** A checkbox with the third, partly-staged state. `indeterminate` is a DOM
 *  property with no attribute, so it is set on the element. */
function StateCheckbox({ state, onChange, title }: { state: RankedState; onChange: () => void; title: string }) {
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (ref.current) ref.current.indeterminate = state === 'part';
  }, [state]);
  return <Checkbox ref={ref} checked={state === 'on'} onChange={onChange} title={title} />;
}

/** The preview chart's highlight as the ranked table arms it - the same lenses the
 *  chart's trades table arms (`useTokenHighlight`, owned by the page), so either
 *  table lights the other's rows and the chart washes both. */
export interface RankedLens {
  /** Arm or disarm the row's shape (its core on the core table), any budget. */
  toggle: (s: FlowDiscoveryStructure) => void;
  /** The color the row's own lens washes in, `null` = not armed. */
  colorOf: (s: FlowDiscoveryStructure) => string | null;
  /** Arm or disarm the row's shape pinned to one budget. */
  toggleBudget: (s: FlowDiscoveryStructure, v: FlowDiscoveryFeeVariant) => void;
  budgetColorOf: (s: FlowDiscoveryStructure, v: FlowDiscoveryFeeVariant) => string | null;
  /** Colors of every armed structure lens that covers this row - its own, a
   *  budget's, one armed from the trades table, or a core over an exact row. */
  rowColors: (s: FlowDiscoveryStructure) => string[];
}

/** How the draft holds one budget of a row: `on` = its pinned row is listed,
 *  `broad` = the shape's unpinned row is (covering every budget), `off` = neither,
 *  `none` = the fee-pin fields on pick nothing this budget carries, so a check
 *  would stage the bare shape. */
export type BudgetState = 'on' | 'broad' | 'off' | 'none';

/** The Budget cell: the head budget, and behind the arrow every budget as its own
 *  checkable line. Expansion is per cell, so it survives a re-sort. */
function BudgetCell({
  s,
  budgetStateOf,
  onToggleBudget,
  lens,
  tag,
}: {
  s: FlowDiscoveryStructure;
  budgetStateOf: (s: FlowDiscoveryStructure, v: FlowDiscoveryFeeVariant) => BudgetState;
  onToggleBudget: (labels: string[], v: FlowDiscoveryFeeVariant) => void;
  lens: RankedLens | null;
  tag: string;
}) {
  const [open, setOpen] = useState(false);
  const variants = s.fee_variants ?? [];
  const top = variants[0];
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
  const n = s.fee_variant_count ?? variants.length;
  const staged = variants.filter((v) => budgetStateOf(s, v) === 'on').length;
  const known = variants.reduce((sum, v) => sum + v.n_trades, 0);
  return (
    <div className="flex flex-col items-start gap-1">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        className="inline-flex items-center gap-1 whitespace-nowrap hover:text-text"
        title={`${feeVariantTitle(s)}\n\nClick to ${open ? 'hide' : 'list'} every budget with its own check.`}
      >
        <span className="text-text-dim/60">{open ? '▾' : '▸'}</span>
        <span className={n === 1 ? 'text-text' : 'text-text-dim'}>{formatFeeVariant(top)}</span>
        {n > 1 && <span className="text-text-dim/60">+{n - 1}</span>}
        {staged > 0 && (
          <Badge variant="info" size="sm">
            {staged} pinned
          </Badge>
        )}
      </button>
      {open && (
        <div className="flex flex-col gap-0.5 text-left text-[11px]">
          {variants.map((v) => {
            const state = budgetStateOf(s, v);
            const pct = known > 0 ? ((v.n_trades / known) * 100).toFixed(0) : '0';
            const lensColor = lens?.budgetColorOf(s, v) ?? null;
            return (
              <label
                key={`${v.cu_limit}|${v.cu_price}|${v.tip_lamports}`}
                className="inline-flex items-center gap-1.5 whitespace-nowrap"
              >
                {lens && (
                  <LensButton
                    armed={lensColor != null}
                    color={lensColor ?? LENS_COLORS.structure[0]}
                    title={
                      lensColor != null
                          ? 'Stop highlighting this budget'
                          : 'Highlight every candle and trade of this shape on this budget (the highlight pin fields when any are on, else all three)'
                    }
                    onClick={() => lens.toggleBudget(s, v)}
                  />
                )}
                <Checkbox
                  checked={state === 'on' || state === 'broad'}
                  disabled={state === 'none'}
                  onChange={() => onToggleBudget(s.ix_labels, v)}
                  title={
                    state === 'none'
                      ? 'The fee-pin fields on pick nothing this budget carries - turn on a field it has'
                      : state === 'broad'
                        ? `In ${tag} through the whole-shape row (any budget). Click to narrow the shape to this budget only.`
                        : state === 'on'
                          ? `In ${tag} pinned to this budget. Click to remove it; other budgets stay.`
                          : `Add this shape to ${tag} pinned to this budget only.`
                  }
                />
                <span className="text-text">{formatFeeVariant(v)}</span>
                <span className="text-text-dim">
                  {v.n_trades.toLocaleString()} tr · {pct}% · {fmt(v.gross_sol)}◎
                </span>
              </label>
            );
          })}
          {n > variants.length && (
            <span className="text-text-dim/60">… {n - variants.length} more, stage from the trades table</span>
          )}
          {(s.fee_unknown_trades ?? 0) > 0 && (
            <span className="text-text-dim/60">
              {(s.fee_unknown_trades ?? 0).toLocaleString()} trades with no fee reading
            </span>
          )}
        </div>
      )}
    </div>
  );
}

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
  stateOf: (s: FlowDiscoveryStructure) => RankedState;
  contagionByStructure: Map<string, number | null>;
  suggestionByStructure: Map<string, StructureSuggestion>;
  liftDefined: boolean;
  onToggle: (labels: string[]) => void;
  budgetStateOf: (s: FlowDiscoveryStructure, v: FlowDiscoveryFeeVariant) => BudgetState;
  onToggleBudget: (labels: string[], v: FlowDiscoveryFeeVariant) => void;
  lens: RankedLens | null;
  tagName: string | null;
}): ColumnDef<FlowDiscoveryStructure>[] {
  const {
    stateOf,
    contagionByStructure,
    suggestionByStructure,
    liftDefined,
    onToggle,
    budgetStateOf,
    onToggleBudget,
    lens,
    tagName,
  } = opts;
  const tag = tagName ? tagLabel(tagName) : 'the tag';

  return [
    {
      key: 'vol',
      label: tagName ? tagLabel(tagName) : 'Vol',
      tooltip: helpText(DISCOVERY_COL_HELP.vol),
      render: (s) => {
        const state = stateOf(s);
        return (
          <StateCheckbox
            state={state}
            onChange={() => onToggle(s.ix_labels)}
            title={
              state === 'part'
                ? `Only narrower rows of this shape are in ${tag} (fee, side or marks pins). Click to widen to the whole shape.`
                : state === 'on'
                  ? `In ${tag}. Click to remove.`
                  : `Add to ${tag}.`
            }
          />
        );
      },
      // Draft state is a slice of the table, not only a click target: filter to the
      // open rows to work through what is left, or to the staged ones to review
      // what a bulk button just added.
      filterOptions: [
        { value: 'staged', label: 'staged' },
        { value: 'part', label: 'partly staged' },
        { value: 'open', label: 'not staged' },
      ],
      filterOptionValue: (s) => STATE_FILTER[stateOf(s)],
      filterTitle: `Show only the rows already in the draft for ${tag}, the ones only narrower rows cover, or the ones still open`,
      sortValue: (s) => STATE_SORT[stateOf(s)],
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
      render: (s) => {
        if (!lens) return <IxLabelsDisplay labels={s.ix_labels} copyJson />;
        const own = lens.colorOf(s);
        return (
          <span className="flex items-start gap-1">
            <LensButton
              armed={own != null}
              color={own ?? LENS_COLORS.structure[0]}
              litBy={lens.rowColors(s)[0] ?? null}
              title={
                own != null
                    ? 'Stop highlighting this ix structure'
                    : 'Highlight every candle and trade with this structure, any budget'
              }
              onClick={() => lens.toggle(s)}
            />
            <IxLabelsDisplay labels={s.ix_labels} copyJson />
          </span>
        );
      },
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
      render: (s) => (
        <BudgetCell s={s} budgetStateOf={budgetStateOf} onToggleBudget={onToggleBudget} lens={lens} tag={tag} />
      ),
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
      searchValue: (s) => (s.fee_variants ?? []).map(formatFeeVariant).join(' | '),
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
  stateOf,
  contagionByStructure,
  suggestionByStructure,
  liftDefined,
  previewKeys,
  onToggle,
  budgetStateOf,
  onToggleBudget,
  lens = null,
  onFilteredRowsChange,
  tagName = null,
}: {
  structures: FlowDiscoveryStructure[];
  /** How the draft holds each row - the check column's three states. */
  stateOf: (s: FlowDiscoveryStructure) => RankedState;
  contagionByStructure: Map<string, number | null>;
  suggestionByStructure: Map<string, StructureSuggestion>;
  /** The group's `lift_defined` — false ⇒ lift is 1.00 by construction, render
   *  it as unmeasured rather than as a verdict. */
  liftDefined: boolean;
  /** `JSON.stringify(ix_labels)` of the rows a hovered bulk-select would add —
   *  outlined so the button's effect is visible BEFORE it's pressed. */
  previewKeys?: ReadonlySet<string>;
  onToggle: (labels: string[]) => void;
  /** How the draft holds one budget of a row - the Budget cell's per-budget checks. */
  budgetStateOf: (s: FlowDiscoveryStructure, v: FlowDiscoveryFeeVariant) => BudgetState;
  /** Stage or unstage the shape pinned to one budget. */
  onToggleBudget: (labels: string[], v: FlowDiscoveryFeeVariant) => void;
  /** The preview chart's highlight; absent = no lens buttons. Pass a memoized one. */
  lens?: RankedLens | null;
  /** Every row surviving the search + column filters. Pagination is off, so this
   *  is exactly what is on screen — which is what lets the page's stage/unstage
   *  buttons act on "what you are looking at". Pass a stable (useCallback)
   *  handler. */
  onFilteredRowsChange?: (rows: FlowDiscoveryStructure[]) => void;
  /** The tag a check stages into, naming the check column (`@volume`). */
  tagName?: string | null;
}) {
  const columns = useMemo(
    () =>
      buildStructureColumns({
        stateOf,
        contagionByStructure,
        suggestionByStructure,
        liftDefined,
        onToggle,
        budgetStateOf,
        onToggleBudget,
        lens,
        tagName,
      }),
    [
      stateOf,
      contagionByStructure,
      suggestionByStructure,
      liftDefined,
      onToggle,
      budgetStateOf,
      onToggleBudget,
      lens,
      tagName,
    ],
  );
  // A lensed row washes in its lens color, the trades table's convention
  // (`--row-lens`), so a structure armed in either table reads as one pick here.
  const rowClassName = useCallback(
    (s: FlowDiscoveryStructure) => {
      const key = structureRowKey(s);
      if (previewKeys?.has(key)) return 'bg-accent/12 outline outline-1 outline-accent/50';
      if (lens && lens.rowColors(s).length > 0) return 'bg-(--row-lens)/16 hover:bg-(--row-lens)/24';
      return stateOf(s) === 'on' ? 'bg-accent/8' : undefined;
    },
    [previewKeys, stateOf, lens],
  );
  const rowStyle = useMemo(() => {
    if (!lens) return undefined;
    return (s: FlowDiscoveryStructure): CSSProperties | undefined => {
      const colors = lens.rowColors(s);
      return colors.length > 0 ? ({ '--row-lens': colors[0] } as CSSProperties) : undefined;
    };
  }, [lens]);

  return (
    <DataTable
      columns={columns}
      rows={structures}
      rowKey={structureRowKey}
      rowClassName={rowClassName}
      rowStyle={rowStyle}
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
      // Unpaginated: `onFilteredRowsChange` reports the rows on screen, and the
      // stage/unstage-filtered buttons must act on every row the filters keep. The
      // backend's trade floor (MIN_STRUCTURE_TRADES) is what keeps the list short.
      paginate={false}
      emptyMessage="No structure matches these filters."
      onFilteredRowsChange={onFilteredRowsChange}
    />
  );
}
