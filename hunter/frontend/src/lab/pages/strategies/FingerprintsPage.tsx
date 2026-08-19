import { FingerprintsView } from 'components/strategy/FingerprintsView';
import { useFingerprintMatchesFor } from '@lab/components/strategy/useFingerprintMatches';

/** Fingerprint library page (lab app) — with the lab-only per-row deep-link
 *  into Flow discovery and the per-row matched-tokens dashboard (creation
 *  heatmap + trend + token table), both scoped to that fingerprint. */
export function FingerprintsPage() {
  const matches = useFingerprintMatchesFor();
  return (
    <>
      <FingerprintsView linkToFlowDiscovery onViewMatches={matches.open} />
      {matches.modal}
    </>
  );
}
