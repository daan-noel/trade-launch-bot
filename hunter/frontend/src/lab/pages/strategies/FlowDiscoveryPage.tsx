import { Fragment, useEffect, useMemo, useRef, useState } from 'react';
import { Link, useSearchParams } from 'react-router-dom';

import { Accordion } from 'components/ui/Accordion';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { Badge } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { IconButton } from 'components/ui/IconButton';
import {
  CheckIcon,
  CloseIcon,
  EditIcon,
  LinkIcon,
  PlayIcon,
  ReuseIcon,
  SpinnerIcon,
} from 'components/ui/icons';
import { Checkbox } from 'components/ui/Checkbox';
import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { InlineAlert } from 'components/ui/Modal';
import { DataTable } from 'components/table/DataTable';
import type { ColumnDef } from 'components/table/types';
import { VolumeIxPatternsEditor } from 'components/strategy/VolumeIxPatternsEditor';
import {
  fingerprintParamsCell,
  chip as paramChip,
  axisTint,
} from 'components/strategy/FingerprintParamsSummary';
import { LabelTip } from 'components/strategy/LabelTip';
import { IxLabelsDisplay } from 'components/ui/IxLabelsDisplay';
import { useLocalStorage } from 'hooks/useLocalStorage';
import { apiErrorMessage } from 'store/baseApi';
import {
  useGetFingerprintsQuery,
  useGetTokenDetailQuery,
  useGetTokenTradesQuery,
  useUpdateFingerprintMutation,
} from 'store/sharedEndpoints';
import { fingerprintsHref, STRATEGY_PARAMS } from 'lib/strategy/nav';
import {
  metricConfigWithVolumePatterns,
  volumeIxPatternsFromConfig,
} from 'lib/strategy/registry';
import {
  DISCOVERY_COL_HELP,
  DISCOVERY_FIELD_HELP,
  SWEEP_FIELD_HELP,
  type HelpTip,
} from 'lib/strategy/strategyHelp';
import { formatIxLabelsText } from 'lib/ixLabels';
import { FlowPreviewChart } from '@lab/components/flow/FlowPreviewChart';
import { patternKeysFrom } from '@lab/lib/flow/classifyFlow';
import { FingerprintGroupPicker } from '@lab/components/sweep/FingerprintGroupPicker';
import { parseIxLabelsFilter, parseNumbers } from '@lab/components/sweep/fingerprintFilters';
import {
  GROUP_FIELD_LABELS,
  GROUP_FIELDS,
  SOL_BUCKET_WIDTH,
  type GroupField,
} from '@lab/components/sweep/groupedTypes';
import {
  useBackgroundJobActions,
  useBackgroundJobsState,
} from '@lab/context/BackgroundJobsContext';
import {
  useBindFlowDiscoveryMutation,
  useGetLastFlowDiscoveryQuery,
  useLazyGetFlowDiscoveryQuery,
  useStartFlowDiscoveryMutation,
  type FlowDiscoveryStartArgs,
} from '@lab/store/labEndpoints';
import type {
  FlowDiscoveryGroup,
  FlowDiscoveryResult,
  FlowDiscoveryStructure,
  FlowDiscoveryTokenGross,
  TradeRecord,
} from 'types';
import { findFingerprintForGroupKey } from 'lib/strategy/matchGroupFingerprint';
import { lamportsToSol, type Fingerprint } from 'lib/strategy/types';
import { tidySolDecimal } from 'utils/format';

interface DiscoveryConfig {
  createdAfter: string;
  createdBefore: string;
  groupBy: GroupField[];
  ixLabelsFilter: string;
  cashbackFilter: 'all' | 'true' | 'false';
  fieldFiltersText: Record<string, string>;
  minTokens: number;
  tokenCap: number;
  curveOnly: boolean;
  bucketWidthSol: number;
  /** Saved fingerprint whose match axes scope discovery (engine match SSOT). */
  seedFingerprintId: string | null;
}

const DEFAULTS: DiscoveryConfig = {
  createdAfter: '',
  createdBefore: '',
  groupBy: ['cu_limit'],
  ixLabelsFilter: '',
  cashbackFilter: 'all',
  fieldFiltersText: {},
  minTokens: 3,
  tokenCap: 5000,
  curveOnly: false,
  bucketWidthSol: SOL_BUCKET_WIDTH,
  seedFingerprintId: null,
};

/** Compact SOL text for filter fields (avoids `1.0000000001`). */
function solText(lamports: number | null | undefined): string {
  const s = lamportsToSol(lamports);
  if (s == null) return '';
  return String(Number(s.toPrecision(12)));
}

/** Fill group-by filters from a saved fingerprint so the picker mirrors its axes.
 *  Discovery run uses `fingerprint_id` for real engine matching (buckets included). */
function configFromFingerprint(fp: Fingerprint): Partial<DiscoveryConfig> {
  const fieldFiltersText: Record<string, string> = {};
  if (fp.cu_limit != null) fieldFiltersText.cu_limit = String(fp.cu_limit);
  if (fp.cu_price != null) fieldFiltersText.cu_price = String(fp.cu_price);
  const init = solText(fp.init_buy_lamports);
  if (init) fieldFiltersText.initial_buy_sol = init;
  const max = solText(fp.max_cost_lamports);
  if (max) fieldFiltersText.max_cost_lamports = max;
  const spend = solText(fp.spendable_lamports_in);
  if (spend) fieldFiltersText.spendable_lamports_in = spend;
  const fsBuy = solText(fp.first_slot_buy_lamports);
  if (fsBuy) fieldFiltersText.first_slot_buy_sol = fsBuy;
  const fsSell = solText(fp.first_slot_sell_lamports);
  if (fsSell) fieldFiltersText.first_slot_sell_sol = fsSell;
  return {
    seedFingerprintId: fp.id,
    // One ALL group over tokens that match this fingerprint.
    groupBy: [],
    fieldFiltersText,
    ixLabelsFilter: formatIxLabelsText(fp.ix_labels),
    bucketWidthSol: tidySolDecimal(
      fp.bucket_size_amount > 0 ? fp.bucket_size_amount : SOL_BUCKET_WIDTH,
    ),
    minTokens: 1,
    cashbackFilter: 'all',
  };
}

function toUtc(local: string): string | undefined {
  if (!local) return undefined;
  const d = new Date(local.endsWith('Z') ? local : `${local}Z`);
  return Number.isNaN(d.getTime()) ? undefined : d.toISOString();
}

function fmt(n: number, digits = 1): string {
  if (!Number.isFinite(n)) return '—';
  return n.toFixed(digits);
}

function groupKeyLabel(gk: Record<string, string>): string {
  const parts = Object.entries(gk).map(([k, v]) => `${k}=${v}`);
  return parts.length ? parts.join(' · ') : 'ALL';
}

