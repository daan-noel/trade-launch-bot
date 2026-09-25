import { patternKeysFrom } from 'lib/flow/classifyFlow';
import { defaultTagName, flowTagOf } from 'lib/flow/tapeClassify';

/**
 * Non-empty `JSON.stringify(labels)` key set of exact ix shapes, or `null` when there
 * are none.
 */
export function flowPatternKeysOf(
  patterns: readonly (readonly string[])[] | null | undefined,
): ReadonlySet<string> | null {
  if (!patterns?.length) return null;
  const keys = patternKeysFrom(patterns);
  return keys.size > 0 ? keys : null;
}

/** The exact ix shapes (labels only) of tag `name` - by default the document's
 *  default tag - as keys. A key set is the fallback a host without a fingerprint id
 *  classifies with; a host that has the id classifies with the whole tag. */
export function flowPatternKeysFromTags(
  doc: unknown,
  name: string = defaultTagName(doc),
): ReadonlySet<string> | null {
  return flowPatternKeysOf(flowTagOf(doc, name)?.match.ix_shape?.map((r) => r.labels));
}
