import { SettingsPage } from 'pages/settings/SettingsPage';
import { ServerStateSection } from './ServerStateSection';

/** Live settings — standard panels plus admin server-state reseed. */
export function LiveSettingsPage() {
  return <SettingsPage extraPanel={<ServerStateSection />} />;
}
