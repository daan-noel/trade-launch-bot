// The strategy **registry**: the frontend mirror of `GET /api/meta/strategy-registry`
// (`hunter_engine::metrics::registry_json`). Every family, metric, span kind, tag
// matcher and rule part carries its one definition and example there; every picker,
// tooltip, sentence and the Guide page render that text as-is. A metric added in Rust
// reaches every surface on the next load with no frontend change.

import { REGISTRY_STALE_SECS, useGetStrategyRegistryQuery } from 'store/sharedEndpoints';
import type { CompareOp } from 'components/table/numericFilter';

/** The six comparison operators of the condition grammar. */
export type Operator = CompareOp;

/** A metric's unit. `flag` reads 0 or 1. */
export type MetricUnit = 'seconds' | 'sol' | 'percent' | 'count' | 'flag' | 'lamports';

/** Whether a metric takes a tag: never, optionally (no tag = every trade), or always. */
export type TagUse = 'none' | 'optional' | 'required';

/** What a metric's tag names: a fingerprint trade tag (`trade`), a tag read at the ix
 *  template level (`template`: only its `ix_template` / `program` matchers count), or a
 *  built-in wallet class (`wallet_class`: `bundled`, `public_app`). */
export type TagLevel = 'trade' | 'template' | 'wallet_class';

/** Which spans a metric accepts. */
export interface SpanUse {
  life: boolean;
  window: boolean;
  since_age: boolean;
  /** A trailing window with a nested `slice` beside it (required when true). */
  slice: boolean;
}

export interface MetricSpec {
  /** Name inside its family: `buy_sol`. */
  name: string;
  /** `m_flow.buy_sol`: the id a condition writes. */
  path: string;
  /** Short noun phrase for sentences, without tag or span: `SOL bought`. A flag's
   *  phrase is the statement its 1 means. */
  phrase: string;
  unit: MetricUnit;
  /** One line: what it measures. */
  summary: string;
  /** One line with a number. */
  example: string;
  /** What a reader has to know (when it reads NaN, what it excludes). May be empty. */
  note: string;
  tags: TagUse;
  tag_level: TagLevel;
  spans: SpanUse;
  monotonic: boolean;
  /** Width of the `=` / `!=` band. */
  eq_tolerance: number;
  /** UI hue, HSL degrees. */
  hue: number;
  /** Reads our own position: sell lines only. */
  position: boolean;
}

export interface FamilySpec {
  /** `m_flow`. */
  name: string;
  title: string;
  summary: string;
  example: string;
  metrics: MetricSpec[];
}

/** One span kind (`life`, `sec`, `slot`, `print`, `lag`, `since_age`, `slice`). */
export interface SpanKindSpec {
  key: string;
  /** How it is written, e.g. `10s`. Empty for life. */
  text: string;
  title: string;
  summary: string;
  example: string;
}

/** One key a fingerprint tag definition takes: a matcher under `match`, or an option
 *  beside it. */
export interface TagFieldSpec {
  key: string;
  kind: 'match' | 'option';
  /** `program[]`, `ix_shape[]`, `ix_template[]`, `marker[]`, `wallet[]`, `bool`,
   *  `cluster`, `side`. */
  value_type: string;
  title: string;
  summary: string;
  example: string;
}

/** One ix marker the `ix_contains` / `ix_lacks` matchers pick from. `router` = a retail
 *  front end a person clicks through, else a mechanism of the transaction. */
export interface IxMarkerSpec {
  name: string;
  router: boolean;
}

export interface TagsSpec {
  summary: string;
  example: string;
  fields: TagFieldSpec[];
  markers: IxMarkerSpec[];
  /** Wallet classes every coin has, readable by `m_holdings.bag_share_pct` only. */
  builtin: { name: string; summary: string }[];
}

/** One rule part (`enter.event`, `signals`, `stage.at_end`, ...). */
export interface RulePartSpec {
  key: string;
  title: string;
  summary: string;
  example: string;
}

/** The whole registry payload. */
export interface StrategyRegistry {
  operators: Operator[];
  families: FamilySpec[];
  spans: SpanKindSpec[];
  tags: TagsSpec;
  rule_parts: RulePartSpec[];
}

/** Every metric, family by family in registry order. */
export function allMetrics(reg: StrategyRegistry | undefined): MetricSpec[] {
  return reg ? reg.families.flatMap((f) => f.metrics) : [];
}

/** A metric by path (`m_flow.buy_sol`). */
export function findMetric(reg: StrategyRegistry | undefined, path: string): MetricSpec | undefined {
  const fam = familyName(path);
  return reg?.families.find((f) => f.name === fam)?.metrics.find((m) => m.path === path);
}

/** A family by name (`m_flow`). */
export function findFamily(reg: StrategyRegistry | undefined, name: string): FamilySpec | undefined {
  return reg?.families.find((f) => f.name === name);
}

/** The family part of a metric path: `m_flow.buy_sol` -> `m_flow`. */
export function familyName(path: string): string {
  const i = path.indexOf('.');
  return i < 0 ? path : path.slice(0, i);
}

/** A rule part's text by key (`enter.filters`). */
export function rulePart(reg: StrategyRegistry | undefined, key: string): RulePartSpec | undefined {
  return reg?.rule_parts.find((p) => p.key === key);
}

/** A span kind by key (`sec`, `slot`, ...). */
export function spanKind(reg: StrategyRegistry | undefined, key: string): SpanKindSpec | undefined {
  return reg?.spans.find((s) => s.key === key);
}

/** A tag field by key (`program`, `sticky`, ...). */
export function tagField(reg: StrategyRegistry | undefined, key: string): TagFieldSpec | undefined {
  return reg?.tags.fields.find((f) => f.key === key);
}

/** The marker vocabulary, routers last so the two kinds read as two blocks. */
export function ixMarkers(reg: StrategyRegistry | undefined): IxMarkerSpec[] {
  return [...(reg?.tags.markers ?? [])].sort((a, b) => Number(a.router) - Number(b.router));
}

/** Tooltip text for a metric: summary, then the note when there is one. */
export function metricHelp(spec: MetricSpec): string {
  const parts = [spec.summary];
  if (spec.note) parts.push(spec.note);
  parts.push(`Example: ${spec.example}`);
  return parts.join('\n');
}

/** Short unit suffix for inputs and values (`◎` for SOL, `s`, `%`). A count and a
 *  flag are bare. */
export function unitSuffix(unit: MetricUnit): string {
  switch (unit) {
    case 'seconds':
      return 's';
    case 'sol':
      return '◎';
    case 'percent':
      return '%';
    case 'lamports':
      return ' lamports';
    case 'count':
    case 'flag':
      return '';
  }
}

/**
 * Cached, app-wide access to the registry. The payload is static for the backend
 * process lifetime, so it is fetched once and held for the session (see the long
 * `keepUnusedDataFor` on the endpoint).
 *
 * The tab, however, outlives the backend process. `refetchOnMountOrArgChange`
 * overrides the app-wide `false` so a copy older than `REGISTRY_STALE_SECS` is
 * re-read on the next mount; otherwise a restart that adds a metric leaves the
 * pickers rendering the previous vocabulary.
 */
export function useStrategyRegistry() {
  return useGetStrategyRegistryQuery(undefined, {
    refetchOnMountOrArgChange: REGISTRY_STALE_SECS,
  });
}
