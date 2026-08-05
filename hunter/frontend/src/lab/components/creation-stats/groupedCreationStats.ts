// Per-fingerprint creation-activity dashboard — shared types + helpers.
// Mirrors the backend `GET /api/tokens/creation-stats/grouped` response (handler
// `creation_stats.rs::get_grouped_creation_stats`). Count-only: each fingerprint
// group gets a day×hour fold (`cells`, reused by `CreationHeatmap`) and a calendar
// trend (`points`, drawn by `GroupedCreationTrendChart`).
//
// The grouping fields reuse the sweep page's canonical fingerprint list
// (`GROUP_FIELDS` / `GROUP_FIELD_LABELS`) so a group here is the same group there.

import {
  GROUP_FIELDS,
  GROUP_FIELD_LABELS,
  type GroupField,
} from '@lab/components/sweep/groupedTypes';
import type { FieldFilterValue } from '@lab/components/sweep/fingerprintFilters';
import { WALLET_MARKER_COLORS } from 'components/token-price-chart/constants';
import type { CreationBucket, CreationSegment } from 'components/creation-stats/creationStats';
import type { SortEntry, TableQuery } from 'components/table/types';
import type { FilterSpec } from 'components/table/numericFilter';
import { toTableRequest } from 'services/tableRequest';
import { TOKEN_INFO_AMOUNT_COLS } from 'components/tokens/sharedTokenColumns';
import type { TokenRecord } from 'types';

export { GROUP_FIELDS, GROUP_FIELD_LABELS };
export type { GroupField };

/** Group ranking criterion: `count` (default, token count — unchanged) |
 *  `trades` (raw `SUM(trade_count)`, still scales with group size like
 *  `count`) | `trades_per_token` (the one that actually surfaces a small
 *  elite group over a big group of mediocre launches). */
export type GroupRankBy = 'count' | 'trades' | 'trades_per_token';

export const RANK_BY_OPTIONS: { value: GroupRankBy; label: string }[] = [
  { value: 'count', label: 'Tokens' },
  { value: 'trades', label: 'Trades' },
  { value: 'trades_per_token', label: 'Trades per token' },
];

/** One ranked group: `g` = 0-based rank (0 = largest), `group_key` = the
 *  fingerprint values (e.g. `{cu_limit:"200000", ix_labels:"A | B"}`). */
export interface GroupedCreationGroup {
  g: number;
  group_key: Record<string, string>;
  total: number;
  /** Lifetime-to-last-sync trade count summed over the group. */
  trades: number;
  /** `trades / total` — the per-token figure `rank_by=trades_per_token` ranks on. */
  trades_avg: number;
}

/** One day-of-week × hour-of-day cell for group `g`. `dow`: 0=Sun … 6=Sat. */
export interface GroupedCreationCell {
  g: number;
  dow: number;
  hour: number;
  count: number;
}

/** One calendar bucket for group `g`; `bucket` is local wall-clock (naive ISO). */
export interface GroupedCreationPoint {
  g: number;
  bucket: string;
  count: number;
}

export interface GroupedCreationResponse {
  bucket: CreationBucket;
  tz: string;
  from: string;
  to: string;
  segment: string;
  group_by: GroupField[];
  /** The applied (clamped) bucket width (SOL) for the continuous SOL group fields,
   *  or `null` when the run keyed them on their **exact** amount (`exactSol`).
   *  Carry the `null` through to identity match/create verbatim — it is the width a
   *  fingerprint saved from such a card stores, and `SolPrecision::Exact` is what
   *  the live engine then matches those axes with. Substituting a width there mints
   *  a rule that arms on a window the card never showed. */
  bucket_width: number | null;
  /** The applied per-field value filters echoed back (`{cu_limit:["300000"]}`). */
  field_filters: Record<string, string[]>;
  /** The applied exact instruction-label set filter, or `null` when none. */
  ix_labels_filter: string[] | null;
  /** The applied (whitelisted) ranking criterion. Inert under a saved-fingerprint
   *  scope — there's only ever one group there. */
  rank_by: GroupRankBy;
  total: number;
  groups: GroupedCreationGroup[];
  cells: GroupedCreationCell[];
  points: GroupedCreationPoint[];
}

/**
 * Args for the "drill-down" tokens table behind one group card (or one of its
 * heatmap tiles): the same window/segment/corpus selectors {@link GroupedCreationArgs}
 * applies (minus `top` — there's no ranking here, just one exact group) plus the
 * two selectors pinning it to a single row (`groupKey`, and for a tile click
 * `dow`/`hour`), plus the drill-down table's own view-state. Mirrors the backend
 * `POST /api/tokens/creation-stats/grouped/tokens` body
 * (`creation_stats.rs::get_grouped_creation_tokens`).
 */
export interface GroupedCreationTokensArgs {
  tz: string;
  from?: string;
  segment: CreationSegment;
  groupBy: GroupField[];
  bucketWidth?: number;
  /** Mirrors {@link GroupedCreationArgs.exactSol} — MUST match the stats request
   *  that produced `groupKey`, or the key is rendered in the other mode. */
  exactSol?: boolean;
  fieldFilters?: Record<string, FieldFilterValue[]>;
  ixLabelsFilter?: string[];
  /** Saved-fingerprint scope — mirrors {@link GroupedCreationArgs.fingerprintId}.
   *  When set, `groupBy`/`fieldFilters`/`ixLabelsFilter`/`groupKey` are all
   *  ignored (there's only ever one group, `g = 0`). */
  fingerprintId?: string;
  /** The exact group to drill into — echoes {@link GroupedCreationGroup.group_key} verbatim. */
  groupKey: Record<string, string>;
  /** Recurring weekly slot (a heatmap-tile click): 0=Sun..6=Sat / 0..23. Both set,
   *  or both omitted for the whole group. */
  dow?: number;
  hour?: number;
  page: number;
  pageSize: number;
  sortKeys: SortEntry[];
  search: string;
  /** Per-column filters from the drill-in TokenTable, already lowered to the
   *  unified `TableRequest.filters` grammar (via {@link drillTokenFilters}). The
   *  backend layers these onto the group's corpus scope so they narrow the rows
   *  AND the pager total. Empty/omitted ⇒ no per-column filter. */
  filters?: Record<string, FilterSpec>;
}

