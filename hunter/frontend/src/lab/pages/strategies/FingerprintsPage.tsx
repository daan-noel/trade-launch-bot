import { FingerprintsView } from 'components/strategy/FingerprintsView';

/** Fingerprint library page (lab app) — with the lab-only per-row deep-link
 *  into Flow discovery scoped to each fingerprint. */
export function FingerprintsPage() {
  return <FingerprintsView linkToFlowDiscovery />;
}
