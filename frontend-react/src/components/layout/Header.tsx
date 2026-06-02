import { NavLink, Link } from 'react-router-dom';
import { useEffect, useState } from 'react';
import { StatusButton } from '../ui/StatusButton';
import { fetchLiveMode, fetchSolPrice, setLiveMode } from '../../services/api';
import { usePriceUnit } from '../../context/PriceUnitContext';
import { cn } from '../../lib/cn';

function NavItem({ to, children }: { to: string; children: React.ReactNode }) {
  return (
    <NavLink
      to={to}
      className={({ isActive }) =>
        cn(
          'relative rounded-md px-3 py-1.5 text-[13px] font-medium text-text-mid transition-colors hover:bg-primary/8 hover:text-text',
          isActive && 'bg-primary/10 text-primary after:absolute after:bottom-[-1px] after:left-2 after:right-2 after:h-0.5 after:rounded-t after:bg-primary after:shadow-[0_0_8px_rgba(19,206,175,0.6)]',
        )
      }
    >
      {children}
    </NavLink>
  );
}

export function Header() {
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
    <header className="sticky top-0 z-[100] h-12 border-b-2 border-primary bg-bg/95 backdrop-blur-md">
      <div className="flex h-full items-center gap-8 px-5">
        <Link to="/" className="flex shrink-0 items-center gap-1.5">
          <span className="text-base text-primary">◈</span>
          <span className="text-sm font-bold tracking-wide text-primary">MEME</span>
          <span className="text-[11px] font-medium tracking-widest text-text-dim">TRADING</span>
        </Link>

        <nav className="flex items-center gap-0.5">
          <NavItem to="/">Home</NavItem>
          <NavItem to="/dashboard">Dashboard</NavItem>
          <NavItem to="/tokens">Tokens</NavItem>
          <NavItem to="/transactions">Transactions</NavItem>
          <NavItem to="/analysis">Analysis</NavItem>
          <NavItem to="/wallet">Wallet</NavItem>

          <div className="group relative">
            <NavLink
              to="/strategies/tpsl"
              className={({ isActive }) =>
                cn(
                  'flex items-center gap-0.5 rounded-md px-3 py-1.5 text-[13px] font-medium text-text-mid transition-colors hover:bg-primary/8 hover:text-text',
                  isActive && 'bg-primary/10 text-primary',
                )
              }
            >
              Strategies
              <span className="text-[9px] opacity-55">▾</span>
            </NavLink>
            <div className="absolute left-0 top-[calc(100%+6px)] hidden min-w-[120px] flex-col gap-0.5 rounded-lg border border-border bg-bg-panel p-1 shadow-[0_8px_24px_rgba(0,0,0,0.55)] group-hover:flex group-focus-within:flex">
              <NavLink
                to="/strategies/tpsl"
                className={({ isActive }) =>
                  cn(
                    'rounded-[5px] px-3 py-1.5 text-xs font-medium text-text-mid whitespace-nowrap transition-colors hover:bg-primary/8 hover:text-text',
                    isActive && 'bg-primary/10 font-bold text-primary',
                  )
                }
              >
                TPSL
              </NavLink>
            </div>
          </div>

          <NavItem to="/settings">Settings</NavItem>
        </nav>

        <div className="ml-auto flex items-center gap-3">
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={toggleUnit}
              className={cn(
                'flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-[11px] font-semibold tracking-wider transition-colors',
                unit === 'SOL'
                  ? 'border-primary text-text'
                  : 'border-secondary text-secondary',
              )}
            >
              <span
                className={cn(
                  'size-1.5 animate-pulse rounded-full',
                  unit === 'SOL'
                    ? 'bg-primary shadow-[0_0_6px_var(--color-primary)]'
                    : 'bg-secondary shadow-[0_0_6px_var(--color-secondary)]',
                )}
              />
              {unit}
            </button>
            {unit === 'USD' && (
              <span className="text-[11px] text-text-dim">
                {usdRate != null ? `SOL/USD ${usdRate.toFixed(2)}` : 'SOL/USD —'}
              </span>
            )}
          </div>
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
