import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Link, useSearchParams } from 'react-router-dom';

import { Accordion } from 'components/ui/Accordion';
import { Badge } from 'components/ui/Badge';
import { IconButton } from 'components/ui/IconButton';
import {
  CheckIcon,
  LinkIcon,
  PlayIcon,
  ReuseIcon,
  SpinnerIcon,
} from 'components/ui/icons';
import { Checkbox } from 'components/ui/Checkbox';
import { DateTimeRangePicker } from 'components/ui/DateTimeRangePicker';
import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { InlineAlert } from 'components/ui/Modal';
import { PageHeader } from 'components/ui/PageHeader';
import {
  fingerprintParamsCell,
  chip as paramChip,
  axisTint,
  IxLabelsChip,
} from 'components/strategy/FingerprintParamsSummary';
import { LabelTip } from 'components/strategy/LabelTip';
import { FingerprintScopeControl } from 'components/strategy/FingerprintScopeControl';
import { useFingerprintMatches } from '@lab/components/strategy/useFingerprintMatches';
import { IxLabelsDisplay } from 'components/ui/IxLabelsDisplay';
import { useLocalStorage } from 'hooks/useLocalStorage';
import { useAccordionOpen } from 'hooks/useUiPrefs';
import { ACCORDION_IDS, STORAGE_KEYS } from 'lib/storage';
import { apiErrorMessage } from 'store/baseApi';
import {
  useGetFingerprintsQuery,
  useGetTokenDetailQuery,
  useGetTokenTradesQuery,
  useUpdateFingerprintMutation,
} from 'store/sharedEndpoints';
import { fingerprintsHref, STRATEGY_PARAMS } from 'lib/strategy/nav';
import {
  flowWalletRules,
  metricConfigWithList,
  patternRowsForList,
  type IxPatternList,
  withFlowWalletRules,
} from 'lib/strategy/registry';
import {
  DISCOVERY_COL_HELP,
  DISCOVERY_FIELD_HELP,
  SWEEP_FIELD_HELP,
} from 'lib/strategy/strategyHelp';
import { configuredAxes, formatPredicate } from 'lib/strategy/fingerprintAxes';
import { formatIxLabelsText } from 'lib/ixLabels';
import { DraftPatternsCart } from '@lab/components/flow/DraftPatternsCart';
import {
  isFirstSlotPresent,
  suggestStructure,
  type StructureSuggestion,
} from '@lab/components/flow/flowDiscoverySuggest';
import { StructureTable } from '@lab/components/flow/StructureTable';
import { TokenPreviewPanel } from '@lab/components/flow/TokenPreviewPanel';
import { patternKeysFrom } from 'lib/flow/classifyFlow';
import {
  addUnpinnedPatterns,
  patternKey,
  patternRowKey,
  removeUnpinnedPatterns,
  rowFromTrade,
  rowPinsFee,
  serializeIxPatternRows,
  togglePatternRow,
  type IxPatternFeeMask,
  type IxPatternRow,
} from 'lib/strategy/ixPatternRows';
import { FingerprintGroupPicker } from '@lab/components/sweep/FingerprintGroupPicker';
import { parseIxLabelsFilter, buildFieldFilters } from '@lab/components/sweep/fingerprintFilters';
import type { Fingerprint } from 'lib/strategy/types';
import {
  GROUP_FIELD_LABELS,
  LAMPORTS_GROUP_FIELDS,
  GROUP_FIELDS,
  type GroupField,
  type PartitionSpec,
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
} from 'types';
import {
  findFingerprintForGroupKey,
  groupValueLabels,
  indexFingerprintsByIdentity,
  renderGroupKey,
  withIxLabelsFilter,
} from 'lib/strategy/matchGroupFingerprint';
import { fingerprintNameFromGroupKey } from 'lib/strategy/fingerprintNameFromGroupKey';

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
  /** How each grouped field is partitioned, keyed by field tag. A field not named
   *  here is `{kind:'distinct'}` — one group per value.
   *
   *  Explicit edges, never a width: the windows a run scored over travel WITH the
   *  run, so the promoted rule and the dashboard read the same ones instead of
   *  three surfaces re-deriving them from one number. */
  partition: Record<string, PartitionSpec>;
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
  partition: {},
  seedFingerprintId: null,
};


/** Fill group-by filters from a saved fingerprint so the picker mirrors its axes.
 *  Discovery run uses `fingerprint_id` for real engine matching (buckets included). */
