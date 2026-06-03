import { Link, NavLink, useLocation } from 'react-router-dom';
import { useEffect, useState } from 'react';
import { StatusButton } from '../ui/StatusButton';
import { NavDropdown } from '../ui/NavDropdown';
import { fetchLiveMode, fetchSolPrice, setLiveMode } from '../../services/api';
import { usePriceUnit } from '../../context/PriceUnitContext';
import { cn } from '../../lib/cn';

function NavItem({ to, children }: { to: string; children: React.ReactNode }) {
  return (
    <NavLink
      to={to}
      end={to === '/'}
      className={({ isActive }) =>
        cn(
          'rounded-md px-3 py-1.5 text-[13px] font-medium transition-all duration-150',
          isActive
            ? 'bg-primary/12 text-primary shadow-[inset_0_1px_0_rgba(19,206,175,0.15)]'
            : 'text-text-mid hover:bg-white/4 hover:text-text',
        )
      }
    >
      {children}
    </NavLink>
  );
}

export function Header() {
  const location = useLocation();
  const analysisActive = location.pathname.startsWith('/analysis');
  const strategiesActive = location.pathname.startsWith('/strategies');
  const [liveMode, setLiveModeState] = useState(false);
  const { unit, usdRate, setUnit, setUsdRate } = usePriceUnit();

  useEffect(() => {
    fetchLiveMode().then(setLiveModeState).catch(() => {});
    fetchSolPrice().then(setUsdRate).catch(() => {});
  }, [setUsdRate]);

  const toggleLive = async () => {
    try {
      const next = await setLiveMode(!liveMode);
      setLiveModeState(next);
    } catch {
      /* ignore */
    }
  };

  const toggleUnit = async () => {
    const next = unit === 'SOL' ? 'USD' : 'SOL';
    setUnit(next);
    if (next === 'USD') {
      try {
        const rate = await fetchSolPrice();
        setUsdRate(rate);
      } catch {
        /* ignore */
      }
    }
  };

  return (
    <header className="sticky top-0 z-100 border-b border-white/6 bg-bg/75 backdrop-blur-xl backdrop-saturate-150">
      <div className="flex h-14 items-center gap-5 px-5">
        <Link
          to="/"
          className="group flex shrink-0 items-center gap-2.5 transition-opacity hover:opacity-90"
        >
          <span className="flex size-8 items-center justify-center rounded-lg bg-primary/10 text-sm text-primary ring-1 ring-primary/20 transition-colors group-hover:bg-primary/15">
            ◈
          </span>
          <span className="flex flex-col leading-tight">
            <span className="text-sm font-semibold tracking-tight text-text">Meme Trading</span>
            <span className="text-[10px] font-medium tracking-wide text-text-dim uppercase">
              Solana Bot
            </span>
          </span>
        </Link>

        <div className="hidden h-5 w-px shrink-0 bg-white/8 md:block" aria-hidden />

        <nav className="flex min-w-0 flex-1 items-center gap-0.5 overflow-visible rounded-lg border border-white/6 bg-white/3 p-1">
          <NavItem to="/">Home</NavItem>
          <NavItem to="/dashboard">Dashboard</NavItem>
          <NavItem to="/tokens">Tokens</NavItem>
          <NavItem to="/transactions">Transactions</NavItem>
          <NavDropdown
            label="Analysis"
            isActive={analysisActive}
            items={[
              { to: '/analysis/general', label: 'General' },
              { to: '/analysis/peak-trough', label: 'Peak / Trough' },
            ]}
          />
          <NavItem to="/wallet">Wallet</NavItem>
          <NavDropdown
            label="Strategies"
            isActive={strategiesActive}
            items={[{ to: '/strategies/tpsl', label: 'TP / SL Rules' }]}
          />
          <NavItem to="/settings">Settings</NavItem>
        </nav>

        <div className="ml-auto flex shrink-0 items-center gap-2.5">
          <div className="flex items-center gap-2">
            <div className="flex rounded-lg border border-white/6 bg-white/3 p-0.5">
              <button
                type="button"
                onClick={() => unit !== 'SOL' && toggleUnit()}
                className={cn(
                  'rounded-md px-2.5 py-1 text-[11px] font-semibold tracking-wide transition-all duration-150',
                  unit === 'SOL'
                    ? 'bg-primary/12 text-primary shadow-sm'
                    : 'text-text-dim hover:text-text',
                )}
              >
                SOL
              </button>
              <button
                type="button"
                onClick={() => unit !== 'USD' && toggleUnit()}
                className={cn(
                  'rounded-md px-2.5 py-1 text-[11px] font-semibold tracking-wide transition-all duration-150',
                  unit === 'USD'
                    ? 'bg-secondary/12 text-secondary shadow-sm'
                    : 'text-text-dim hover:text-text',
                )}
              >
                USD
              </button>
            </div>
            {unit === 'USD' && (
              <span className="hidden text-[11px] tabular-nums text-text-dim lg:inline">
                {usdRate != null ? `$${usdRate.toFixed(2)}` : '—'}
              </span>
            )}
          </div>

          <div className="hidden h-5 w-px bg-white/8 sm:block" aria-hidden />

          <StatusButton
            state={liveMode ? 'live' : 'dead'}
            label={liveMode ? 'WS LIVE' : 'WS DEAD'}
            onClick={toggleLive}
          />
        </div>
      </div>
    </header>
  );
}