/**
 * Serialize a drill-in TokenTable's view-state per-column filters into the
 * `TableRequest.filters` map the grouped-tokens endpoint applies. Uses the SAME
 * serializer path as the live Tokens list ({@link tokensTableRequestBody}): an
 * empty `numericCols` set (the backend re-parses each raw predicate via
 * `lower_filter`) plus `TOKEN_INFO_AMOUNT_COLS` so SOL amount-column filters
 * rewrite display→storage units before that re-parse. Search/sort/page are
 * carried by the args' own fields, so only `.filters` is lifted here.
 */
export function drillTokenFilters(query: TableQuery): Record<string, FilterSpec> {
  return toTableRequest(
    {
      page: query.page,
      pageSize: query.pageSize,
      sortKeys: query.sortKeys,
      search: query.search,
      colFilters: query.colFilters,
      structuredFilters: query.structuredFilters,
    },
    new Set(),
    { amountCols: TOKEN_INFO_AMOUNT_COLS },
  ).filters;
}

export interface GroupedCreationTokensResponse {
  total: number;
  items: TokenRecord[];
}

export interface GroupedCreationArgs {
  bucket: CreationBucket;
  tz: string;
  /** RFC3339; omit to use the backend default (last 30d). */
  from?: string;
  segment: CreationSegment;
  /** Compound-key fields, in selection order. */
  groupBy: GroupField[];
  /** Number of top groups to return. */
  top: number;
  /** Bucket width (SOL) for the continuous SOL group fields — the same knob the
   *  grouped sweep uses, so the dashboard groups a corpus identically to a sweep at
   *  this width. Omitted ⇒ the backend default (0.1). */
  bucketWidth?: number;
  /** `true` ⇒ key the continuous SOL fields on their **exact** amount instead of a
   *  bucket range, so each distinct value forms its own group. Mutually exclusive
   *  with `bucketWidth` (the backend ignores the width in this mode). A separate
   *  named flag, never a magic width of 0 — see Rust `SolPrecision`. */
  exactSol?: boolean;
  /** Per-field value filters restricting the corpus before partitioning (keys =
   *  GroupField tags, values = allowed string forms). Independent of `groupBy`.
   *  Empty/omitted ⇒ no filter. `ix_labels` uses `ixLabelsFilter` instead. */
  fieldFilters?: Record<string, FieldFilterValue[]>;
  /** Exact instruction-label set filter (set-equality). Omitted ⇒ no filter. */
  ixLabelsFilter?: string[];
  /** Group ranking criterion. Omitted when `count` so the default keeps a
   *  stable RTK cache key (same trick `bucketWidth` uses). Inert under a
   *  saved-fingerprint scope. */
  rankBy?: GroupRankBy;
  /** Saved-fingerprint scope — same contract as the sweep's/flow discovery's
   *  `fingerprint_id`: when set, `groupBy`/`top`/`fieldFilters`/`ixLabelsFilter`
   *  above are all ignored; the corpus becomes the fingerprint's own
   *  engine-matched tokens, collapsed into a single "ALL" group (`g = 0`). */
  fingerprintId?: string;
}

/** Look-back-derived top-N presets for the group-count picker. */
export const TOP_OPTIONS: { value: number; label: string }[] = [
  { value: 4, label: 'Top 4' },
  { value: 8, label: 'Top 8' },
  { value: 12, label: 'Top 12' },
  { value: 16, label: 'Top 16' },
];

/** The `∅` sentinel the backend renders for a missing fingerprint value. */
export const MISSING_VALUE = '∅';

/** Stable per-group color (rank-indexed) shared by the trend series + legend +
 *  heatmap card accents so a group reads as the same color everywhere. */
export function groupColor(g: number): string {
  return WALLET_MARKER_COLORS[g % WALLET_MARKER_COLORS.length];
}

/**
 * Render one group-key field value for display. `ix_labels` arrives as a
 * `" | "`-joined list — split it so the caller can stack each label; everything
 * else is a scalar. Returns the list (length 1 for scalars).
 */
export function groupValueParts(field: string, value: string): string[] {
  return field === 'ix_labels' ? value.split(' | ') : [value];
}

/** A short one-line label for a group (used as a chart series name / card title).
 *  Joins each field as `label=value`, abbreviating long ix-label sets. */
export function groupShortLabel(group: GroupedCreationGroup): string {
  const entries = Object.entries(group.group_key);
  if (entries.length === 0) return 'ALL tokens';
  return entries
    .map(([k, v]) => {
      const label = GROUP_FIELD_LABELS[k as GroupField] ?? k;
      if (k === 'ix_labels') {
        const n = v === MISSING_VALUE ? 0 : groupValueParts(k, v).length;
        return `${label}=${n} ix`;
      }
      return `${label}=${v}`;
    })
    .join(' · ');
}
