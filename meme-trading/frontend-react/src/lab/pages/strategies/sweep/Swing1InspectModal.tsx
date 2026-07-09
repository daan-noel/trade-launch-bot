import {
  TokenInspectModal,
  type InspectTarget,
} from 'components/tpsl2/TokenInspectModal';
import type { Swing1DetectParams } from '@lab/services/swing1Detect';
import { useSwing1DetectOverlay } from '@lab/hooks/useSwing1DetectOverlay';

interface Swing1InspectModalProps {
  target: InspectTarget;
  /** The drilled-in combo's swept knob values (sweep keys, no `p_` prefix).
   *  Every key matches a {@link Swing1DetectParams} field, so the chart's swing
   *  legs are the exact ones this combo's sim detected. */
  params: Record<string, number | null>;
  onClose: () => void;
}

/**
 * Lab wrapper around the shared {@link TokenInspectModal} that adds the swing1
 * leg overlay. Owns the detect fetch so the shared modal stays mode-agnostic.
 * Runs the SAME funnel the combo sim ran (all trades, full history — the
 * backtest's `find_by_mints_all` has no venue filter) and draws the legs
 * exactly like the swing1 detect page: one isolated `perLeg` segment spanning
 * each leg's full `start_at`→`end_at` (`perLegFullSpanEnd`). A `connected` path
 * here bridged the idle gaps between legs with misleading diagonals — that was
 * the wrong graph.
 */
export function Swing1InspectModal({ target, params, onClose }: Swing1InspectModalProps) {
  const swingOverlay = useSwing1DetectOverlay(target.mint_address, params as Swing1DetectParams);
  return <TokenInspectModal target={target} swingOverlay={swingOverlay} onClose={onClose} />;
}
