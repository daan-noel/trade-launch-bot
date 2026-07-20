import type { AppSettings } from 'services/api';
import {
  SettingsPanel,
  SettingsPanelIntro,
  SettingsRows,
  ToggleRow,
} from './SettingsPrimitives';

interface TrackingSectionProps {
  settings: AppSettings;
  saving: boolean;
  update: (patch: Partial<AppSettings>) => void;
}

export function TrackingSection({ settings, saving, update }: TrackingSectionProps) {
  return (
    <SettingsPanel>
      <SettingsPanelIntro
        title="Token tracking"
        description="What the live ingest pipeline records. Changes apply immediately."
      />
      <SettingsRows>
        <ToggleRow
          title="Track Mayhem-mode tokens"
          description="When off, new Mayhem tokens are skipped and existing ones drop from live tracking."
          tip="Historical data is kept. Only live ingest / tracking is affected."
          checked={settings.track_mayhem}
          disabled={saving}
          onChange={(track_mayhem) => update({ track_mayhem })}
        />
        <ToggleRow
          title="Track trade history after migration"
          description="When off, migrated tokens stop recording AMM trades and unsubscribe from their pools."
          tip="The migration event itself is still tracked."
          checked={settings.track_post_migration}
          disabled={saving}
          onChange={(track_post_migration) => update({ track_post_migration })}
        />
        <ToggleRow
          title="Persist raw transactions"
          description="When off, raw tx blobs are not written (curbs DB growth)."
          tip="Decoded trades and metrics are still recorded either way."
          checked={settings.persist_raw}
          disabled={saving}
          onChange={(persist_raw) => update({ persist_raw })}
        />
      </SettingsRows>
    </SettingsPanel>
  );
}