function configFromFingerprint(fp: Fingerprint): Partial<DiscoveryConfig> {
  // Each axis's own predicate, rendered in that axis's display unit through the ONE
  // formatter — so the filter box shows exactly the window the fingerprint matches
  // rather than a value re-derived at some substituted precision.
  const fieldFiltersText: Record<string, string> = {};
  let ixLabelsFilter = '';
  for (const [id, pred] of configuredAxes(fp.criteria ?? {})) {
    if (pred.kind === 'sequence') {
      ixLabelsFilter = formatIxLabelsText(pred.labels);
      continue;
    }
    fieldFiltersText[id] = formatPredicate(id, pred);
  }
  return {
    seedFingerprintId: fp.id,
    // One ALL group over tokens that match this fingerprint.
    groupBy: [],
    fieldFiltersText,
    ixLabelsFilter,
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

function groupKeyLabel(gk: Record<string, unknown>): string {
  const parts = renderGroupKey(gk).map(([k, v]) => `${k}=${v}`);
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
  init_buy_lamports: 'init',
  max_cost_lamports: 'max',
  spendable_lamports_in: 'spend',
  first_slot_buy_lamports: 'fs_buy',
  first_slot_sell_lamports: 'fs_sell',
};

/** Selected-group header — reuses `fingerprintParamsCell`'s chip style +
 *  `axisTint` hue table so the group-key header reads consistently with the
 *  fingerprint-param chips shown a few lines below it on this same page,
 *  instead of the flat `key=value · key=value` string used by the sidebar
 *  list. `ix_labels` is excluded from the chip row — a pipe-joined instruction
 *  sequence doesn't compress into a `label=value` chip, so it renders as
 *  pretty-printed JSON via `IxLabelsDisplay` instead (same as the sweep
 *  table's group-key column). It DOES still get an `Nix` chip in the row
 *  though — the shared `IxLabelsChip`, so the count, the hashed ribbon, and
 *  the click-to-copy tooltip read exactly as they do for a fingerprint's own
 *  `ix_labels` axis a few lines below. */
function groupKeyChips(gk: Record<string, unknown>) {
  const entries = renderGroupKey(gk);
  if (entries.length === 0) {
    return <span className="text-sm font-bold text-text">ALL tokens</span>;
  }
  const hasIxValue = Object.prototype.hasOwnProperty.call(gk, 'ix_labels');
  const ixParts = groupValueLabels(gk.ix_labels);
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
          {ixParts && <IxLabelsChip labels={ixParts} />}
        </div>
      )}
      {hasIxValue && (
        <div className="flex items-start gap-1.5">
          <span className="pt-0.5 text-[9px] font-bold uppercase tracking-wider text-text-dim/80">
            {GROUP_FIELD_LABELS.ix_labels}:
          </span>
          <IxLabelsDisplay labels={ixParts ?? []} copyJson empty="∅" />
        </div>
      )}
    </div>
  );
}


/**
 * Lab page: score trade ix-structures per fingerprint group, toggle volume
 * patterns, apply via fingerprint update or promote-style bind.
 */