/** Compact group-key label for a fingerprint NAME. Unlike `groupKeyLabel` this
 *  drops `∅` (absent) params — they carry no info in a name — and collapses the
 *  pipe-joined `ix_labels` sequence into a `Nix` count so the name stays short
 *  instead of listing every instruction. */
function groupKeyName(gk: Record<string, string>): string {
  const parts: string[] = [];
  for (const [k, v] of Object.entries(gk)) {
    if (v === MISSING_GROUP_VALUE) continue;
    if (k === 'ix_labels') {
      parts.push(`${v.split(' | ').length}ix`);
      continue;
    }
    const short = GROUP_FIELD_AXIS[k as GroupField] ?? k;
    parts.push(`${short}=${v}`);
  }
  return parts.length ? parts.join(' · ') : 'ALL';
}

/** Discovery `GroupField` → the `fingerprintParamsCell` axis-hue key for the
 *  same underlying concept, so a group-key chip gets the EXACT hue a
 *  fingerprint's own param chip would use for that axis (same SSOT palette,
 *  not a second hand-picked one). Fields with no fingerprint-axis counterpart
 *  (`is_cashback_enabled`) fall through to the hashed-hue fallback. */
const GROUP_FIELD_AXIS: Partial<Record<GroupField, string>> = {
  cu_limit: 'cu_limit',
  cu_price: 'cu_price',
  initial_buy_sol: 'init',
  max_cost_lamports: 'max',
  spendable_lamports_in: 'spend',
  first_slot_buy_sol: 'fs_buy',
  first_slot_sell_sol: 'fs_sell',
};

/** Backend sentinel for "field absent on this fingerprint" — mirrors
 *  `engine::grouping::MISSING`. */
const MISSING_GROUP_VALUE = '∅';

/** Selected-group header — reuses `fingerprintParamsCell`'s chip style +
 *  `axisTint` hue table so the group-key header reads consistently with the
 *  fingerprint-param chips shown a few lines below it on this same page,
 *  instead of the flat `key=value · key=value` string used by the sidebar
 *  list. `ix_labels` is excluded from the chip row — a pipe-joined instruction
 *  sequence doesn't compress into a `label=value` chip, so it renders as
 *  pretty-printed JSON via `IxLabelsDisplay` instead (same as the sweep
 *  table's group-key column). It DOES still get an `Nix` count chip in the
 *  row though — same `${ix.length}ix` chip `fingerprintParamsCell` renders for
 *  a fingerprint's own `ix_labels` axis, same `axisTint('ix')` hue. */
function groupKeyChips(gk: Record<string, string>) {
  const entries = Object.entries(gk);
  if (entries.length === 0) {
    return <span className="text-sm font-bold text-text">ALL tokens</span>;
  }
  const ixValue = gk.ix_labels;
  const ixParts =
    ixValue != null && ixValue !== MISSING_GROUP_VALUE ? ixValue.split(' | ') : null;
  const chipEntries = entries.filter(([k]) => k !== 'ix_labels');
  return (
    <div className="flex flex-col gap-1.5">
      {(chipEntries.length > 0 || ixParts) && (
        <div className="flex flex-wrap items-center gap-1.5">
          {chipEntries.map(([k, v]) => {
            const label = GROUP_FIELD_LABELS[k as GroupField] ?? k;
            const axisLabel = GROUP_FIELD_AXIS[k as GroupField] ?? k;
            return (
              <span key={k}>
                {paramChip(
                  <>
                    <span className="text-text-dim">{label}=</span>
                    {v}
                  </>,
                  { style: axisTint(axisLabel), title: `${label}: ${v}` },
                )}
              </span>
            );
          })}
          {ixParts && (
            <span>
              {paramChip(`${ixParts.length}ix`, {
                style: axisTint('ix'),
                title: formatIxLabelsText(ixParts),
              })}
            </span>
          )}
        </div>
      )}
      {ixValue != null && (
        <div className="flex items-start gap-1.5">
          <span className="pt-0.5 text-[9px] font-bold uppercase tracking-wider text-text-dim/80">
            {GROUP_FIELD_LABELS.ix_labels}:
          </span>
          <IxLabelsDisplay
            labels={ixValue === MISSING_GROUP_VALUE ? [] : ixValue.split(' | ')}
            copyJson
            empty={MISSING_GROUP_VALUE}
          />
        </div>
      )}
    </div>
  );
}

/** Buy-only / sell-only / both-sided (e.g. Axiom's shared swap ix), from a
 *  structure's buy_sol/sell_sol split — a 2% dust tolerance absorbs noise. */
function sideOf(s: { buy_sol: number; sell_sol: number }): 'buy' | 'sell' | 'both' {
  const total = s.buy_sol + s.sell_sol;
  if (total <= 0) return 'both';
  const buyFrac = s.buy_sol / total;
  if (buyFrac >= 0.98) return 'buy';
  if (buyFrac <= 0.02) return 'sell';
  return 'both';
}

/** Text color for a 0..1 "how bot/wash-like does this value look" grade —
 *  dim slate at 0 (no signal) fading up through warning into danger red as the
 *  signal strengthens. Mirrors the warning=volume convention used by the flow
 *  split bar above this table. */
function signalColor(t: number): string {
  const c = Math.max(0, Math.min(1, t));
  if (c <= 0.02) return 'rgba(148,163,184,0.55)';
  const warn: [number, number, number] = [244, 224, 122];
  const danger: [number, number, number] = [242, 54, 69];
  const r = Math.round(warn[0] + (danger[0] - warn[0]) * c);
  const g = Math.round(warn[1] + (danger[1] - warn[1]) * c);
  const b = Math.round(warn[2] + (danger[2] - warn[2]) * c);
  const a = (0.45 + 0.55 * c).toFixed(2);
  return `rgba(${r},${g},${b},${a})`;
}

/** Lift ≈1 is common-everywhere noise; ≥3× concentrated here is the strongest
 *  "this shape is this group's own tooling" signal (see DISCOVERY_COL_HELP.lift). */
const liftGrade = (lift: number) => signalColor((lift - 1) / 2);
/** Recur% / Burst% / Reuse are already 0..1 or 0..100 signals — higher = more bot-like. */
const pctGrade = (pct: number) => signalColor(pct / 100);
const unitGrade = (v: number) => signalColor(v);
/** Wash trends toward 0 as buys/sells cancel out (bot round-trip) — invert so
 *  the danger end lines up with "money went in a circle", not with 1. */
const washGrade = (wash: number) => signalColor(1 - wash);

const clamp01 = (t: number) => Math.max(0, Math.min(1, t));

