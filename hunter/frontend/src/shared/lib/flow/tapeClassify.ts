/**
 * The ONE mapping from the strip's selected list to overlay classify options.
 *
 * Badges, chart lines and contagion notes all go through here so switching
 * tagged / dump / working cannot leave the table answering one question and the
 * lines another. `useFlowPatternSource` stays the engine's tagged snapshot for
 * metric panes; this is the live overlay.
 */

import {
  patternKeysFrom,
  type FlowClassifyOptions,
  type FlowSide,
} from './classifyFlow';
import type { IxPatternRow } from 'lib/strategy/ixPatternRows';
import type { TapeList } from 'lib/strategy/registry';

export interface TapeClassifyInput {
  list: TapeList;
  /** Label-sequence keys (tagged/dump) or grain ids (working). */
  keys: ReadonlySet<string> | null | undefined;
  /** Whole rows when the list carries fee pins. Ignored on `'working'`. */
  rows?: readonly IxPatternRow[] | null;
  creatorWallet?: string | null;
  /**
   * Wallet-contagion override. Default: on for `'tagged'`, off for dump/working
   * (those groups have no wallet rule). A flow lens supplies this explicitly.
   */
  contagion?: boolean;
  /**
   * Whether the creator wallet seeds the contagion set. Default follows
   * `'tagged'`. Staging surfaces pass the fingerprint's `creator_is_tagged`.
   */
  seedCreator?: boolean;
  excludeWallets?: ReadonlySet<string> | null;
  side?: FlowSide | null;
}

/** Build classify options for the selected tape list, or `null` when nothing
 *  can classify (empty keys and no creator-contagion fallback). */
export function classifyOptsForTape(input: TapeClassifyInput): FlowClassifyOptions | null {
  const list = input.list;
  const keys = input.keys;
  const hasKeys = keys != null && keys.size > 0;
  const contagion = input.contagion ?? list === 'tagged';
  const seedCreator = input.seedCreator ?? list === 'tagged';
  const creatorWallet = seedCreator ? (input.creatorWallet ?? null) : null;

  if (!hasKeys && (!contagion || !creatorWallet)) return null;

  const rows =
    list === 'working' ? null : input.rows && input.rows.length > 0 ? input.rows : null;

  return {
    patternKeys: keys ?? new Set<string>(),
    patternRows: rows,
    match: list === 'working' ? 'grain' : 'labels',
    creatorWallet,
    contagion,
    excludeWallets: input.excludeWallets ?? null,
    side: input.side ?? null,
  };
}

/** Overlay key set from a staging draft. Working keys are grain ids; tagged/dump
 *  keys are `JSON.stringify(labels)`. */
export function keysForTapeDraft(
  list: TapeList,
  rows: readonly IxPatternRow[],
  workingTemplates: readonly string[],
): ReadonlySet<string> | null {
  if (list === 'working') {
    return workingTemplates.length > 0 ? new Set(workingTemplates) : null;
  }
  const keys = patternKeysFrom(rows.map((r) => r.labels));
  return keys.size > 0 ? keys : null;
}
