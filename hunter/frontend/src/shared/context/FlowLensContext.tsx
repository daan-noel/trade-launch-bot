import { createContext, useContext } from 'react';

import type { FlowTag } from 'lib/flow/classifyFlow';
import type { IxPattern, IxPatternSetKind } from 'lib/flow/ixPatternSets';
import type { TagStage } from 'hooks/useIxPatternTarget';

/**
 * The **flow lens** a page puts every chart under: an analysis-owned pattern set,
 * read as a tag, that classifies `@set` / `@!set` in place of a fingerprint tag.
 *
 * A wallet study has no fingerprint, so Trader Analysis owns its set instead
 * (`ix_pattern_sets`) and provides it here. What cannot travel as props without
 * threading five component layers is the tag itself, the exclusions and the write
 * target a badge click lands on.
 *
 * Absent (every other page) ⇒ the chart stack classifies with the host fingerprint's
 * tag and badge clicks write to that fingerprint.
 */
export interface FlowLensTarget extends TagStage {
  kind: IxPatternSetKind;
  /** The whole stored set - exact rows (empty on a templates set). */
  patterns: IxPattern[];
  /** The whole stored set - grain ids / programs (empty on an exact set). */
  workingTemplates: string[];
  /** Group a newly clicked exact shape is filed under (`null` ⇒ ungrouped). */
  activeGroup: string | null;
}

export interface FlowLensValue {
  /** The narrowed set read as a tag (its `sticky` and `side` are the lens'
   *  switches). `null` ⇒ nothing classifies. */
  tag: FlowTag | null;
  /** Wallets that always read as the rest (the studied trader itself). */
  excludeWallets: ReadonlySet<string> | null;
  /** `null` ⇒ read-only lens (no set picked). */
  target: FlowLensTarget | null;
}

const FlowLensContext = createContext<FlowLensValue | null>(null);

export const FlowLensProvider = FlowLensContext.Provider;

/** The active lens, or `null` where no page provides one. */
export function useFlowLensContext(): FlowLensValue | null {
  return useContext(FlowLensContext);
}