// ── auto-suggest ("is this shape manufactured volume?") ──────────────────────
// A client-side composite of the bot-likelihood columns, so the human doesn't
// have to eyeball every row. Deliberately conservative: two hard gates, then a
// vote of the normalized signals. Nothing here is written to the backend — the
// backend still only flags degenerate `ambiguity`; this just pre-checks the
// rows a human almost certainly would (see DISCOVERY_COL_HELP.suggested).

/** Lift below this is "common everywhere" noise — never auto-flag (mirrors the
 *  backend `LIFT_AMBIGUOUS` used for the group `ambiguity` chip). */
const SUGGEST_MIN_LIFT = 1.25;
/** Size floor — ignore dust-sized shapes (mirrors backend `MIN_STRUCTURE_SOL`). */
const SUGGEST_MIN_GROSS = 0.05;
/** A normalized 0..1 signal at/above this counts as "strong". */
const SUGGEST_STRONG = 0.5;
/** How many strong signals a row needs before it's Suggested (a single hot
 *  column isn't enough — averages hide a lone spike). */
const SUGGEST_MIN_STRONG = 2;

interface StructureSuggestion {
  /** Cleared both gates AND tripped ≥ SUGGEST_MIN_STRONG signals. */
  suggested: boolean;
  /** Excluded by the lift/size gate before signals were even weighed. */
  gated: boolean;
  /** Mean of the applicable normalized signals (0..1) — the shown %. */
  score: number;
  /** Human labels of the signals that crossed SUGGEST_STRONG (the "why"). */
  reasons: string[];
}

/** Composite bot-likelihood verdict for one structure. `contagion` is the
 *  client-side Contagion% for this row (or null when nothing's checked / it's
 *  itself checked); it substitutes for Wash on one-sided rows, whose Wash is
 *  meaningless (see DISCOVERY_COL_HELP.side / .suggested). */
function suggestStructure(
  s: FlowDiscoveryStructure,
  side: 'buy' | 'sell' | 'both',
  contagion: number | null,
): StructureSuggestion {
  const gated = s.group_lift < SUGGEST_MIN_LIFT || s.gross_sol < SUGGEST_MIN_GROSS;
  const signals: { label: string; value: number }[] = [
    { label: 'Recur', value: clamp01(s.cross_token_recurrence / 100) },
    { label: 'Burst', value: clamp01(s.slot_burst / 100) },
    { label: 'Reuse', value: clamp01(s.wallet_reuse) },
  ];
  if (side === 'both') {
    signals.push({ label: 'Wash', value: clamp01(1 - s.wash_symmetry) });
  } else if (contagion != null) {
    signals.push({ label: 'Contagion', value: clamp01(contagion / 100) });
  }
  const strong = signals.filter((sig) => sig.value >= SUGGEST_STRONG);
  const score = signals.reduce((a, sig) => a + sig.value, 0) / signals.length;
  return {
    gated,
    score,
    reasons: strong.map((sig) => sig.label),
    suggested: !gated && strong.length >= SUGGEST_MIN_STRONG,
  };
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
  onToggle: (labels: string[]) => void;
}): ColumnDef<FlowDiscoveryStructure>[] {
  const { draftPatterns, contagionByStructure, suggestionByStructure, onToggle } = opts;
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
        if (sug.gated) {
          return (
            <span
              className="text-text-dim/40"
              title="Gated out — Lift × < 1.25 (ambient everywhere) or Gross◎ below the dust floor"
            >
              —
            </span>
          );
        }
        const pct = `${fmt(sug.score * 100, 0)}%`;
        if (sug.suggested) {
          return (
            <Badge variant="warning" size="sm" title={`Strong: ${sug.reasons.join(', ')}`}>
              vol {pct}
            </Badge>
          );
        }
        return (
          <span
            style={{ color: signalColor(sug.score) }}
            title={
              sug.reasons.length
                ? `Strong: ${sug.reasons.join(', ')} — needs ≥ 2 to auto-flag`
                : 'No strong signals'
            }
          >
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
      key: 'lift',
      label: 'Lift ×',
      tooltip: helpText(DISCOVERY_COL_HELP.lift),
      render: (s) => <span style={{ color: liftGrade(s.group_lift) }}>{fmt(s.group_lift, 2)}</span>,
      sortValue: (s) => s.group_lift,
      filterNumber: (s) => s.group_lift,
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
            style={side === 'both' ? { color: washGrade(s.wash_symmetry) } : undefined}
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
        <span style={{ color: pctGrade(s.cross_token_recurrence) }}>
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
      render: (s) => <span style={{ color: pctGrade(s.slot_burst) }}>{fmt(s.slot_burst)}</span>,
      sortValue: (s) => s.slot_burst,
      filterNumber: (s) => s.slot_burst,
      searchValue: () => '',
    },
    {
      key: 'reuse',
      label: 'Reuse 0–1',
      tooltip: helpText(DISCOVERY_COL_HELP.reuse),
      render: (s) => <span style={{ color: unitGrade(s.wallet_reuse) }}>{fmt(s.wallet_reuse, 2)}</span>,
      sortValue: (s) => s.wallet_reuse,
      filterNumber: (s) => s.wallet_reuse,
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
        return <span style={{ color: pctGrade(c) }}>{fmt(c)}</span>;
      },
      sortValue: (s) => contagionByStructure.get(JSON.stringify(s.ix_labels)) ?? null,
      searchValue: () => '',
    },
  ];
}


/**
 * Lab page: score trade ix-structures per fingerprint group, toggle volume
 * patterns, apply via fingerprint update or promote-style bind.
 */
