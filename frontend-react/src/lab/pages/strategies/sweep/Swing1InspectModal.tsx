import { useEffect, useMemo, useState } from 'react';
import {
  TokenInspectModal,
  type InspectTarget,
} from 'components/tpsl2/TokenInspectModal';
import type { ChartSwingLeg, ChartSwingOverlay } from 'components/token-price-chart';
import { fetchSwing1Detect, type Swing1DetectParams } from '@lab/services/swing1Detect';

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
 * Runs the SAME funnel the combo sim ran (curve-only, full history) and draws
 * the legs exactly like the swing1 detect page: one isolated `perLeg` segment
 * spanning each leg's full `start_at`→`end_at` (`perLegFullSpanEnd`). A
 * `connected` path here bridged the idle gaps between legs with misleading
 * diagonals — that was the wrong graph.
 */
export function Swing1InspectModal({ target, params, onClose }: Swing1InspectModalProps) {
  const [legs, setLegs] = useState<ChartSwingLeg[] | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLegs(null);
    fetchSwing1Detect(target.mint, params as Swing1DetectParams, {
      startMs: null,
      endMs: null,
      curveOnly: true,
    })
      .then((res) => {
        if (!cancelled) setLegs(res.legs as unknown as ChartSwingLeg[]);
      })
      .catch(() => {
        // Overlay is best-effort: on failure the modal still shows entry/exit.
        if (!cancelled) setLegs(null);
      });
    return () => {
      cancelled = true;
    };
  }, [target.mint, params]);

  const swingOverlay = useMemo<ChartSwingOverlay | null>(() => {
    if (!legs || !legs.length) return null;
    return { legs, segmentMode: 'perLeg' as const, perLegFullSpanEnd: true };
  }, [legs]);

  return <TokenInspectModal target={target} swingOverlay={swingOverlay} onClose={onClose} />;
}
