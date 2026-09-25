/**
 * The ONE mapping from "which tag does this surface read" to classify options.
 *
 * The chart lines, the trades-table badges and the reasons map all go through here,
 * so a tag switch cannot leave the table answering one question and the lines
 * another.
 */

import type { IxPatternRow } from 'lib/strategy/ixPatternRows';
import { tagNames, tagsFromJson, usedMatchers, type TagMatch } from 'lib/strategy/tagsDoc';
import type { FlowClassifyOptions, FlowTag } from './classifyFlow';

/** The tag a surface reads when nothing picked one: `volume` when the document
 *  defines it (the name v1's flow list converts to), else its first tag, else
 *  `volume` (a click then creates it). */
export const DEFAULT_TAG = 'volume';

export function defaultTagName(doc: unknown): string {
  const names = tagNames(doc);
  return names.includes(DEFAULT_TAG) ? DEFAULT_TAG : (names[0] ?? DEFAULT_TAG);
}

/** Tag `name` of a stored `tags` document, or `null` when it does not define it. */
export function flowTagOf(doc: unknown, name: string): FlowTag | null {
  return tagsFromJson(doc).find((t) => t.name === name) ?? null;
}

/** `@name`, or `@!name` for the rest - how rules and every surface write a half. */
export function tagLabel(name: string, negated = false): string {
  return `@${negated ? '!' : ''}${name}`;
}

/** Whether the tag can classify anything: at least one matcher in use. */
export function tagClassifies(tag: FlowTag | null | undefined): tag is FlowTag {
  return !!tag && usedMatchers({ ...tag, id: '' }).length > 0;
}

/** An ad-hoc tag of exact ix shapes only - for a host that hands down a bare key set
 *  (a stored run's frozen shapes) rather than a fingerprint tag. */
export function shapeTag(name: string, rows: readonly IxPatternRow[]): FlowTag {
  return {
    name,
    match: rows.length > 0 ? { ix_shape: [...rows] } : {},
    side: null,
    sticky: false,
    exclude_creation_slot: false,
  };
}

/** `tag` with some matchers replaced - a staging draft previewed as the tag it
 *  would save. An empty list drops that matcher. */
export function withDraftMatch(tag: FlowTag, patch: Partial<TagMatch>): FlowTag {
  const match: TagMatch = { ...tag.match };
  for (const [k, v] of Object.entries(patch) as [keyof TagMatch, TagMatch[keyof TagMatch]][]) {
    if (v === undefined || (Array.isArray(v) && v.length === 0)) delete match[k];
    else (match as Record<string, unknown>)[k] = v;
  }
  return { ...tag, match };
}

/** Classify options for one tag, or `null` when nothing can classify. */
export function classifyOptsForTag(
  tag: FlowTag | null | undefined,
  creatorWallet?: string | null,
  excludeWallets?: ReadonlySet<string> | null,
): FlowClassifyOptions | null {
  if (!tagClassifies(tag)) return null;
  return { tag, creatorWallet: creatorWallet ?? null, excludeWallets: excludeWallets ?? null };
}