export function FlowDiscoveryPage() {
  const [stored, setConfig] = useLocalStorage<DiscoveryConfig>(
    'hunter.lab.flowDiscovery.config',
    DEFAULTS,
  );
  const config: DiscoveryConfig = {
    ...DEFAULTS,
    ...stored,
    groupBy: (stored.groupBy ?? DEFAULTS.groupBy).filter((f): f is GroupField =>
      (GROUP_FIELDS as readonly string[]).includes(f),
    ),
  };
  const {
    createdAfter,
    createdBefore,
    groupBy,
    ixLabelsFilter,
    cashbackFilter,
    fieldFiltersText,
    minTokens,
    tokenCap,
    curveOnly,
    bucketWidthSol,
    seedFingerprintId,
  } = config;

  function setField<K extends keyof DiscoveryConfig>(key: K, value: DiscoveryConfig[K]) {
    setConfig((prev) => ({ ...DEFAULTS, ...prev, [key]: value }));
  }

  const ixLabelsGrouped = groupBy.includes('ix_labels');
  const ixFilter = useMemo(() => parseIxLabelsFilter(ixLabelsFilter), [ixLabelsFilter]);
  // When scoping by a saved fingerprint, filters are display-only (engine match
  // is the SSOT) — don't block Run on ix-filter parse errors.
  const ixFilterError =
    !seedFingerprintId && !ixLabelsGrouped ? ixFilter.error : null;

  const { markStarting, markFinished } = useBackgroundJobActions();
  const { isRunning } = useBackgroundJobsState();
  const running = isRunning('discovery', 'discovery');

  const [startDiscovery, startState] = useStartFlowDiscoveryMutation();
  const [fetchResult] = useLazyGetFlowDiscoveryQuery();
  const [bindFp, bindState] = useBindFlowDiscoveryMutation();
  const [updateFp, updateState] = useUpdateFingerprintMutation();
  const { data: fingerprints = [] } = useGetFingerprintsQuery();

  const seedFp =
    (seedFingerprintId && fingerprints.find((f) => f.id === seedFingerprintId)) || null;

  const [result, setResult] = useState<FlowDiscoveryResult | null>(null);
  const [selectedGroupIdx, setSelectedGroupIdx] = useState(0);
  const { data: lastResult } = useGetLastFlowDiscoveryQuery();
  // Rehydrate from the disk-cached last run on mount/reload — only when this
  // session hasn't produced (or already loaded) a result of its own.
  useEffect(() => {
    if (lastResult && !result) {
      setResult(lastResult);
      setSelectedGroupIdx(0);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- run once when the cached result arrives
  }, [lastResult]);
  /** Apply target — null ⇒ promote-style create/bind from the group key. */
  const [targetFpId, setTargetFpId] = useState<string | null>(seedFingerprintId);
  const [draftPatterns, setDraftPatterns] = useState<string[][]>([]);
  const [applyError, setApplyError] = useState<string | null>(null);
  const [applyOk, setApplyOk] = useState<string | null>(null);
  const [selectedTokenMint, setSelectedTokenMint] = useState<string | null>(null);
  const patternKeys = useMemo(() => patternKeysFrom(draftPatterns), [draftPatterns]);
  const { data: previewTrades = [], isFetching: previewTradesLoading } = useGetTokenTradesQuery(
    selectedTokenMint ?? '',
    { skip: !selectedTokenMint },
  );
  const { data: previewDetail } = useGetTokenDetailQuery(selectedTokenMint ?? '', {
    skip: !selectedTokenMint,
  });

  const selectedGroup: FlowDiscoveryGroup | null =
    result?.groups[selectedGroupIdx] ?? null;

  /** Group indices whose group_key identity-matches a saved fingerprint. */
  const fingerprintGroupIdxs = useMemo(() => {
    const set = new Set<number>();
    if (!result || fingerprints.length === 0) return set;
    result.groups.forEach((g, i) => {
      if (findFingerprintForGroupKey(g.group_key, fingerprints, bucketWidthSol)) set.add(i);
    });
    return set;
  }, [result, fingerprints, bucketWidthSol]);
  const flowSplit = useMemo(() => {
    if (!selectedGroup) return null;
    const draftKeys = new Set(draftPatterns.map((p) => JSON.stringify(p)));
    let volumeGross = 0;
    let totalGross = 0;
    for (const s of selectedGroup.structures) {
      totalGross += s.gross_sol;
      if (draftKeys.has(JSON.stringify(s.ix_labels))) volumeGross += s.gross_sol;
    }
    const organicGross = Math.max(0, totalGross - volumeGross);
    const volumePct = totalGross > 0 ? (volumeGross / totalGross) * 100 : 0;
    return { volumeGross, organicGross, totalGross, volumePct };
  }, [selectedGroup, draftPatterns]);
  /** % of each UNCHECKED row's gross SOL that comes from wallets already tagged
   *  by a CHECKED row — previews live's wallet-contagion classifier (flow_split.rs
   *  FlowState::classify), which sweeps a tagged wallet's later trades into
   *  "volume" on ANY structure, not just the one that matched. Null = checked
   *  already, or nothing checked yet to compare against. */
  const contagionByStructure = useMemo(() => {
    const map = new Map<string, number | null>();
    if (!selectedGroup) return map;
    const draftKeys = new Set(draftPatterns.map((p) => JSON.stringify(p)));
    const checkedWalletGross = new Map<string, number>();
    for (const s of selectedGroup.structures) {
      if (!draftKeys.has(JSON.stringify(s.ix_labels))) continue;
      for (const w of s.wallets) {
        checkedWalletGross.set(
          w.wallet_hash,
          (checkedWalletGross.get(w.wallet_hash) ?? 0) + w.gross_sol,
        );
      }
    }
    for (const s of selectedGroup.structures) {
      const key = JSON.stringify(s.ix_labels);
      if (draftKeys.has(key) || checkedWalletGross.size === 0 || s.gross_sol <= 0) {
        map.set(key, null);
        continue;
      }
      let overlap = 0;
      for (const w of s.wallets) {
        if (checkedWalletGross.has(w.wallet_hash)) overlap += w.gross_sol;
      }
      map.set(key, (overlap / s.gross_sol) * 100);
    }
    return map;
  }, [selectedGroup, draftPatterns]);
  /** Per-structure auto-suggest verdict (client-side composite of the
   *  bot-likelihood columns — see suggestStructure). Recomputes with contagion,
   *  so checking a both-side wash row can light up the one-sided rows its
   *  wallets also trade. */
  const suggestionByStructure = useMemo(() => {
    const map = new Map<string, StructureSuggestion>();
    if (!selectedGroup) return map;
    for (const s of selectedGroup.structures) {
      const key = JSON.stringify(s.ix_labels);
      map.set(key, suggestStructure(s, sideOf(s), contagionByStructure.get(key) ?? null));
    }
    return map;
  }, [selectedGroup, contagionByStructure]);
  /** Suggested rows not yet in the draft — drives the auto-select button. */
  const suggestedUnchecked = useMemo(() => {
    if (!selectedGroup) return [] as string[][];
    const draftKeys = new Set(draftPatterns.map((p) => JSON.stringify(p)));
    return selectedGroup.structures
      .filter((s) => {
        const key = JSON.stringify(s.ix_labels);
        return suggestionByStructure.get(key)?.suggested && !draftKeys.has(key);
      })
      .map((s) => s.ix_labels);
  }, [selectedGroup, suggestionByStructure, draftPatterns]);

  function autoSelectSuggested() {
    if (suggestedUnchecked.length === 0) return;
    setDraftPatterns((prev) => [...prev, ...suggestedUnchecked]);
    setApplyOk(null);
  }
  const autoMatchedFp = selectedGroup
    ? findFingerprintForGroupKey(selectedGroup.group_key, fingerprints, bucketWidthSol)
    : null;
  const targetFp: Fingerprint | null =
    (targetFpId && fingerprints.find((f) => f.id === targetFpId)) || null;
  const currentPatterns = volumeIxPatternsFromConfig(targetFp?.metric_config ?? {});

  // Prefer the config-section seed fingerprint; else identity-axis auto-match.
  useEffect(() => {
    const preferred = seedFingerprintId ?? autoMatchedFp?.id ?? null;
    setTargetFpId(preferred);
    const fp = preferred ? fingerprints.find((f) => f.id === preferred) : null;
    setDraftPatterns(fp ? volumeIxPatternsFromConfig(fp.metric_config) : []);
    setApplyOk(null);
    setSelectedTokenMint(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- group/run/seed are the SSOTs
  }, [selectedGroupIdx, result?.run_id, seedFingerprintId]);

  function selectSeedFingerprint(id: string) {
    if (!id) {
      setField('seedFingerprintId', null);
      return;
    }
    const fp = fingerprints.find((f) => f.id === id);
    if (!fp) return;
    setConfig((prev) => ({ ...DEFAULTS, ...prev, ...configFromFingerprint(fp) }));
    setTargetFpId(fp.id);
    setDraftPatterns(volumeIxPatternsFromConfig(fp.metric_config));
    setApplyOk(null);
    setApplyError(null);
  }

  // Deep-link seed: arriving from Fingerprints with `?fp=<id>` scopes discovery
  // to that fingerprint. Applied once per param value (once fingerprints load),
  // so it seeds the config without fighting later manual seed changes.
  const [searchParams] = useSearchParams();
  const appliedFpParam = useRef<string | null>(null);
  useEffect(() => {
    const fp = searchParams.get(STRATEGY_PARAMS.fingerprint);
    if (!fp || appliedFpParam.current === fp) return;
    if (!fingerprints.some((f) => f.id === fp)) return; // wait for the list to load
    appliedFpParam.current = fp;
    if (fp !== seedFingerprintId) selectSeedFingerprint(fp);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- param + loaded list are the SSOTs
  }, [searchParams, fingerprints]);

  async function handleRun() {
    if (running || ixFilterError) return;
    setApplyError(null);
    setApplyOk(null);

    markStarting('discovery', 'discovery', 'Flow discovery');
    try {
      const body: FlowDiscoveryStartArgs = {
        created_after: toUtc(createdAfter),
        created_before: toUtc(createdBefore),
        curve_only: curveOnly,
        group_by: groupBy,
        bucket_width_sol: bucketWidthSol,
        min_tokens: minTokens,
        token_cap: tokenCap,
      };
      if (seedFingerprintId) {
        body.fingerprint_id = seedFingerprintId;
      } else {
        const fieldFilters: Record<string, (number | boolean)[]> = {};
        for (const f of GROUP_FIELDS) {
          if (f === 'ix_labels' || f === 'is_cashback_enabled') continue;
          const nums = parseNumbers(fieldFiltersText[f] ?? '');
          if (nums.length > 0) fieldFilters[f] = nums;
        }
        if (cashbackFilter !== 'all') {
          fieldFilters.is_cashback_enabled = [cashbackFilter === 'true'];
        }
        body.ix_labels_filter =
          !ixLabelsGrouped && ixFilter.labels ? ixFilter.labels : undefined;
        body.field_filters =
          Object.keys(fieldFilters).length > 0 ? fieldFilters : undefined;
      }

      const { run_id } = await startDiscovery(body).unwrap();

      // Poll briefly until the finished SSE lands the result (or timeout).
      for (let i = 0; i < 120; i++) {
        await new Promise((r) => setTimeout(r, 500));
        try {
          const res = await fetchResult(run_id).unwrap();
          setResult(res);
          setSelectedGroupIdx(0);
          markFinished('discovery', 'discovery');
          return;
        } catch {
          /* still running */
        }
      }
      markFinished('discovery', 'discovery');
      setApplyError('Timed out waiting for discovery result — check the jobs indicator.');
    } catch (e) {
      markFinished('discovery', 'discovery');
      setApplyError(apiErrorMessage(e as never, 'Failed to start discovery'));
    }
  }

  function toggleStructure(labels: string[]) {
    const key = JSON.stringify(labels);
    setDraftPatterns((prev) => {
      const has = prev.some((p) => JSON.stringify(p) === key);
      if (has) return prev.filter((p) => JSON.stringify(p) !== key);
      return [...prev, labels];
    });
    setApplyOk(null);
  }

  function selectTargetFingerprint(id: string) {
    const nextId = id || null;
    setTargetFpId(nextId);
    setApplyOk(null);
    if (nextId) {
      const fp = fingerprints.find((f) => f.id === nextId);
      if (fp) setDraftPatterns(volumeIxPatternsFromConfig(fp.metric_config));
    }
  }

  async function handleApply() {
    if (!selectedGroup || draftPatterns.length === 0) return;
    setApplyError(null);
    setApplyOk(null);
    const patterns = draftPatterns.map((p) => p.map((s) => s.trim()).filter(Boolean)).filter((p) => p.length > 0);
    try {
      if (targetFp) {
        await updateFp({
          id: targetFp.id,
          body: {
            name: targetFp.name,
            cu_limit: targetFp.cu_limit,
            cu_price: targetFp.cu_price,
            init_buy_lamports: targetFp.init_buy_lamports,
            max_cost_lamports: targetFp.max_cost_lamports,
            spendable_lamports_in: targetFp.spendable_lamports_in,
            first_slot_buy_lamports: targetFp.first_slot_buy_lamports,
            first_slot_sell_lamports: targetFp.first_slot_sell_lamports,
            bucket_size_amount: targetFp.bucket_size_amount,
            ix_labels: targetFp.ix_labels,
            metric_config: metricConfigWithVolumePatterns(patterns),
          },
        }).unwrap();
        setApplyOk(`Updated fingerprint “${targetFp.name}”.`);
      } else {
        const fp = await bindFp({
          group_key: selectedGroup.group_key,
          bucket_width_sol: bucketWidthSol,
          volume_ix_patterns: patterns,
          name: `flow · ${groupKeyName(selectedGroup.group_key)}`,
        }).unwrap();
        setTargetFpId(fp.id);
        setApplyOk(`Bound fingerprint “${fp.name}”.`);
      }
    } catch (e) {
      setApplyError(apiErrorMessage(e as never, 'Failed to apply patterns'));
    }
  }

  const applying = bindState.isLoading || updateState.isLoading;

  return (
    <div className="pt-2">
      <div className="mb-3 flex flex-wrap items-baseline gap-3">
        <h1 className="text-xl font-extrabold text-text">Flow discovery</h1>
        <span className="text-sm text-text-mid">
          Rank ix structures per fingerprint group → toggle volume patterns
        </span>
      </div>

      <div className="mb-4 flex flex-wrap items-end gap-3 bg-surface">
        <div className="flex flex-col gap-1">
          <LabelTip
            tip={DISCOVERY_FIELD_HELP.createdRange}
            className="text-[9px] font-bold uppercase tracking-wider text-text-dim/80"
          >
            Created range · UTC
          </LabelTip>
          <div className="flex items-center gap-1">
            <Input
              type="datetime-local"
              value={createdAfter}
              onChange={(e) => setField('createdAfter', e.target.value)}
            />
            <span className="text-[10px] text-text-dim/50">–</span>
            <Input
              type="datetime-local"
              value={createdBefore}
              onChange={(e) => setField('createdBefore', e.target.value)}
            />
          </div>
        </div>
        <div className="flex flex-col gap-1 w-[120px]">
          <LabelTip
            tip={SWEEP_FIELD_HELP.minTokens}
            className="text-[9px] font-bold uppercase tracking-wider text-text-dim/80"
          >
            Min tokens
          </LabelTip>
          <Input
            type="number"
            min={1}
            value={minTokens}
            onChange={(e) => setField('minTokens', Math.max(1, Number(e.target.value) || 1))}
          />
        </div>
        <div className="flex flex-col gap-1 w-[120px]">
          <LabelTip
            tip={SWEEP_FIELD_HELP.tokenCap}
            className="text-[9px] font-bold uppercase tracking-wider text-text-dim/80"
          >
            Token cap
          </LabelTip>
          <Input
            type="number"
            min={1}
            max={100000}
            value={tokenCap}
            onChange={(e) =>
              setField('tokenCap', Math.min(100000, Math.max(1, Number(e.target.value) || 1)))
            }
          />
        </div>
        <label className="flex h-[34px] items-center gap-1.5 text-sm text-text-mid">
          <Checkbox
            checked={curveOnly}
            onChange={(e) => setField('curveOnly', e.target.checked)}
          />
          <LabelTip tip={SWEEP_FIELD_HELP.curveOnly}>curve only</LabelTip>
        </label>
        <IconButton
          variant="primary"
          size="lg"
          onClick={handleRun}
          disabled={running || !!ixFilterError || startState.isLoading}
          label={running ? 'Discovering…' : 'Run discovery'}
          title={running ? 'Discovering…' : 'Run discovery'}
        >
          {running ? <SpinnerIcon /> : <PlayIcon />}
        </IconButton>
      </div>

      <div className="mb-4 border-t border-white/10 pt-3">
        <Accordion title="Group by fingerprint" defaultOpen>
          <div className="mb-3 flex flex-col gap-2 rounded border border-white/8 p-3">
            <label className="flex min-w-[16rem] flex-col gap-1 text-[11px] text-text-dim">
              <LabelTip
                tip={DISCOVERY_FIELD_HELP.seedFingerprint}
                className="text-[9px] font-bold uppercase tracking-wider text-text-dim/80"
              >
                Scope by saved fingerprint
              </LabelTip>
              <Select
                fieldSize="sm"
                value={seedFingerprintId ?? ''}
                onChange={(e) => selectSeedFingerprint(e.target.value)}
              >
                <option value="">Manual group-by / filters below</option>
                {fingerprints.map((f) => (
                  <option key={f.id} value={f.id}>
                    {f.name || f.id.slice(0, 8)}
                    {f.used_by != null ? ` · used by ${f.used_by}` : ''}
                  </option>
                ))}
              </Select>
            </label>
            {seedFp ? (
              <div className="flex flex-col gap-1.5">
                <div className="flex flex-wrap items-center gap-2">
                  <Link
                    to={fingerprintsHref(seedFp.id)}
                    className="inline-flex items-center gap-1 rounded-md hover:opacity-90"
                    title={`Open fingerprint “${seedFp.name}”`}
                  >
                    <Badge variant="info">engine match · {seedFp.name}</Badge>
                    <LinkIcon className="h-3.5 w-3.5 text-accent" />
                  </Link>
                  <span className="text-[10px] text-text-dim">
                    Discovery scores only tokens that match this fingerprint, then
                    Apply writes volume_ix_patterns back to it.
                  </span>
                </div>
                {fingerprintParamsCell(seedFp)}
              </div>
            ) : (
              <p className="text-[10px] text-text-dim">
                Pick a fingerprint to detect its volume_ix_patterns — or leave empty and
                use the manual group-by / filters.
              </p>
            )}
          </div>
          <FingerprintGroupPicker
            groupBy={groupBy}
            onToggleField={(f) =>
              setField(
                'groupBy',
                groupBy.includes(f) ? groupBy.filter((x) => x !== f) : [...groupBy, f],
              )
            }
            fieldFiltersText={fieldFiltersText}
            onSetFieldFilter={(field, value) =>
              setConfig((prev) => {
                const base = { ...DEFAULTS, ...prev };
                return {
                  ...base,
                  fieldFiltersText: { ...base.fieldFiltersText, [field]: value },
                };
              })
            }
            cashbackFilter={cashbackFilter}
            onSetCashback={(v) => setField('cashbackFilter', v)}
            bucketWidthSol={bucketWidthSol}
            onSetBucketWidth={(n) =>
              setField('bucketWidthSol', n <= 0 ? SOL_BUCKET_WIDTH : n)
            }
            ixLabelsText={ixLabelsFilter}
            onSetIxLabels={(v) => setField('ixLabelsFilter', v)}
            ixFilter={ixFilter}
            emptyHint={
              seedFingerprintId
                ? 'Scoped to the saved fingerprint → one "ALL" group of matching tokens.'
                : 'No fields selected → one "ALL" group (noisy lift).'
            }
          />
        </Accordion>
      </div>

      {(applyError || ixFilterError || startState.error) && (
        <InlineAlert variant="error">
          {applyError || ixFilterError || apiErrorMessage(startState.error, 'Error')}
        </InlineAlert>
      )}
      {applyOk && <InlineAlert variant="success">{applyOk}</InlineAlert>}

      {result && (
        <div className="flex flex-col gap-3 lg:flex-row">
          <div className="lg:w-72 shrink-0 flex flex-col gap-1">
            <h2 className="text-xs font-semibold uppercase tracking-wide text-text-dim mb-1">
              Groups ({result.groups.length})
            </h2>
            {result.groups.length === 0 ? (
              <p className="text-xs text-text-dim">No groups survived min_tokens.</p>
            ) : (
              <div className="flex max-h-[65vh] flex-col gap-1 overflow-y-auto pr-1">
                {result.groups.map((g, i) => {
                  const isFingerprint = fingerprintGroupIdxs.has(i);
                  return (
                    <button
                      key={i}
                      type="button"
                      onClick={() => setSelectedGroupIdx(i)}
                      className={`rounded border px-2 py-1.5 text-left text-xs transition ${i === selectedGroupIdx
                          ? 'border-accent/50 bg-accent/10 text-text'
                          : isFingerprint
                            ? 'border-l-2 border-l-accent/70 border-y-white/8 border-r-white/8 bg-accent/5 text-text-mid hover:border-white/20'
                            : 'border-white/8 text-text-mid hover:border-white/20'
                        }`}
                    >
                      <div className="flex items-center gap-1.5">
                        <span className="truncate font-mono">{groupKeyLabel(g.group_key)}</span>
                        {isFingerprint && <Badge variant="info">fp</Badge>}
                        {g.ambiguity && <Badge variant="warning">ambig</Badge>}
                      </div>
                      <div className="mt-0.5 text-[10px] text-text-dim">
                        {g.n_tokens} tokens · {g.n_trades_scored.toLocaleString()} trades
                      </div>
                    </button>
                  );
                })}
              </div>
            )}
          </div>

          {selectedGroup && (
            <div className="min-w-0 flex-1 flex flex-col gap-3">
              <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5 rounded border-l-2 border-accent/60 bg-white/2 py-1.5 pl-2.5 pr-2">
                {groupKeyChips(selectedGroup.group_key)}
                <span className="text-[11px] text-text-dim">
                  {selectedGroup.n_tokens.toLocaleString()} tokens ·{' '}
                  {selectedGroup.n_trades_scored.toLocaleString()} trades
                </span>
                {selectedGroup.ambiguity && (
                  <Badge variant="warning">top structure lift ≈ 1 — split may be noisy</Badge>
                )}
              </div>

              <div className="flex flex-col gap-3">
                <div className="grid grid-cols-1 items-start gap-3 sm:grid-cols-2">
                  <div className="flex flex-col gap-2 rounded border border-white/8 p-3">
                    <div className="flex flex-wrap items-end gap-3">
                      <label className="flex min-w-0 flex-1 flex-col gap-1 text-[11px] text-text-dim">
                        <LabelTip
                          tip={DISCOVERY_FIELD_HELP.applyFingerprint}
                          className="text-[9px] font-bold uppercase tracking-wider text-text-dim/80"
                        >
                          Apply to fingerprint
                        </LabelTip>
                      <Select
                        fieldSize="sm"
                        value={targetFpId ?? ''}
                        onChange={(e) => selectTargetFingerprint(e.target.value)}
                      >
                        <option value="">Create / bind from group key</option>
                        {fingerprints.map((f) => (
                          <option key={f.id} value={f.id}>
                            {f.name || f.id.slice(0, 8)}
                            {f.used_by != null ? ` · used by ${f.used_by}` : ''}
                            {autoMatchedFp?.id === f.id ? ' · auto-match' : ''}
                          </option>
                        ))}
                      </Select>
                    </label>
                    {autoMatchedFp && targetFpId !== autoMatchedFp.id && (
                      <IconButton
                        variant="ghost"
                        size="md"
                        type="button"
                        onClick={() => selectTargetFingerprint(autoMatchedFp.id)}
                        title="Use auto-match"
                        aria-label="Use auto-match"
                      >
                        <ReuseIcon />
                      </IconButton>
                    )}
                  </div>
                  {targetFp ? (
                    <div className="flex flex-wrap items-center gap-2">
                      <Link
                        to={fingerprintsHref(targetFp.id)}
                        className="inline-flex items-center gap-1 rounded-md hover:opacity-90"
                        title={`Open fingerprint “${targetFp.name}”`}
                      >
                        <Badge variant="info">update · {targetFp.name}</Badge>
                        <LinkIcon className="h-3.5 w-3.5 text-accent" />
                      </Link>
                      {fingerprintParamsCell(targetFp)}
                    </div>
                  ) : (
                    <Badge variant="neutral">will create / bind fingerprint from this group</Badge>
                  )}
                </div>

                {flowSplit && flowSplit.totalGross > 0 && (
                  <div className="rounded border border-white/8 p-3">
                    <div className="mb-1.5 flex flex-wrap items-center justify-between gap-2">
                      <LabelTip
                        tip={DISCOVERY_FIELD_HELP.volumeSplit}
                        className="text-[9px] font-bold uppercase tracking-wider text-text-dim/80"
                      >
                        Flow split · checked structures
                      </LabelTip>
                      <span className="font-mono text-[11px] text-text-dim">
                        {fmt(flowSplit.volumePct)}% volume of {fmt(flowSplit.totalGross)}◎ scored
                      </span>
                    </div>
                    <div className="flex h-2 w-full overflow-hidden rounded-full bg-white/6">
                      {flowSplit.volumeGross > 0 && (
                        <div
                          className="h-full rounded-full bg-warning"
                          style={{
                            width: `${flowSplit.volumePct}%`,
                            marginRight: flowSplit.organicGross > 0 ? 2 : 0,
                          }}
                        />
                      )}
                      {flowSplit.organicGross > 0 && (
                        <div
                          className="h-full rounded-full bg-white/20"
                          style={{ width: `${100 - flowSplit.volumePct}%` }}
                        />
                      )}
                    </div>
                    <div className="mt-1.5 flex flex-wrap items-center gap-3 text-[10px] text-text-dim">
                      <span className="inline-flex items-center gap-1">
                        <span className="size-2 rounded-full bg-warning" /> Volume (checked):{' '}
                        {fmt(flowSplit.volumeGross)}◎
                      </span>
                      <span className="inline-flex items-center gap-1">
                        <span className="size-2 rounded-full bg-white/30" /> Organic (unchecked):{' '}
                        {fmt(flowSplit.organicGross)}◎
                      </span>
                    </div>
                  </div>
                )}
                </div>

                {selectedGroup.tokens.length > 0 && (
                  <TokenPreviewPanel
                    tokens={selectedGroup.tokens}
                    selectedMint={selectedTokenMint}
                    onSelect={setSelectedTokenMint}
                    trades={previewTrades}
                    tradesLoading={previewTradesLoading}
                    creatorWallet={previewDetail?.creator_wallet ?? null}
                    athPriceInSol={previewDetail?.ath_price ?? null}
                    isMigrated={previewDetail?.is_migrated ?? false}
                    patternKeys={patternKeys}
                    onTogglePattern={toggleStructure}
                  />
                )}

              </div>

              <DraftPatternsCart
                draftPatterns={draftPatterns}
                onChange={setDraftPatterns}
                currentPatterns={currentPatterns}
                targetFp={targetFp}
                applying={applying}
                onApply={handleApply}
              />

              {selectedGroup.n_trades_scored === 0 && (
                <InlineAlert variant="warning">
                  {selectedGroup.n_tokens} tokens matched, but 0 trades had ix_labels — discovery
                  needs lake trade columns written by a post–volume-flow export. Delete the sealed
                  day folders under lake-data/trades (or their _meta.json) and re-run{' '}
                  <code className="font-mono">cargo run -p hunter-lab -- lake-export</code>, then
                  Run discovery again. (Suggested structures appear in the table below once
                  trades score.)
                </InlineAlert>
              )}

              <div className="flex flex-col gap-2">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <span className="inline-flex flex-wrap items-center gap-2 text-xs font-semibold text-text-mid">
                    <LabelTip tip={DISCOVERY_COL_HELP.suggested}>Ranked ix structures</LabelTip>
                    {suggestedUnchecked.length > 0 && (
                      <Badge variant="warning" size="sm">
                        {suggestedUnchecked.length} suggested
                      </Badge>
                    )}
                  </span>
                  <button
                    type="button"
                    disabled={suggestedUnchecked.length === 0}
                    onClick={autoSelectSuggested}
                    className="inline-flex items-center gap-1 rounded border border-warning/40 px-2 py-1 text-[11px] font-semibold text-warning transition hover:bg-warning/10 disabled:cursor-not-allowed disabled:opacity-40"
                    title="Add every auto-suggested structure to the draft volume_ix_patterns"
                  >
                    <CheckIcon className="h-3.5 w-3.5" />
                    Auto-select suggested
                  </button>
                </div>
                <StructureTable
                  structures={selectedGroup.structures}
                  draftPatterns={draftPatterns}
                  contagionByStructure={contagionByStructure}
                  suggestionByStructure={suggestionByStructure}
                  onToggle={toggleStructure}
                />
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

/** Staging "cart" for the volume_ix_patterns being assembled: an accent-elevated
 *  panel that reads as the page's deliverable, not just another box. Checked rows
 *  from the ranked table land here as chips; the primary Apply CTA writes them back
 *  to the fingerprint. Raw JSON editing is one toggle away. */
function DraftPatternsCart({
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
        <div className="rounded border border-dashed border-white/12 px-3 py-4 text-center">
          <p className="text-[11px] text-text-dim">
            No structures staged. Check rows in the ranked table below —{' '}
            <button
              type="button"
              className="font-semibold text-accent hover:underline"
              onClick={() => {
                onChange([...draftPatterns, []]);
                setRawEdit(true);
              }}
            >
              or add one manually
            </button>
            .
          </p>
          <p className="mt-1 text-[10px] text-text-dim/70">
            Flow metrics stay NaN until at least one structure is staged.
          </p>
        </div>
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

/** Ranking table itself, split out so its `columns` memo only recomputes on
 *  selection/checked-pattern changes, not on every `FlowDiscoveryPage` render. */
function StructureTable({
  structures,
  draftPatterns,
  contagionByStructure,
  suggestionByStructure,
  onToggle,
}: {
  structures: FlowDiscoveryStructure[];
  draftPatterns: string[][];
  contagionByStructure: Map<string, number | null>;
  suggestionByStructure: Map<string, StructureSuggestion>;
  onToggle: (labels: string[]) => void;
}) {
  const draftKeysSig = draftPatterns.map((p) => JSON.stringify(p)).join('|');
  const columns = useMemo(
    () =>
      buildStructureColumns({
        draftPatterns,
        contagionByStructure,
        suggestionByStructure,
        onToggle,
      }),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- draftKeysSig/contagion/suggestion maps are the real identities
    [draftKeysSig, contagionByStructure, suggestionByStructure, onToggle],
  );
  return (
    <DataTable
      columns={columns}
      rows={structures}
      rowKey={(s) => JSON.stringify(s.ix_labels)}
      rowClassName={(s) =>
        draftPatterns.some((p) => JSON.stringify(p) === JSON.stringify(s.ix_labels))
          ? 'bg-accent/8'
          : undefined
      }
      searchable={false}
      colFilters={false}
      colToggle={false}
      selectable={false}
      paginate={false}
    />
  );
}

/** Ranked member-token picker (cheap gross_sol roster, no trade payload) +
 *  the per-token candlestick preview for whichever one is picked. Only the
 *  picked token's full trade history is ever fetched — the roster itself
 *  can't show a real vol/organic split (that needs the trade-level
 *  classifier), which is exactly what picking a token and reading the chart
 *  below gives you instead. */
function TokenPreviewPanel({
  tokens,
  selectedMint,
  onSelect,
  trades,
  tradesLoading,
  creatorWallet,
  athPriceInSol,
  isMigrated,
  patternKeys,
  onTogglePattern,
}: {
  tokens: FlowDiscoveryTokenGross[];
  selectedMint: string | null;
  onSelect: (mint: string) => void;
  trades: TradeRecord[];
  tradesLoading: boolean;
  creatorWallet: string | null;
  athPriceInSol: number | null;
  isMigrated: boolean;
  patternKeys: ReadonlySet<string>;
  onTogglePattern: (labels: string[]) => void;
}) {
  return (
    <div className="rounded border border-white/8 p-3">
      <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
        <span className="text-xs font-semibold text-text-mid">
          Token preview · {tokens.length.toLocaleString()} ranked
        </span>
        {selectedMint && tradesLoading && (
          <span className="text-[10px] text-text-dim">Loading trades…</span>
        )}
      </div>
      <div className="flex flex-wrap gap-3">
        <div className="flex max-h-70 w-56 shrink-0 flex-col gap-1 overflow-y-auto pr-1">
          {tokens.map((t) => (
            <div
              key={t.mint_address}
              role="button"
              tabIndex={0}
              onClick={() => onSelect(t.mint_address)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault();
                  onSelect(t.mint_address);
                }
              }}
              className={`cursor-pointer rounded border px-2 py-1 text-left text-[10px] transition ${
                t.mint_address === selectedMint
                  ? 'border-accent/50 bg-accent/10 text-text'
                  : 'border-white/8 text-text-mid hover:border-white/20'
              }`}
            >
              <AddressDisplay address={t.mint_address} truncate={false} kind="token" iconSize='lg' stopPropagation />
              <div className="text-text-dim">
                {fmt(t.gross_sol)}◎ · {t.n_trades.toLocaleString()} trades
              </div>
            </div>
          ))}
        </div>
        <div className="min-w-0 flex-1">
          {selectedMint ? (
            <FlowPreviewChart
              trades={trades}
              patternKeys={patternKeys}
              onTogglePattern={onTogglePattern}
              creatorWallet={creatorWallet}
              athPriceInSol={athPriceInSol}
              isMigrated={isMigrated}
            />
          ) : (
            <p className="text-[11px] text-text-dim">
              Pick a token to preview its vol/non-vol split — updates live as you toggle
              structures below.
            </p>
          )}
        </div>
      </div>
    </div>
  );
}
