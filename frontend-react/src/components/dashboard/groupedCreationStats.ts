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
} from 'components/sweep/groupedTypes';
import { WALLET_MARKER_COLORS } from 'components/token-price-chart/constants';
import type { CreationBucket, CreationSegment } from './creationStats';

export { GROUP_FIELDS, GROUP_FIELD_LABELS };
export type { GroupField };

/** One ranked group: `g` = 0-based rank (0 = largest), `group_key` = the
 *  fingerprint values (e.g. `{cu_limit:"200000", ix_labels:"A | B"}`). */
export interface GroupedCreationGroup {
  g: number;
  group_key: Record<string, string>;
  total: number;
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
  /** The applied per-field value filters echoed back (`{cu_limit:["300000"]}`). */
  field_filters: Record<string, string[]>;
  /** The applied exact instruction-label set filter, or `null` when none. */
  ix_labels_filter: string[] | null;
  total: number;
  groups: GroupedCreationGroup[];
  cells: GroupedCreationCell[];
  points: GroupedCreationPoint[];
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
  /** Per-field value filters restricting the corpus before partitioning (keys =
   *  GroupField tags, values = allowed string forms). Independent of `groupBy`.
   *  Empty/omitted ⇒ no filter. `ix_labels` uses `ixLabelsFilter` instead. */
  fieldFilters?: Record<string, string[]>;
  /** Exact instruction-label set filter (set-equality). Omitted ⇒ no filter. */
  ixLabelsFilter?: string[];
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
