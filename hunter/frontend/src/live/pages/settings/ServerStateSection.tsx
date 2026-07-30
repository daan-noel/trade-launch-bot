import { useState } from 'react';

import { Button } from 'components/ui/Button';
import { RefreshIcon, SpinnerIcon } from 'components/ui/icons';
import { apiErrorMessage } from 'store/baseApi';
import { useReloadCachesMutation } from '@live/store/liveEndpoints';
import {
  SettingsPanel,
  SettingsPanelIntro,
  SettingsRows,
} from 'pages/settings/SettingsPrimitives';
import { cn } from 'lib/cn';

interface ReloadStep {
  name: string;
  ok: boolean;
  detail?: string | null;
}

interface ReloadCachesBody {
  ok: boolean;
  steps: ReloadStep[];
}

/**
 * Live-only admin control: re-read PG-backed server caches (settings, engine,
 * token seed, portfolio TTL caches) without restarting the process.
 */
export function ServerStateSection() {
  const [reloadCaches, { isLoading }] = useReloadCachesMutation();
  const [err, setErr] = useState<string | null>(null);
  const [result, setResult] = useState<ReloadCachesBody | null>(null);

  const run = async () => {
    if (
      !window.confirm(
        'Reload DB-backed server caches? This re-reads settings, strategy rules, token seed data, and position adopt state. In-flight trades are not interrupted.',
      )
    ) {
      return;
    }
    setErr(null);
    setResult(null);
    try {
      const body = await reloadCaches().unwrap();
      setResult(body);
    } catch (e) {
      const data = (e as { data?: ReloadCachesBody })?.data;
      if (data?.steps) {
        setResult(data);
      }
      setErr(apiErrorMessage(e as never) ?? 'Reload failed');
    }
  };

  return (
    <SettingsPanel className="lg:col-span-2">
      <SettingsPanelIntro
        title="Server state"
        description="Re-seed in-memory caches from Postgres when DB edits or a failed engine reload left the running process stale."
        tip={{
          body:
            'Reloads app settings, strategy engine rules + PG position adopt, token cache seed (merge, not wipe), AMM pool facts for held migrated mints, wallet interning cache, and portfolio TTL caches. Does not rebuild armed/pre-entry state or chain caches (blockhash, reserves) — restart the container for a full cold boot.',
        }}
      />
      <SettingsRows>
        <div className="flex flex-col gap-3 py-3">
          <div className="flex flex-wrap items-center gap-2">
            <Button
              variant="ghost"
              size="sm"
              disabled={isLoading}
              onClick={() => void run()}
            >
              {isLoading ? (
                <SpinnerIcon className="mr-1.5 size-4" />
              ) : (
                <RefreshIcon className="mr-1.5 size-4" />
              )}
              Reload caches from DB
            </Button>
            {result?.ok && (
              <span className="text-xs font-medium text-green">All steps OK</span>
            )}
            {result && !result.ok && (
              <span className="text-xs font-medium text-red">Completed with errors</span>
            )}
          </div>
          {err && !result?.steps && (
            <p className="text-xs text-red">{err}</p>
          )}
          {result?.steps && result.steps.length > 0 && (
            <ul className="flex flex-col gap-1 rounded-md border border-white/6 bg-bg-card/50 p-2 text-[11px]">
              {result.steps.map((step) => (
                <li
                  key={step.name}
                  className={cn(
                    'flex flex-wrap items-baseline gap-x-2 gap-y-0.5',
                    step.ok ? 'text-text-dim' : 'text-red',
                  )}
                >
                  <span className="font-mono font-semibold uppercase tracking-wide">
                    {step.ok ? 'OK' : 'FAIL'}
                  </span>
                  <span className="font-medium text-text">{step.name}</span>
                  {step.detail && (
                    <span className="text-text-dim">{step.detail}</span>
                  )}
                </li>
              ))}
            </ul>
          )}
        </div>
      </SettingsRows>
    </SettingsPanel>
  );
}