export function FlowDiscoveryPage() {
  const [stored, setConfig] = useLocalStorage<DiscoveryConfig>(
    STORAGE_KEYS.flowDiscoveryConfig,
    DEFAULTS,
    { debounceMs: 400 },
  );
  // Collapsing the group list hands its column back to the selected group's detail.
  const [groupsOpen, setGroupsOpen] = useAccordionOpen(ACCORDION_IDS.flowDiscoveryGroups, true);
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
    partition,
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
  // Per-field value filters, parsed once — same contract as the ix filter above:
  // display-only under a fingerprint scope, otherwise a parse error blocks Run so
  // a dropped SOL filter can't silently widen the discovery corpus.
  const fieldFilterParse = useMemo(
    () =>
      buildFieldFilters(fieldFiltersText, {
        fields: GROUP_FIELDS,
        bucketed: LAMPORTS_GROUP_FIELDS,
        cashback: cashbackFilter,
        labels: GROUP_FIELD_LABELS,
      }),
    [fieldFiltersText, cashbackFilter],
  );
  const fieldFilterError = seedFingerprintId ? null : fieldFilterParse.error;

  const { markStarting, markFinished } = useBackgroundJobActions();
  const { isRunning } = useBackgroundJobsState();
  const running = isRunning('discovery', 'discovery');

  const [startDiscovery, startState] = useStartFlowDiscoveryMutation();
  const [fetchResult] = useLazyGetFlowDiscoveryQuery();
  const [bindFp, bindState] = useBindFlowDiscoveryMutation();
  const [updateFp, updateState] = useUpdateFingerprintMutation();
  const { data: fingerprints = [], isLoading: fingerprintsLoading } =
    useGetFingerprintsQuery();
  const fingerprintsById = useMemo(() => {
    const map = new Map(fingerprints.map((f) => [f.id, f]));
    return map;
  }, [fingerprints]);
  const fpByIdentity = useMemo(
    () => indexFingerprintsByIdentity(fingerprints),
    [fingerprints],
  );
  const seedFp = seedFingerprintId
    ? fingerprintsById.get(seedFingerprintId)
    : undefined;
  const fpMatches = useFingerprintMatches(seedFingerprintId, seedFp?.name);

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
  const [draftPatterns, setDraftPatterns] = useState<IxPatternRow[]>([]);
  /** Sticky fee-field modifiers for the preview trades table. Ranked-table
   *  checkboxes stay structure-only. */
  const [feePins, setFeePins] = useState<IxPatternFeeMask>({});
  /** `m_flow_ix`'s two wallet rules, seeded from the target fingerprint. Both default
   *  true, matching the backend, so a bind-created fingerprint reads the same here. */
  const [walletRules, setWalletRules] = useState(() => flowWalletRules(null));
  /** Which list Apply writes the draft into. Switching it reseeds the draft from
   *  that list, so the cart always shows the list it is about to overwrite —
   *  applying a tagged draft onto the dump key would be a silent list swap. */
  const [stageInto, setStageInto] = useState<IxPatternList>('tagged');
  const [applyError, setApplyError] = useState<string | null>(null);
  const [applyOk, setApplyOk] = useState<string | null>(null);
  const [selectedTokenMint, setSelectedTokenMint] = useState<string | null>(null);
  const patternKeys = useMemo(
    () => patternKeysFrom(draftPatterns.map((r) => r.labels)),
    [draftPatterns],
  );
  const draftRowKeys = useMemo(
    () => new Set(draftPatterns.map((r) => patternRowKey(r))),
    [draftPatterns],
  );
  /** Catch-all (unpinned) rows — what the ranked table's Vol checkbox stages. */
  const draftUnpinned = useMemo(
    () => draftPatterns.filter((r) => !rowPinsFee(r)).map((r) => r.labels),
    [draftPatterns],
  );
  const unpinnedKeys = useMemo(
    () => new Set(draftUnpinned.map((p) => patternKey(p))),
    [draftUnpinned],
  );
  const { data: previewTrades = [], isFetching: previewTradesLoading } = useGetTokenTradesQuery(
    selectedTokenMint ?? '',
    { skip: !selectedTokenMint },
  );
  const { data: previewDetail } = useGetTokenDetailQuery(selectedTokenMint ?? '', {
    skip: !selectedTokenMint,
  });

  const selectedGroup: FlowDiscoveryGroup | null =
    result?.groups[selectedGroupIdx] ?? null;

  // ── The displayed result's OWN identity ───────────────────────────────────
  // Read off the run, never off the form above it. `result` can be a disk-cached
  // run from an earlier session (see the rehydrate effect), so the live config is
  // not its identity — and the label filter is part of what a group binds to.
  /** The exact-set label filter the RUN applied, or null. */
  const runIxLabels: string[] | null = useMemo(() => {
    if (result && result.ix_labels_filter !== undefined) return result.ix_labels_filter;
    return ixLabelsGrouped ? null : ixFilter.labels;
  }, [result, ixLabelsGrouped, ixFilter.labels]);
  /** The saved fingerprint the RUN was scoped to, if any. */
  const runScopeFp: Fingerprint | null = useMemo(() => {
    const id = result && result.fingerprint_id !== undefined ? result.fingerprint_id : seedFingerprintId;
    return (id && fingerprintsById.get(id)) || null;
  }, [result, seedFingerprintId, fingerprintsById]);

  /** Resolve one group to its saved fingerprint, exactly as promote/bind would.
   *
   *  A scoped run pins the whole corpus to one fingerprint and its groups are
   *  sub-slices of it, so that fingerprint is the authoritative attribution — and
   *  a scoped run's key is usually `{}`, which would otherwise fuzzily match any
   *  unrelated fingerprint that merely shares the precision. Same precedence the
   *  grouped sweep uses. */
  const resolveGroupFp = useCallback(
    (groupKey: Record<string, unknown>): Fingerprint | null =>
      runScopeFp ??
      findFingerprintForGroupKey(withIxLabelsFilter(groupKey, runIxLabels), fingerprints, {
        byIdentity: fpByIdentity,
      }),
    [runScopeFp, runIxLabels, fingerprints, fpByIdentity],
  );

  /** Group indices whose group_key identity-matches a saved fingerprint. */
  const fingerprintGroupIdxs = useMemo(() => {
    const set = new Set<number>();
    if (!result || fingerprints.length === 0) return set;
    result.groups.forEach((g, i) => {
      if (resolveGroupFp(g.group_key)) set.add(i);
    });
    return set;
  }, [result, fingerprints, resolveGroupFp]);
  const flowIx = useMemo(() => {
    if (!selectedGroup) return null;
    let volumeGross = 0;
    let totalGross = 0;
    for (const s of selectedGroup.structures) {
      totalGross += s.gross_sol;
      if (patternKeys.has(JSON.stringify(s.ix_labels))) volumeGross += s.gross_sol;
    }
    const organicGross = Math.max(0, totalGross - volumeGross);
    const volumePct = totalGross > 0 ? (volumeGross / totalGross) * 100 : 0;
    return { volumeGross, organicGross, totalGross, volumePct };
  }, [selectedGroup, patternKeys]);
  /** % of each UNCHECKED row's gross SOL that comes from wallets already tagged
   *  by a CHECKED row — previews live's wallet-contagion classifier (flow_ix.rs
   *  FlowState::classify), which sweeps a tagged wallet's later trades into
   *  "volume" on ANY structure, not just the one that matched. Null = checked
   *  already, or nothing checked yet to compare against. */
  const contagionByStructure = useMemo(() => {
    const map = new Map<string, number | null>();
    if (!selectedGroup) return map;
    const checkedWalletGross = new Map<string, number>();
    for (const s of selectedGroup.structures) {
      if (!patternKeys.has(JSON.stringify(s.ix_labels))) continue;
      for (const w of s.wallets) {
        checkedWalletGross.set(
          w.wallet_hash,
          (checkedWalletGross.get(w.wallet_hash) ?? 0) + w.gross_sol,
        );
      }
    }
    for (const s of selectedGroup.structures) {
      const key = JSON.stringify(s.ix_labels);
      if (patternKeys.has(key) || checkedWalletGross.size === 0 || s.gross_sol <= 0) {
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
  }, [selectedGroup, patternKeys]);
  /** Whether this run has an out-of-group baseline for `group_lift`. A scoped run
   *  (or any run with no group-by) is one group over the whole corpus, so every
   *  lift is exactly 1.0 — the gate must be skipped, not failed. Absent on a
   *  pre-field cached result: those were read as having a real lift. */
  const liftDefined = selectedGroup?.lift_defined ?? true;
  /** Per-structure auto-suggest verdict (client-side composite of the
   *  bot-likelihood columns — see suggestStructure). Deliberately does NOT depend
   *  on `contagionByStructure`: contagion is defined against the current draft, so
   *  feeding it in made a row's verdict change as you clicked and the bulk-select
   *  non-idempotent. It stays a column you read, not an input to the score — which
   *  also keeps this memo off the check/uncheck path. */
  const suggestionByStructure = useMemo(() => {
    const map = new Map<string, StructureSuggestion>();
    if (!selectedGroup) return map;
    for (const s of selectedGroup.structures) {
      map.set(
        JSON.stringify(s.ix_labels),
        suggestStructure(s, { liftDefined, groupTokens: selectedGroup.n_tokens }),
      );
    }
    return map;
  }, [selectedGroup, liftDefined]);
  /** Suggested rows not yet in the draft — drives the auto-select button. */
  const suggestedUnchecked = useMemo(() => {
    if (!selectedGroup) return [] as string[][];
    return selectedGroup.structures
      .filter((s) => {
        const key = JSON.stringify(s.ix_labels);
        return suggestionByStructure.get(key)?.suggested && !unpinnedKeys.has(key);
      })
      .map((s) => s.ix_labels);
  }, [selectedGroup, suggestionByStructure, unpinnedKeys]);

  /** Every row that traded in a matched token's creation slot — a property of THIS
   *  corpus alone, deliberately not differenced against the draft. Independent of
   *  the composite above: a shape present in the launch bundle is bundler tooling
   *  by identity, whether or not it also trips the bot signals (and whether or not
   *  it trades later — see `isFirstSlotPresent`). The creation shape is covered by
   *  the same test.
   *
   *  The launch auto-select gates on THIS, not on the unchecked diff below, because
   *  the two answer different questions and only this one is about the run: the
   *  draft is re-seeded from the target fingerprint's SAVED patterns on every run
   *  (`seedFromFingerprint`), so once a launch set has been applied, a re-run over a
   *  new window re-stages it and the diff is empty — even though the new corpus is
   *  full of launch shapes. Disabling on the diff made "this window has no launch
   *  shapes" and "you already saved these" the same dead button, which also killed
   *  the hover preview (a `disabled` button fires no mouse events), removing the one
   *  affordance that could tell them apart. Clicking with an empty diff is a
   *  deliberate no-op. */
  const firstSlotAll = useMemo(() => {
    if (!selectedGroup) return [] as string[][];
    return selectedGroup.structures.filter(isFirstSlotPresent).map((s) => s.ix_labels);
  }, [selectedGroup]);
  /** Of those, the ones the click would actually add. */
  const firstSlotUnchecked = useMemo(() => {
    return firstSlotAll.filter((labels) => !unpinnedKeys.has(JSON.stringify(labels)));
  }, [firstSlotAll, unpinnedKeys]);
  /** No row in the group carries a first-slot count — the run predates the backend
   *  field, so presence is *unknown* for every shape rather than false. Reported, so
   *  an unscored run can't read as "this window had no launch bundle". */
  const firstSlotUnscored = useMemo(
    () =>
      !!selectedGroup &&
      selectedGroup.structures.length > 0 &&
      selectedGroup.structures.every((s) => s.first_slot_trades == null),
    [selectedGroup],
  );

  // ── the per-token launch set ──────────────────────────────────────────────
  // `firstSlotAll` above answers "which ranked structures appeared in SOME member
  // token's creation slot" — three lossy steps away from "what was in this
  // token's launch bundle": it aggregates over the whole group, it only ever sees
  // the rows that survived the server-side rank + `max_structures_per_group`
  // truncation, and a rare small bundler shape loses that ranking by
  // construction. The backend therefore ships each roster token its OWN
  // creation-slot shape list, uncapped and unfloored, and this button applies
  // exactly that.
  /** Roster row for the token being previewed, or null when none is picked. */
  const selectedTokenRow = useMemo(
    () => selectedGroup?.tokens.find((t) => t.mint_address === selectedTokenMint) ?? null,
    [selectedGroup, selectedTokenMint],
  );
  /** Every shape in the previewed token's creation slot. */
  const tokenLaunchAll = useMemo(
    () => selectedTokenRow?.first_slot_ix_labels ?? [],
    [selectedTokenRow],
  );
  /** Of those, the ones the click would actually add. */
  const tokenLaunchUnchecked = useMemo(() => {
    return tokenLaunchAll.filter((labels) => !unpinnedKeys.has(JSON.stringify(labels)));
  }, [tokenLaunchAll, unpinnedKeys]);
  /** The run predates the per-token field — the list is *unknown*, not empty. */
  const tokenLaunchUnscored = !!selectedTokenRow && selectedTokenRow.first_slot_ix_labels == null;

  // ── the filtered set ──────────────────────────────────────────────────────
  // What the table currently shows, after its search + per-column filters. The
  // three buttons above read the GROUP (and the per-token one reads the token's
  // own bundle); this pair reads the table instead, so any slice you can express
  // in the filter row — side, launch presence, `Lift >3`, still-open — is also a
  // thing you can stage in one click.
  const [emittedFiltered, setEmittedFiltered] = useState<FlowDiscoveryStructure[]>([]);
  const handleFilteredStructures = useCallback((rows: FlowDiscoveryStructure[]) => {
    setEmittedFiltered(rows);
  }, []);
  /** The emitted set narrowed to rows that are actually in the selected group.
   *
   *  Switching groups swaps the table's whole row set, and the emit that carries
   *  the new one arrives via an effect — a child effect, so it lands BEFORE any
   *  parent effect could clear the old value, which makes clearing on the switch
   *  a race that wipes the fresh set. Intersecting instead is order-independent:
   *  a leftover row from the previous group can never be counted, whichever
   *  render this is read on. */
  const filteredStructures = useMemo(() => {
    if (!selectedGroup) return [] as FlowDiscoveryStructure[];
    const groupKeys = new Set(selectedGroup.structures.map((s) => JSON.stringify(s.ix_labels)));
    return emittedFiltered.filter((s) => groupKeys.has(JSON.stringify(s.ix_labels)));
  }, [emittedFiltered, selectedGroup]);

  /** Filtered rows the stage button would add (staged ones are already there). */
  const filteredUnstaged = useMemo(() => {
    return filteredStructures
      .filter((s) => !unpinnedKeys.has(JSON.stringify(s.ix_labels)))
      .map((s) => s.ix_labels);
  }, [filteredStructures, unpinnedKeys]);
  /** Filtered rows the unstage button would remove. */
  const filteredStaged = useMemo(() => {
    return filteredStructures
      .filter((s) => unpinnedKeys.has(JSON.stringify(s.ix_labels)))
      .map((s) => s.ix_labels);
  }, [filteredStructures, unpinnedKeys]);
  /** Every row on screen — what both buttons outline on hover, so the pair marks
   *  the same set whichever one you are about to press. */
  const filteredAll = useMemo(
    () => filteredStructures.map((s) => s.ix_labels),
    [filteredStructures],
  );
  /** The table is filtered down from the group — worth saying, since pagination is
   *  off and there is no pager footer to read a count off. */
  const filteredIsNarrowed =
    !!selectedGroup && filteredStructures.length < selectedGroup.structures.length;

  function autoSelectSuggested() {
    if (suggestedUnchecked.length === 0) return;
    setDraftPatterns((prev) => addUnpinnedPatterns(prev, suggestedUnchecked));
    setApplyOk(null);
  }

  function autoSelectFirstSlot() {
    if (firstSlotUnchecked.length === 0) return;
    setDraftPatterns((prev) => addUnpinnedPatterns(prev, firstSlotUnchecked));
    setApplyOk(null);
  }

  function autoSelectTokenLaunch() {
    if (tokenLaunchUnchecked.length === 0) return;
    setDraftPatterns((prev) => addUnpinnedPatterns(prev, tokenLaunchUnchecked));
    setApplyOk(null);
  }

  function stageFiltered() {
    if (filteredUnstaged.length === 0) return;
    setDraftPatterns((prev) => addUnpinnedPatterns(prev, filteredUnstaged));
    setApplyOk(null);
  }

  function unstageFiltered() {
    if (filteredStaged.length === 0) return;
    setDraftPatterns((prev) => removeUnpinnedPatterns(prev, filteredStaged));
    setApplyOk(null);
  }

  /** Rows a hovered/focused bulk-select acts on — outlined in the table so the
   *  effect is visible before the click, not discovered after it. The launch button
   *  passes its FULL set, not its unchecked diff: when everything is already staged
   *  the outline is the only way to see which rows the button is talking about, and
   *  re-adding a staged row is a no-op anyway. */
  const [previewPatterns, setPreviewPatterns] = useState<string[][] | null>(null);
  const previewKeys = useMemo(
    () => new Set((previewPatterns ?? []).map((p) => JSON.stringify(p))),
    [previewPatterns],
  );
  /** Hover/focus handlers for a bulk-select button. */
  function previewProps(rows: string[][]) {
    const show = () => setPreviewPatterns(rows);
    const hide = () => setPreviewPatterns(null);
    return { onMouseEnter: show, onMouseLeave: hide, onFocus: show, onBlur: hide };
  }
  const autoMatchedFp = selectedGroup ? resolveGroupFp(selectedGroup.group_key) : null;
  const targetFp: Fingerprint | null =
    (targetFpId && fingerprints.find((f) => f.id === targetFpId)) || null;
  const currentPatterns = patternRowsForList(targetFp?.metric_config ?? {}, stageInto);
  const savedWalletRules = flowWalletRules(targetFp?.metric_config);

  /** Point the apply target at a fingerprint and load its SAVED patterns into the
   *  draft — the ONE seeding path, so every trigger (group change, late list load,
   *  manual pick) stages the same thing. `null` ⇒ promote-style bind, empty draft.
   *
   *  The two wallet rules seed here too. Apply PUTs the whole row, so a stale pair
   *  held over from the previously selected fingerprint would be written onto this
   *  one — the classifier changing as a side effect of picking a target. */
  const seedFromFingerprint = useCallback(
    (id: string | null) => {
      setTargetFpId(id);
      const fp = id ? fingerprints.find((f) => f.id === id) : null;
      setDraftPatterns(fp ? patternRowsForList(fp.metric_config, stageInto) : []);
      setWalletRules(flowWalletRules(fp?.metric_config));
      setApplyOk(null);
    },
    [fingerprints, stageInto],
  );

  /** Point staging at the other list and reseed from it. */
  const changeStageInto = useCallback(
    (list: IxPatternList) => {
      setStageInto(list);
      const fp = targetFpId ? fingerprints.find((f) => f.id === targetFpId) : null;
      setDraftPatterns(fp ? patternRowsForList(fp.metric_config, list) : []);
      setApplyOk(null);
    },
    [fingerprints, targetFpId],
  );

  /** The seed ran before the fingerprint list had loaded, so its "no fingerprint"
   *  verdict came from an absent list, not from identity — re-resolve once below. */
  const seedUnresolved = useRef(false);

  // Prefer the config-section seed fingerprint; else identity-axis auto-match.
  useEffect(() => {
    seedUnresolved.current = fingerprintsLoading;
    seedFromFingerprint(seedFingerprintId ?? autoMatchedFp?.id ?? null);
    setSelectedTokenMint(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- group/run/seed are the SSOTs
  }, [selectedGroupIdx, result?.run_id, seedFingerprintId]);

  // The fingerprint list is fetched in parallel with the run, so the seed above
  // routinely runs with an empty list and stages nothing — and since the group
  // list preselects index 0, re-clicking that group is a no-op that never
  // re-seeds, leaving an already-configured group looking unconfigured. Resolve
  // once, the moment the list lands. Deliberately NOT keyed on `fingerprints`:
  // Apply invalidates the tag, and a refetch must never overwrite a live draft.
  useEffect(() => {
    if (!seedUnresolved.current || fingerprintsLoading) return;
    seedUnresolved.current = false;
    const preferred = seedFingerprintId ?? autoMatchedFp?.id ?? null;
    if (preferred) seedFromFingerprint(preferred);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- fires once, when the list resolves
  }, [fingerprintsLoading]);

  function selectSeedFingerprint(id: string) {
    if (!id) {
      setField('seedFingerprintId', null);
      return;
    }
    const fp = fingerprintsById.get(id);
    if (!fp) return;
    setConfig((prev) => ({ ...DEFAULTS, ...prev, ...configFromFingerprint(fp) }));
    seedFromFingerprint(fp.id);
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
    if (running || ixFilterError || fieldFilterError) return;
    setApplyError(null);
    setApplyOk(null);

    markStarting('discovery', 'discovery', 'Flow discovery');
    try {
      const body: FlowDiscoveryStartArgs = {
        created_after: toUtc(createdAfter),
        created_before: toUtc(createdBefore),
        curve_only: curveOnly,
        group_by: groupBy,
        // Exact mode replaces the width outright (backend ignores it there).
        ...(Object.keys(partition).length > 0 ? { partition } : {}),
        min_tokens: minTokens,
        token_cap: tokenCap,
      };
      if (seedFingerprintId) {
        body.fingerprint_id = seedFingerprintId;
      } else {
        const fieldFilters = fieldFilterParse.filters;
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
    setDraftPatterns((prev) => togglePatternRow(prev, { labels: [...labels] }));
    setApplyOk(null);
  }

  function toggleTrade(labels: string[], trade: { cu_limit?: number | null; cu_price?: number | null; tip_lamports?: number | null }) {
    setDraftPatterns((prev) => togglePatternRow(prev, rowFromTrade(labels, trade, feePins)));
    setApplyOk(null);
  }

  function selectTargetFingerprint(id: string) {
    const nextId = id || null;
    // Switching to promote-style bind keeps whatever is staged — only a real
    // fingerprint re-seeds the draft from its saved config.
    if (nextId && fingerprints.some((f) => f.id === nextId)) {
      seedFromFingerprint(nextId);
      return;
    }
    setTargetFpId(nextId);
    setApplyOk(null);
  }

  async function handleApply() {
    if (!selectedGroup || draftPatterns.length === 0) return;
    setApplyError(null);
    setApplyOk(null);
    const patterns = serializeIxPatternRows(draftPatterns);
    if (patterns.length === 0) return;
    try {
      if (targetFp) {
        await updateFp({
          id: targetFp.id,
          body: {
            name: targetFp.name,
            // The whole criteria map round-trips: a PUT replaces the row, so an
            // omitted axis would silently WIDEN what this fingerprint matches. Same
            // reason `wildcard` is sent — omitted it defaults to false, turning a
            // match-everything row into a criterion-less one.
            criteria: targetFp.criteria,
            wildcard: targetFp.wildcard,
            // Into the existing config, never over it: the PUT replaces the row, so
            // the other groups and the classifier's own flags have to be carried.
            // Into the list being staged, through that group's own writer. The
            // wallet rules ride along only on the tagged list — `m_dump_ix` has none.
            metric_config: withFlowWalletRules(
              metricConfigWithList(targetFp.metric_config ?? {}, draftPatterns, stageInto),
              walletRules,
            ),
          },
        }).unwrap();
        setApplyOk(`Updated fingerprint “${targetFp.name}”.`);
      } else {
        // Bind builds the fingerprint from the posted key alone — unlike the sweep's
        // `promote_group`, it has no run row to read the label filter off. So the
        // key we post must already carry it, or the bound fingerprint silently
        // drops the `ix_labels` axis and arms on every token shape. Identity, name
        // and the badge all read this one resolved key, so they cannot disagree.
        const boundKey = withIxLabelsFilter(selectedGroup.group_key, runIxLabels);
        // The key carries the window it selected, so bind is a copy — there is no
        // precision to pass along, and so no substituted precision that could arm the
        // bound rule on a window the card never showed.
        const fp = await bindFp({
          group_key: boundKey,
          ix_patterns: patterns,
          list: stageInto,
          name: fingerprintNameFromGroupKey(boundKey),
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
      <PageHeader
        title="Flow discovery"
        description="Rank ix structures per fingerprint group → toggle volume patterns"
      />

      <div className="mb-4 flex flex-wrap items-end gap-3 bg-surface">
        <div className="flex flex-col gap-1">
          <LabelTip
            tip={DISCOVERY_FIELD_HELP.createdRange}
            className="text-[9px] font-bold uppercase tracking-wider text-text-dim/80"
          >
            Created range · UTC
          </LabelTip>
          <DateTimeRangePicker
            aria-label="Created range"
            zoneLabel="UTC"
            emptyLabel="All history"
            customPreset="custom"
            value={{ preset: 'custom', from: createdAfter, to: createdBefore }}
            onChange={({ from, to }) => {
              setField('createdAfter', from);
              setField('createdBefore', to);
            }}
          />
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
          disabled={running || !!ixFilterError || !!fieldFilterError || startState.isLoading}
          label={running ? 'Discovering…' : 'Run discovery'}
          title={running ? 'Discovering…' : 'Run discovery'}
        >
          {running ? <SpinnerIcon /> : <PlayIcon />}
        </IconButton>
      </div>

      <div className="mb-4 border-t border-white/10 pt-3">
        <Accordion title="Group by fingerprint" defaultOpen>
          <FingerprintScopeControl
            fingerprints={fingerprints}
            value={seedFingerprintId}
            onChange={selectSeedFingerprint}
            tip={DISCOVERY_FIELD_HELP.seedFingerprint}
            scopedDescription="Discovery scores only tokens that match this fingerprint, then Apply writes ix_patterns back to it."
            manualHint="Pick a fingerprint to detect its ix_patterns — or leave empty and use the manual group-by / filters."
            matchedCount={fpMatches.count}
            matchedCountLoading={fpMatches.countLoading}
            onViewMatches={fpMatches.openMatches}
            onRequestMatchCount={fpMatches.ensureCount}
          />
          {fpMatches.matchesModal}
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
            onClearFilters={() =>
              setConfig((prev) => ({
                ...DEFAULTS,
                ...prev,
                fieldFiltersText: {},
                cashbackFilter: 'all',
                ixLabelsFilter: '',
              }))
            }
            cashbackFilter={cashbackFilter}
            onSetCashback={(v) => setField('cashbackFilter', v)}
            partition={partition}
            onSetPartition={(f, spec) =>
              setField('partition', { ...partition, [f]: spec })
            }
            ixLabelsText={ixLabelsFilter}
            onSetIxLabels={(v) => setField('ixLabelsFilter', v)}
            ixFilter={ixFilter}
            filtersDisabled={!!seedFingerprintId}
            emptyHint={
              seedFingerprintId
                ? 'Scoped to the saved fingerprint — value filters are not sent; group-by still splits the matched slice.'
                : 'No fields selected → one "ALL" group (noisy lift).'
            }
          />
        </Accordion>
      </div>

      {(applyError || ixFilterError || fieldFilterError || startState.error) && (
        <InlineAlert variant="error">
          {applyError || ixFilterError || fieldFilterError || apiErrorMessage(startState.error, "Error")}
        </InlineAlert>
      )}
      {applyOk && <InlineAlert variant="success">{applyOk}</InlineAlert>}

      {result && (
        <div className={`flex flex-col gap-3 ${groupsOpen ? 'lg:flex-row' : ''}`}>
          <Accordion
            className={`shrink-0 ${groupsOpen ? 'lg:w-72' : ''}`}
            bordered={false}
            padding="none"
            open={groupsOpen}
            onOpenChange={setGroupsOpen}
            title={
              <span className="text-xs font-semibold uppercase tracking-wide text-text-dim">
                Groups ({result.groups.length})
              </span>
            }
            badge={
              !groupsOpen && selectedGroup ? (
                <span className="max-w-[32ch] truncate font-mono text-[11px] text-text-mid">
                  {groupKeyLabel(selectedGroup.group_key)}
                </span>
              ) : undefined
            }
          >
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
          </Accordion>

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

                {flowIx && flowIx.totalGross > 0 && (
                  <div className="rounded border border-white/8 p-3">
                    <div className="mb-1.5 flex flex-wrap items-center justify-between gap-2">
                      <LabelTip
                        tip={DISCOVERY_FIELD_HELP.volumeSplit}
                        className="text-[9px] font-bold uppercase tracking-wider text-text-dim/80"
                      >
                        {stageInto === 'dump' ? 'Dump cover' : 'Flow split'} · checked structures
                      </LabelTip>
                      <span className="font-mono text-[11px] text-text-dim">
                        {fmt(flowIx.volumePct)}% {stageInto === 'dump' ? 'dump' : 'volume'} of{' '}
                        {fmt(flowIx.totalGross)}◎ scored
                      </span>
                    </div>
                    <div className="flex h-2 w-full overflow-hidden rounded-full bg-white/6">
                      {flowIx.volumeGross > 0 && (
                        <div
                          className="h-full rounded-full bg-warning"
                          style={{
                            width: `${flowIx.volumePct}%`,
                            marginRight: flowIx.organicGross > 0 ? 2 : 0,
                          }}
                        />
                      )}
                      {flowIx.organicGross > 0 && (
                        <div
                          className="h-full rounded-full bg-white/20"
                          style={{ width: `${100 - flowIx.volumePct}%` }}
                        />
                      )}
                    </div>
                    <div className="mt-1.5 flex flex-wrap items-center gap-3 text-[10px] text-text-dim">
                      <span className="inline-flex items-center gap-1">
                        <span className="size-2 rounded-full bg-warning" />{' '}
                        {stageInto === 'dump' ? 'Dump builds' : 'Volume'} (checked):{' '}
                        {fmt(flowIx.volumeGross)}◎
                      </span>
                      <span className="inline-flex items-center gap-1">
                        <span className="size-2 rounded-full bg-white/30" />{' '}
                        {stageInto === 'dump' ? 'Everything else' : 'Organic'} (unchecked):{' '}
                        {fmt(flowIx.organicGross)}◎
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
                    tokenCreatedAt={previewDetail?.created_at ?? null}
                    patternKeys={patternKeys}
                    patternRowKeys={draftRowKeys}
                    feePins={feePins}
                    onFeePinsChange={setFeePins}
                    onTogglePattern={toggleTrade}
                  />
                )}

              </div>

              <DraftPatternsCart
                draftPatterns={draftPatterns}
                onChange={setDraftPatterns}
                currentPatterns={currentPatterns}
                targetFp={targetFp}
                stageInto={stageInto}
                onStageIntoChange={changeStageInto}
                walletRules={walletRules}
                savedWalletRules={savedWalletRules}
                onWalletRulesChange={setWalletRules}
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
                    {filteredIsNarrowed && (
                      <Badge
                        variant="neutral"
                        size="sm"
                        title="The table filters are hiding rows — the two filtered buttons act on the shown ones only, the three group buttons still read the whole group"
                      >
                        {filteredStructures.length} of {selectedGroup.structures.length} shown
                      </Badge>
                    )}
                    {suggestedUnchecked.length > 0 && (
                      <Badge variant="warning" size="sm">
                        {suggestedUnchecked.length} suggested
                      </Badge>
                    )}
                    {firstSlotAll.length > 0 && (
                      <Badge variant="info" size="sm">
                        {firstSlotAll.length} at launch
                        {firstSlotUnchecked.length === 0
                          ? ' · all staged'
                          : ` · ${firstSlotUnchecked.length} new`}
                      </Badge>
                    )}
                    {firstSlotAll.length === 0 && firstSlotUnscored && (
                      <Badge variant="neutral" size="sm">
                        launch presence unscored
                      </Badge>
                    )}
                    {selectedTokenRow && !tokenLaunchUnscored && (
                      <Badge variant="info" size="sm">
                        {tokenLaunchAll.length} in this token&apos;s slot
                        {selectedTokenRow.first_slot != null
                          ? ` (${selectedTokenRow.first_slot})`
                          : ''}
                      </Badge>
                    )}
                  </span>
                  <div className="flex flex-wrap items-center gap-2">
                    {selectedTokenRow && (
                      <button
                        type="button"
                        disabled={tokenLaunchAll.length === 0}
                        onClick={autoSelectTokenLaunch}
                        {...previewProps(tokenLaunchAll)}
                        className="inline-flex items-center gap-1 rounded border border-info/40 px-2 py-1 text-[11px] font-semibold text-info transition hover:bg-info/10 disabled:cursor-not-allowed disabled:opacity-40"
                        title={
                          tokenLaunchAll.length === 0
                            ? tokenLaunchUnscored
                              ? 'This run predates the per-token launch set, so the creation-slot shapes of the previewed token are unknown. Re-run discovery.'
                              : 'No trade in the creation slot of the previewed token carried ix_labels'
                            : `Add every ix shape that traded in the creation slot of the previewed token${
                                selectedTokenRow.first_slot != null
                                  ? ` (slot ${selectedTokenRow.first_slot})`
                                  : ''
                              } — ${tokenLaunchAll.length} shape(s), ${
                                tokenLaunchUnchecked.length
                              } not yet staged. Uncapped and unfloored: unlike the group button this is not read off the ranked table, so a shape too small or too low-ranked to appear as a row above is still added (the hover outline can only mark the ones that do have a row).`
                        }
                      >
                        <CheckIcon className="h-3.5 w-3.5" />
                        Launch shapes · this token
                      </button>
                    )}
                    <button
                      type="button"
                      disabled={firstSlotAll.length === 0}
                      onClick={autoSelectFirstSlot}
                      {...previewProps(firstSlotAll)}
                      className="inline-flex items-center gap-1 rounded border border-info/40 px-2 py-1 text-[11px] font-semibold text-info transition hover:bg-info/10 disabled:cursor-not-allowed disabled:opacity-40"
                      title={
                        firstSlotAll.length === 0
                          ? firstSlotUnscored
                            ? 'No structure in this group carries a first-slot count — the run predates the backend field, so launch presence is unknown. Re-run discovery.'
                            : "No structure in this group traded in a matched token's creation slot"
                          : firstSlotUnchecked.length === 0
                            ? `All ${firstSlotAll.length} launch shapes are already staged (hover outlines them) — the draft is re-seeded from the target fingerprint's saved patterns on every run`
                            : `Add the ${firstSlotUnchecked.length} not-yet-staged structure(s) that appear in ANY matched token's creation slot — the launch bundle, create instruction included — to the draft ix_patterns. Group-wide and read off the ranked table above, so it can miss a shape that fell outside the server-side row cap; for one token's exact bundle, pick it in the preview and use the per-token button.`
                      }
                    >
                      <CheckIcon className="h-3.5 w-3.5" />
                      Launch shapes · group
                    </button>
                    <button
                      type="button"
                      disabled={suggestedUnchecked.length === 0}
                      onClick={autoSelectSuggested}
                      {...previewProps(suggestedUnchecked)}
                      className="inline-flex items-center gap-1 rounded border border-warning/40 px-2 py-1 text-[11px] font-semibold text-warning transition hover:bg-warning/10 disabled:cursor-not-allowed disabled:opacity-40"
                      title="Add every auto-suggested structure to the draft ix_patterns"
                    >
                      <CheckIcon className="h-3.5 w-3.5" />
                      Auto-select suggested
                    </button>
                    <span className="mx-0.5 h-4 w-px bg-border" aria-hidden />
                    <button
                      type="button"
                      disabled={filteredUnstaged.length === 0}
                      onClick={stageFiltered}
                      {...previewProps(filteredAll)}
                      className="inline-flex items-center gap-1 rounded border border-accent/40 px-2 py-1 text-[11px] font-semibold text-accent transition hover:bg-accent/10 disabled:cursor-not-allowed disabled:opacity-40"
                      title={
                        filteredStructures.length === 0
                          ? 'No row matches the table filters'
                          : filteredUnstaged.length === 0
                            ? `All ${filteredStructures.length} filtered row(s) are already staged (hover outlines them)`
                            : `Stage the ${filteredUnstaged.length} not-yet-staged row(s) of the ${filteredStructures.length} the table filters currently show. Reads the TABLE, not the group — so it stages exactly the slice you can see, and nothing the server-side row cap left out.`
                      }
                    >
                      <CheckIcon className="h-3.5 w-3.5" />
                      Stage filtered
                      {filteredUnstaged.length > 0 ? ` · ${filteredUnstaged.length}` : ''}
                    </button>
                    <button
                      type="button"
                      disabled={filteredStaged.length === 0}
                      onClick={unstageFiltered}
                      {...previewProps(filteredAll)}
                      className="inline-flex items-center gap-1 rounded border border-border px-2 py-1 text-[11px] font-semibold text-text-dim transition hover:bg-white/5 hover:text-text disabled:cursor-not-allowed disabled:opacity-40"
                      title={
                        filteredStaged.length === 0
                          ? 'No filtered row is staged'
                          : `Unstage the ${filteredStaged.length} staged row(s) of the ${filteredStructures.length} the table filters currently show. Only reaches rows that HAVE a row here: the draft is re-seeded from the target fingerprint's saved patterns, so a staged pattern absent from this group is untouched — remove that one in the cart below.`
                      }
                    >
                      Unstage filtered
                      {filteredStaged.length > 0 ? ` · ${filteredStaged.length}` : ''}
                    </button>
                  </div>
                </div>
                <StructureTable
                  structures={selectedGroup.structures}
                  draftPatterns={draftUnpinned}
                  contagionByStructure={contagionByStructure}
                  suggestionByStructure={suggestionByStructure}
                  liftDefined={liftDefined}
                  previewKeys={previewKeys}
                  onToggle={toggleStructure}
                  onFilteredRowsChange={handleFilteredStructures}
                />
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
