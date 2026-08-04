import { useMemo } from 'react';

import { DataTable } from 'components/table/DataTable';
import type { ColumnDef } from 'components/table/types';
import { Badge } from 'components/ui/Badge';
import { Checkbox } from 'components/ui/Checkbox';
import { IxLabelsDisplay } from 'components/ui/IxLabelsDisplay';
import { formatIxLabelsText } from 'lib/ixLabels';
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

/** Ranking-table columns for the ix-structure DataTable. `draftPatterns`/
 *  `contagionByStructure` are per-render (selection + checked-structure
 *  dependent), so this is rebuilt via `useMemo` rather than module-level. */
function buildStructureColumns(opts: {
  draftPatterns: string[][];
  contagionByStructure: Map<string, number | null>;
  suggestionByStructure: Map<string, StructureSuggestion>;
  liftDefined: boolean;
  onToggle: (labels: string[]) => void;
}): ColumnDef<FlowDiscoveryStructure>[] {
  const { draftPatterns, contagionByStructure, suggestionByStructure, liftDefined, onToggle } =
    opts;
  const draftKeys = new Set(draftPatterns.map((p) => JSON.stringify(p)));

  return [
    {
      key: 'vol',
      label: 'Vol',
      tooltip: helpText(DISCOVERY_COL_HELP.vol),
      render: (s) => (
        <Checkbox
          checked={draftKeys.has(JSON.stringify(s.ix_labels))}
          onChange={() => onToggle(s.ix_labels)}
        />
      ),
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
        // fired (a row with nothing strong used to explain nothing at all).
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
      searchValue: () => '',
    },
    {
      key: 'structure',
      label: 'Structure',
      tooltip: helpText(DISCOVERY_COL_HELP.structure),
      render: (s) => <IxLabelsDisplay labels={s.ix_labels} copyJson />,
      searchValue: (s) => formatIxLabelsText(s.ix_labels),
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
      searchValue: () => '',
    },
  ];
}

/** Ranking table itself, split out so its `columns` memo only recomputes on
 *  selection/checked-pattern changes, not on every `FlowDiscoveryPage` render. */
export function StructureTable({
  structures,
  draftPatterns,
  contagionByStructure,
  suggestionByStructure,
  liftDefined,
  previewKeys,
  onToggle,
}: {
  structures: FlowDiscoveryStructure[];
  draftPatterns: string[][];
  contagionByStructure: Map<string, number | null>;
  suggestionByStructure: Map<string, StructureSuggestion>;
  /** The group's `lift_defined` — false ⇒ lift is 1.00 by construction, render
   *  it as unmeasured rather than as a verdict. */
  liftDefined: boolean;
  /** `JSON.stringify(ix_labels)` of the rows a hovered bulk-select would add —
   *  outlined so the button's effect is visible BEFORE it's pressed. */
  previewKeys?: ReadonlySet<string>;
  onToggle: (labels: string[]) => void;
}) {
  const draftKeysSig = draftPatterns.map((p) => JSON.stringify(p)).join('|');
  const columns = useMemo(
    () =>
      buildStructureColumns({
        draftPatterns,
        contagionByStructure,
        suggestionByStructure,
        liftDefined,
        onToggle,
      }),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- draftKeysSig/contagion/suggestion maps are the real identities
    [draftKeysSig, contagionByStructure, suggestionByStructure, liftDefined, onToggle],
  );
  return (
    <DataTable
      columns={columns}
      rows={structures}
      rowKey={(s) => JSON.stringify(s.ix_labels)}
      rowClassName={(s) => {
        const key = JSON.stringify(s.ix_labels);
        if (previewKeys?.has(key)) return 'bg-accent/12 outline outline-1 outline-accent/50';
        return draftPatterns.some((p) => JSON.stringify(p) === key) ? 'bg-accent/8' : undefined;
      }}
      searchable={false}
      colFilters={false}
      colToggle={false}
      selectable={false}
      paginate={false}
    />
  );
}
