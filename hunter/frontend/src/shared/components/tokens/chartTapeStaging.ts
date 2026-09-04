import type { TapeList, WorkingWrite } from 'hooks/useIxPatternTarget';
import type { IxPatternFee, IxPatternFeeMask, IxPatternRow } from 'lib/strategy/ixPatternRows';

/**
 * Staging tape (Flow Discovery). Overlay, badges and clicks all read this
 * draft instead of persisting to a fingerprint. The cart owns Apply.
 *
 * Lives in its own module so a route can type the tape without statically
 * importing `TokenTradeChart` (that file pulls `lightweight-charts`).
 */
export interface ChartTapeStaging {
  list: TapeList;
  setList: (list: TapeList) => void;
  rows: IxPatternRow[];
  workingTemplates: string[];
  workingWrite?: WorkingWrite;
  setWorkingWrite?: (write: WorkingWrite) => void;
  keys: ReadonlySet<string> | null;
  feePins: IxPatternFeeMask;
  setFeePins: (mask: IxPatternFeeMask) => void;
  toggle: (labels: readonly string[], fee?: IxPatternFee) => void;
  contagion?: boolean;
  seedCreator?: boolean;
}
