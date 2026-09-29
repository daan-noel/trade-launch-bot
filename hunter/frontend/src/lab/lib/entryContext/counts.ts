/**
 * The Entry Context summary: two rows (Target signal, and All entries)
 * by two columns (buys, tokens). Each cell is his / matched — how many sit in
 * that row, and how many of those pass the idea (`EntryLogic.idea`, the buys
 * table's filters).
 */

import type { EntryLogic } from './logic';
import type { EntryRow } from './types';

export interface CohortCell {
  /** Buys or tokens in this row. */
  his: number;
  /** Of those, the ones the idea holds for. */
  matched: number;
}

export interface CohortSide {
  buys: CohortCell;
  tokens: CohortCell;
}

export interface CohortTable {
  /** The probe found the target in the slots before the buy. */
  target: CohortSide;
  /** Every readable buy in scope, whichever structure signaled. */
  all: CohortSide;
}

const cell = (his: number, matched: number): CohortCell => ({ his, matched });

/** Distinct tokens, and how many of them have at least one buy the idea holds for. */
function tokens(buys: readonly EntryRow[], idea: (e: EntryRow) => boolean): { his: number; matched: number } {
  const hit = new Map<string, boolean>();
  for (const e of buys) hit.set(e.mint_address, (hit.get(e.mint_address) ?? false) || idea(e));
  let matched = 0;
  for (const v of hit.values()) if (v) matched += 1;
  return { his: hit.size, matched };
}

/**
 * `signaled` is the probe's own verdict (the target's transaction landed in the
 * probe window). Probe-column filters (`logic.probe`) tighten that signal; they
 * are not part of the idea.
 */
export function cohortTable(
  entries: readonly EntryRow[],
  logic: EntryLogic,
  signaled: (e: EntryRow) => boolean,
  probeOn: boolean,
): CohortTable {
  const signal = (e: EntryRow) => probeOn && signaled(e) && logic.probe(e);
  const his = entries.filter((e) => !e.unknown_reason && logic.inScope(e));
  const targetBuys = his.filter(signal);
  const hisTokens = tokens(his, logic.idea);
  const targetTokens = tokens(targetBuys, logic.idea);

  return {
    target: {
      buys: cell(targetBuys.length, targetBuys.filter(logic.idea).length),
      tokens: cell(targetTokens.his, targetTokens.matched),
    },
    all: {
      buys: cell(his.length, his.filter(logic.idea).length),
      tokens: cell(hisTokens.his, hisTokens.matched),
    },
  };
}
