import { createContext, useContext } from 'react';

import type { FlowSide } from 'lib/flow/classifyFlow';
import type { IxPattern } from 'lib/flow/ixPatternSets';

/**
 * The **flow lens** a page puts every chart under: which `ix_labels` pattern set
 * classifies vol/non-vol, and how.
 *
 * The chart stack normally classifies with a fingerprint's saved
 * `ix_patterns`, handed down as `flowPatternKeys` props. A wallet study
 * has no fingerprint, so Trader Analysis owns its set instead (`ix_pattern_sets`)
 * and provides it here. The KEYS still travel as props — that path already
 * exists and every card reads it. What cannot travel as props without threading
 * five component layers is the rest of the lens: the classifier options and the
 * write target a Vol badge click lands on. Those come through this context.
 *
 * Absent (every other page) ⇒ the chart stack behaves exactly as before: engine
 * contagion on, no exclusions, Vol badges writing to a fingerprint.
 */
export interface FlowLensTarget {
  /** Set name, for the strip above the trades table. */
  name: string;
  /** The FULL stored set — a toggle edits this, not the narrowed view. */
  patterns: IxPattern[];
  /** Group a newly toggled-in pattern is filed under (`null` ⇒ ungrouped). */
  activeGroup: string | null;
  /** Add/remove one ordered `ix_labels` sequence and persist it. */
  toggle: (labels: readonly string[]) => void;
  saving: boolean;
  error: string | null;
}

export interface FlowLensValue {
  /** Forward-only wallet tagging. Off for a structural-only read — see
   *  `FlowClassifyOptions.contagion`. */
  contagion: boolean;
  /** Wallets never classified as volume (the studied trader itself). */
  excludeWallets: ReadonlySet<string> | null;
  /** Classify one leg only — `null` ⇒ both, the engine's behavior. Patterns are
   *  side-blind, so this is the only way to tell a structure buying from the
   *  same structure selling. See `FlowClassifyOptions.side`. */
  side: FlowSide | null;
  /** `null` ⇒ read-only lens (no set picked, or nothing to write to). */
  target: FlowLensTarget | null;
}

const FlowLensContext = createContext<FlowLensValue | null>(null);

export const FlowLensProvider = FlowLensContext.Provider;

/** The active lens, or `null` where no page provides one. */
export function useFlowLensContext(): FlowLensValue | null {
  return useContext(FlowLensContext);
}
