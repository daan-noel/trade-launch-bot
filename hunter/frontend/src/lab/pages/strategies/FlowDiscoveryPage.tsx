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
import { Input } from 'components/ui/Input';
import { Select } from 'components/ui/Select';
import { InlineAlert } from 'components/ui/Modal';
import { PageHeader } from 'components/ui/PageHeader';
import {
  fingerprintParamsCell,
  chip as paramChip,
  axisTint,
} from 'components/strategy/FingerprintParamsSummary';
import { LabelTip } from 'components/strategy/LabelTip';
import { FingerprintScopeControl } from 'components/strategy/FingerprintScopeControl';
import { useFingerprintMatches } from '@lab/components/strategy/useFingerprintMatches';
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
} from 'lib/strategy/strategyHelp';
import { formatIxLabelsText } from 'lib/ixLabels';
import { DraftPatternsCart } from '@lab/components/flow/DraftPatternsCart';
import {
  isFirstSlotPresent,
  suggestStructure,
  type StructureSuggestion,
} from '@lab/components/flow/flowDiscoverySuggest';
import { StructureTable } from '@lab/components/flow/StructureTable';
import { TokenPreviewPanel } from '@lab/components/flow/TokenPreviewPanel';
import { patternKeysFrom } from '@lab/lib/flow/classifyFlow';
import { FingerprintGroupPicker } from '@lab/components/sweep/FingerprintGroupPicker';
import { parseIxLabelsFilter, buildFieldFilters } from '@lab/components/sweep/fingerprintFilters';
import {
  GROUP_FIELD_LABELS,
  BUCKETED_GROUP_FIELDS,
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
} from 'types';
import {
  findFingerprintForGroupKey,
  withIxLabelsFilter,
} from 'lib/strategy/matchGroupFingerprint';
import { fingerprintNameFromGroupKey } from 'lib/strategy/fingerprintNameFromGroupKey';
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
  /** Group the SOL axes on their exact amount (`SolPrecision::Exact`) instead of a
   *  `bucketWidthSol`-wide range. A promoted/bound fingerprint stores a NULL width. */
  exactSol: boolean;
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
  exactSol: false,
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
    // A NULL width IS the exact mode — mirror it rather than substituting 0.1.
    exactSol: fp.bucket_size_amount == null,
    bucketWidthSol: tidySolDecimal(fp.bucket_size_amount ?? SOL_BUCKET_WIDTH),
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
    exactSol,
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
        bucketed: BUCKETED_GROUP_FIELDS,
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
  const seedFp = seedFingerprintId
    ? fingerprints.find((f) => f.id === seedFingerprintId)
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

  // ── The displayed result's OWN identity ───────────────────────────────────
  // Read off the run, never off the form above it. `result` can be a disk-cached
  // run from an earlier session (see the rehydrate effect), so the live config is
  // not its identity — and precision + label filter are both part of what a group
  // binds to. A pre-echo cached result omits these; fall back to the form, which
  // is the best available guess and matches the old behaviour exactly.
  /** The width the RUN grouped at — `null` means it keyed exact amounts. */
  const runWidth: number | null = useMemo(() => {
    if (result && result.bucket_width_sol !== undefined) {
      return result.bucket_width_sol == null ? null : tidySolDecimal(result.bucket_width_sol);
    }
    return exactSol ? null : tidySolDecimal(bucketWidthSol);
  }, [result, exactSol, bucketWidthSol]);
  /** The exact-set label filter the RUN applied, or null. */
  const runIxLabels: string[] | null = useMemo(() => {
    if (result && result.ix_labels_filter !== undefined) return result.ix_labels_filter;
    return ixLabelsGrouped ? null : ixFilter.labels;
  }, [result, ixLabelsGrouped, ixFilter.labels]);
  /** The saved fingerprint the RUN was scoped to, if any. */
  const runScopeFp: Fingerprint | null = useMemo(() => {
    const id = result && result.fingerprint_id !== undefined ? result.fingerprint_id : seedFingerprintId;
    return (id && fingerprints.find((f) => f.id === id)) || null;
  }, [result, seedFingerprintId, fingerprints]);

  /** Resolve one group to its saved fingerprint, exactly as promote/bind would.
   *
   *  A scoped run pins the whole corpus to one fingerprint and its groups are
   *  sub-slices of it, so that fingerprint is the authoritative attribution — and
   *  a scoped run's key is usually `{}`, which would otherwise fuzzily match any
   *  unrelated fingerprint that merely shares the precision. Same precedence the
   *  grouped sweep uses. */
  const resolveGroupFp = useCallback(
    (groupKey: Record<string, string>): Fingerprint | null =>
      runScopeFp ??
      findFingerprintForGroupKey(
        withIxLabelsFilter(groupKey, runIxLabels),
        fingerprints,
        runWidth,
      ),
    [runScopeFp, runIxLabels, fingerprints, runWidth],
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
    const draftKeys = new Set(draftPatterns.map((p) => JSON.stringify(p)));
    return selectedGroup.structures
      .filter((s) => {
        const key = JSON.stringify(s.ix_labels);
        return suggestionByStructure.get(key)?.suggested && !draftKeys.has(key);
      })
      .map((s) => s.ix_labels);
  }, [selectedGroup, suggestionByStructure, draftPatterns]);

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
    const draftKeys = new Set(draftPatterns.map((p) => JSON.stringify(p)));
    return firstSlotAll.filter((labels) => !draftKeys.has(JSON.stringify(labels)));
  }, [firstSlotAll, draftPatterns]);
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

  function autoSelectSuggested() {
    if (suggestedUnchecked.length === 0) return;
    setDraftPatterns((prev) => [...prev, ...suggestedUnchecked]);
    setApplyOk(null);
  }

  function autoSelectFirstSlot() {
    if (firstSlotUnchecked.length === 0) return;
    setDraftPatterns((prev) => [...prev, ...firstSlotUnchecked]);
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
  const currentPatterns = volumeIxPatternsFromConfig(targetFp?.metric_config ?? {});

  /** Point the apply target at a fingerprint and load its SAVED patterns into the
   *  draft — the ONE seeding path, so every trigger (group change, late list load,
   *  manual pick) stages the same thing. `null` ⇒ promote-style bind, empty draft. */
  const seedFromFingerprint = useCallback(
    (id: string | null) => {
      setTargetFpId(id);
      const fp = id ? fingerprints.find((f) => f.id === id) : null;
      setDraftPatterns(fp ? volumeIxPatternsFromConfig(fp.metric_config) : []);
      setApplyOk(null);
    },
    [fingerprints],
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
    const fp = fingerprints.find((f) => f.id === id);
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
        ...(exactSol ? { exact_sol: true } : { bucket_width_sol: bucketWidthSol }),
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
        // Bind builds the fingerprint from the posted key alone — unlike the sweep's
        // `promote_group`, it has no run row to read the label filter off. So the
        // key we post must already carry it, or the bound fingerprint silently
        // drops the `ix_labels` axis and arms on every token shape. Identity, name
        // and the badge all read this one resolved key, so they cannot disagree.
        const boundKey = withIxLabelsFilter(selectedGroup.group_key, runIxLabels);
        const fp = await bindFp({
          group_key: boundKey,
          // Exact mode replaces the width outright (backend ignores it there).
          // Both come from the RUN, not the form — binding a rehydrated result at
          // the form's precision would arm on a window the card never showed.
          ...(runWidth == null ? { exact_sol: true } : { bucket_width_sol: runWidth }),
          volume_ix_patterns: patterns,
          name: fingerprintNameFromGroupKey(boundKey, 'f', runWidth),
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
            scopedDescription="Discovery scores only tokens that match this fingerprint, then Apply writes volume_ix_patterns back to it."
            manualHint="Pick a fingerprint to detect its volume_ix_patterns — or leave empty and use the manual group-by / filters."
            matchedCount={fpMatches.count}
            matchedCountLoading={fpMatches.countLoading}
            onViewMatches={fpMatches.openMatches}
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
            cashbackFilter={cashbackFilter}
            onSetCashback={(v) => setField('cashbackFilter', v)}
            bucketWidthSol={bucketWidthSol}
            onSetBucketWidth={(n) =>
              setField('bucketWidthSol', n <= 0 ? SOL_BUCKET_WIDTH : n)
            }
            exactSol={exactSol}
            onSetExactSol={(v) => setField('exactSol', v)}
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

      {(applyError || ixFilterError || fieldFilterError || startState.error) && (
        <InlineAlert variant="error">
          {applyError || ixFilterError || fieldFilterError || apiErrorMessage(startState.error, "Error")}
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
                  </span>
                  <div className="flex flex-wrap items-center gap-2">
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
                            : `Add the ${firstSlotUnchecked.length} not-yet-staged structure(s) that appear in a matched token's creation slot — the launch bundle, create instruction included — to the draft volume_ix_patterns`
                      }
                    >
                      <CheckIcon className="h-3.5 w-3.5" />
                      Select launch shapes
                    </button>
                    <button
                      type="button"
                      disabled={suggestedUnchecked.length === 0}
                      onClick={autoSelectSuggested}
                      {...previewProps(suggestedUnchecked)}
                      className="inline-flex items-center gap-1 rounded border border-warning/40 px-2 py-1 text-[11px] font-semibold text-warning transition hover:bg-warning/10 disabled:cursor-not-allowed disabled:opacity-40"
                      title="Add every auto-suggested structure to the draft volume_ix_patterns"
                    >
                      <CheckIcon className="h-3.5 w-3.5" />
                      Auto-select suggested
                    </button>
                  </div>
                </div>
                <StructureTable
                  structures={selectedGroup.structures}
                  draftPatterns={draftPatterns}
                  contagionByStructure={contagionByStructure}
                  suggestionByStructure={suggestionByStructure}
                  liftDefined={liftDefined}
                  previewKeys={previewKeys}
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
