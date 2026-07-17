import { StatusButton } from 'components/ui/StatusButton';
import { useGetLiveModeQuery, useSetLiveModeMutation } from '@live/store/liveEndpoints';

/**
 * Live-only LIVE/DEAD kill switch for the header right slot. Owns the
 * live-mode query + mutation so the lab build (which has no ingest/trading
 * to switch) never imports them.
 */
export function LiveModeControl() {
  const { data: liveMode = false } = useGetLiveModeQuery();
  const [setLiveMode] = useSetLiveModeMutation();

  const toggleLive = async () => {
    try {
      await setLiveMode(!liveMode).unwrap();
    } catch {
      /* ignore */
    }
  };

  return (
    <>
      <div className="hidden h-5 w-px bg-white/8 sm:block" aria-hidden />
      <StatusButton
        state={liveMode ? 'live' : 'dead'}
        label={liveMode ? 'Helius / LIVE' : 'Helius / DEAD'}
        onClick={toggleLive}
        className={liveMode ? 'animate-pulse' : undefined}
      />
    </>
  );
}
