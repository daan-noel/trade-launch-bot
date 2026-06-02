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

function ChevronDown({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 12 12"
      fill="none"
      aria-hidden
      className={cn('size-3 opacity-50', className)}
    >
      <path
        d="M3 4.5 6 7.5 9 4.5"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
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

        <nav className="flex min-w-0 flex-1 items-center gap-0.5 overflow-x-auto rounded-lg border border-white/6 bg-white/3 p-1 scrollbar-none [&::-webkit-scrollbar]:hidden">
          <NavItem to="/">Home</NavItem>
          <NavItem to="/dashboard">Dashboard</NavItem>
          <NavItem to="/tokens">Tokens</NavItem>
          <NavItem to="/transactions">Transactions</NavItem>
          <NavItem to="/analysis">Analysis</NavItem>
          <NavItem to="/wallet">Wallet</NavItem>

          <div className="group relative shrink-0">
            <NavLink
              to="/strategies/tpsl"
              className={({ isActive }) =>
                cn(
                  'flex items-center gap-1 rounded-md px-3 py-1.5 text-[13px] font-medium transition-all duration-150',
                  isActive
                    ? 'bg-primary/12 text-primary shadow-[inset_0_1px_0_rgba(19,206,175,0.15)]'
                    : 'text-text-mid hover:bg-white/4 hover:text-text',
                )
              }
            >
              Strategies
              <ChevronDown className="transition-transform group-hover:translate-y-px" />
            </NavLink>
            <div className="invisible absolute left-0 top-[calc(100%+8px)] z-10 flex min-w-[132px] translate-y-1 flex-col gap-0.5 rounded-lg border border-white/8 bg-bg-panel/95 p-1 opacity-0 shadow-[0_12px_32px_rgba(0,0,0,0.45)] backdrop-blur-xl transition-all duration-150 group-hover:visible group-hover:translate-y-0 group-hover:opacity-100 group-focus-within:visible group-focus-within:translate-y-0 group-focus-within:opacity-100">
              <NavLink
                to="/strategies/tpsl"
                className={({ isActive }) =>
                  cn(
                    'rounded-md px-3 py-2 text-xs font-medium text-text-mid whitespace-nowrap transition-colors hover:bg-white/6 hover:text-text',
                    isActive && 'bg-primary/10 text-primary',
                  )
                }
              >
                TP / SL Rules
              </NavLink>
            </div>
          </div>

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
