import { defaultTagName, flowTagOf } from 'lib/flow/tapeClassify';
import { parseIxPatternRow, patternRowKey, type IxPatternRow } from 'lib/strategy/ixPatternRows';

/**
 * Non-empty {@link patternRowKey} set of `ix_shape` rows, or `null` when there are
 * none. A row keys whole - core level, side, marks and fee pins - so a key set
 * classifies the same trades its rows do. An unpinned ix structure keys as its bare
 * `JSON.stringify(labels)`.
 */
export function flowPatternKeysOf(rows: readonly IxPatternRow[] | null | undefined): ReadonlySet<string> | null {
  if (!rows?.length) return null;
  const keys = new Set(rows.filter((r) => r.labels.some((l) => l.trim())).map(patternRowKey));
  return keys.size > 0 ? keys : null;
}

/** The `ix_shape` rows of tag `name` - by default the document's default tag - as
 *  keys. A key set is the fallback a host without a fingerprint id classifies with;
 *  a host that has the id classifies with the whole tag. */
export function flowPatternKeysFromTags(
  doc: unknown,
  name: string = defaultTagName(doc),
): ReadonlySet<string> | null {
  return flowPatternKeysOf(flowTagOf(doc, name)?.match.ix_shape);
}

/** Rows back from a key set. A key that is not a stored row (a template grain id,
 *  a corrupted store) is dropped. */
export function rowsFromKeys(keys: ReadonlySet<string> | null | undefined): IxPatternRow[] {
  if (!keys) return [];
  const out: IxPatternRow[] = [];
  for (const key of keys) {
    let parsed: unknown;
    try {
      parsed = JSON.parse(key);
    } catch {
      continue;
    }
    const row = parseIxPatternRow(parsed);
    if (row) out.push(row);
  }
  return out;
}
