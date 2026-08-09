import clsx from 'clsx';
import { useEffect, useMemo, useState } from 'react';
import {
  useWalletPoolQuery,
  useRefreshWalletBalancesMutation,
  useGenerateWalletsMutation,
  useFundPoolMutation,
  useTransferSolMutation,
  useSweepWalletsMutation,
  useConsolidateWalletsMutation,
  useRestoreFromKeystoreMutation,
  useExportWalletKeyMutation,
} from '@shared/store/endpoints';
import { apiErrorMessage } from '@shared/store/baseApi';
import {
  AgeCell,
  Banner,
  Button,
  Card,
  Column,
  DataTable,
  Field,
  FilterToggle,
  Icon,
  IconButton,
  Input,
  Select,
  StatusPill,
  RolePill,
  roleColorVar,
  statusTone,
  toneColorVar,
  AddressDisplay,
  usePinnedRows,
} from '@shared/components/ui';
import {
  connectWalletPoolStream,
  connectRestoreProgressStream,
  connectRestoreCompleteStream,
  connectActionProgressStream,
  type ActionProgress,
  type RestoreProgress,
  type RestoreSummary,
} from '@shared/services/sse';
import { api } from '@shared/store/endpoints';
import { useDispatch } from 'react-redux';
import { formatSol } from '@shared/lib/format';
import type {
  ManagedWalletPool,
  SweepReport,
  TransferReport,
  WalletRole,
  WalletStatus,
} from '@shared/types';

// A wallet in one of these statuses can't be an ad-hoc transfer source (a launch is
// mid-flight against it, or it's retired) — mirrors the backend status guard.
const NON_SOURCE_STATUSES = new Set<WalletStatus>(['funding', 'reserved', 'retired']);

const LOW_POOL_THRESHOLD = 3;
// Lamports per SOL — for the Balance column's numeric filter (displayed in SOL).
const LAMPORTS_PER_SOL = 1_000_000_000;
const ROLES: WalletRole[] = ['dev', 'bundler', 'treasury', 'trading'];
// Only these roles are funded treasury→pool (dev launches, bundlers co-buy). Treasury
// is the SOURCE and trading isn't part of the launch warm-pool, so the per-role/low-pool
// Fund actions are offered only for these — mirrors the backend's fundable-role set.
const FUNDABLE_ROLES: WalletRole[] = ['dev', 'bundler'];
const STATUSES: WalletStatus[] = ['generated', 'funding', 'funded', 'reserved', 'used', 'retired'];

// The steady balance poller only live-refreshes `generated`/`funding` (+ the treasury
// by role); every other status carries a FROZEN snapshot in `balance_lamports` (funded/
// reserved at their funded-era value; used/retired at whatever they last held). So a
// balance is only trustworthy — countable into the total — when it was checked recently.
// The poller runs every ≤30s, so anything older than this is a stale snapshot until the
// operator hits "Refresh balances" (which re-reads EVERY wallet on-chain and re-stamps
// `balance_checked_at`, flipping used/retired live so they fold into the exact total).
const BALANCE_STALE_AFTER_MS = 90_000;

function balanceAgeMs(w: ManagedWalletPool): number | null {
  if (!w.balance_checked_at) return null;
  return Date.now() - new Date(w.balance_checked_at).getTime();
}

function isBalanceLive(w: ManagedWalletPool): boolean {
  const age = balanceAgeMs(w);
  return age != null && age < BALANCE_STALE_AFTER_MS;
}

// Statuses whose on-chain balance is STABLE between checks: a `funded`/`reserved`
// wallet just holds its SOL until a launch consumes it (flipping it to `used`), so its
// last-read balance stays exact even after the steady poller — which only re-reads
// `generated`/`funding` + treasury — lets its `balance_checked_at` go stale. Without
// this, minutes after a manual refresh the total would collapse to ~treasury only.
const STABLE_BALANCE_STATUSES = new Set<WalletStatus>(['funded', 'reserved']);

// A retired wallet holding no SOL — swept, shredded, done. Hidden from the pool table
// by default (see the "Show retired (empty)" toggle) since it's just visual noise.
function isRetiredEmpty(w: ManagedWalletPool): boolean {
  return w.status === 'retired' && (w.balance_lamports ?? 0) === 0;
}

// A freshly-generated, not-yet-funded wallet. Hidden by default (see the "Show
// generated" toggle) so the table shows the live, funded pool by default.
function isGenerated(w: ManagedWalletPool): boolean {
  return w.status === 'generated';
}

// Terminal statuses — a wallet here is done being launched from. Its SOL still counts
// toward the pool total, but it's broken out under its own status chip (not the per-role
// chips) so the operator can see how much is parked in spent/shredded wallets awaiting a
// sweep, instead of it hiding inside e.g. the "dev" role figure.
const TERMINAL_STATUSES = new Set<WalletStatus>(['used', 'retired']);

// Whether a wallet's balance is trustworthy enough to fold into the holdings total:
// either freshly polled OR sitting in a stable status. `used`/`retired` may have spent
// SOL post-launch, so they stay stale-gated (excluded until a manual Refresh re-reads
// them). `isBalanceLive` remains the narrower "freshly read on-chain" freshness signal.
function isBalanceCountable(w: ManagedWalletPool): boolean {
  return isBalanceLive(w) || STABLE_BALANCE_STATUSES.has(w.status);
}

function formatAgo(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s}s ago`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m ago`;
  return `${Math.round(m / 60)}h ago`;
}

/**
 * At-a-glance card for one role: big funded count (red when below threshold),
 * a proportional status bar, and per-status chips. Clickable to filter the pool.
 */
function RoleSummaryCard({
  role,
  counts,
  total,
  active,
  onSelect,
  onFund,
  funding,
}: {
  role: string;
  counts: Record<string, number>;
  total: number;
  active: boolean;
  onSelect: () => void;
  // Present only for fundable roles (dev/bundler) — renders a scoped Fund action
  // that tops just this role up to its warm target, drawing from the treasury.
  onFund?: () => void;
  funding?: boolean;
}) {
  const funded = counts.funded ?? 0;
  const low = funded < LOW_POOL_THRESHOLD;
  const accent = roleColorVar(role);
  return (
    <div className="relative">
      <button
        type="button"
        onClick={onSelect}
        className={clsx(
          'panel w-full px-3.5 py-3 text-left transition-colors hover:border-[var(--color-line-2)]',
          // Reserve room so the absolutely-positioned Fund badge never overlaps the
          // status chips at the bottom of the card.
          onFund && 'pb-10',
        )}
        style={{
          borderLeft: `3px solid ${accent}`,
          outline: active ? `1px solid ${accent}` : undefined,
        }}
      >
      <div className="flex items-center justify-between">
        <RolePill role={role} />
        <span className="text-xs muted">{total} total</span>
      </div>

      <div className="mt-2 flex items-baseline gap-1.5">
        <span
          className="text-2xl font-semibold leading-none"
          style={{ color: low ? 'var(--color-bad)' : undefined }}
        >
          {funded}
        </span>
        <span className="text-xs muted">funded{low ? ` · low (<${LOW_POOL_THRESHOLD})` : ''}</span>
      </div>

      {total > 0 && (
        <div className="mt-2 flex h-1.5 overflow-hidden rounded-full bg-[var(--color-line)]">
          {STATUSES.map((s) => {
            const n = counts[s] ?? 0;
            if (n === 0) return null;
            return (
              <div
                key={s}
                title={`${s}: ${n}`}
                style={{ width: `${(n / total) * 100}%`, background: toneColorVar(statusTone(s)) }}
              />
            );
          })}
        </div>
      )}

      <div className="mt-2 flex flex-wrap gap-x-2.5 gap-y-0.5 text-xs">
        {STATUSES.filter((s) => (counts[s] ?? 0) > 0).map((s) => (
          <span key={s} className="muted">
            <span style={{ color: toneColorVar(statusTone(s)) }}>●</span> {counts[s]} {s}
          </span>
        ))}
      </div>
      </button>
      {onFund && (
        // Sibling of the filter button (not nested — that would be invalid HTML),
        // absolutely positioned over the card's bottom-right so clicking it funds
        // this role instead of toggling the filter.
        <button
          type="button"
          disabled={funding}
          onClick={onFund}
          title={`Fund ${role} wallets up to their warm target (from the treasury)`}
          className={clsx(
            'badge badge-neutral absolute bottom-2.5 right-2.5 cursor-pointer',
            funding && 'opacity-60',
          )}
        >
          <Icon name="coins" />
          Fund
        </button>
      )}
    </div>
  );
}

export function WalletPoolPage() {
  const [roleFilter, setRoleFilter] = useState('');
  const [genRole, setGenRole] = useState<WalletRole>('bundler');
  const [genCount, setGenCount] = useState(5);
  const [genLabel, setGenLabel] = useState('');
  const [msg, setMsg] = useState<string | null>(null);
  // Retired (drained) and freshly-`generated` (not-yet-funded) wallets are dead weight in
  // the table — hidden by default so the list reads as the live, funded pool; the operator
  // can toggle either group back in to audit/fund/export them.
  const [showRetiredEmpty, setShowRetiredEmpty] = useState(false);
  const [showGenerated, setShowGenerated] = useState(false);

  const {
    data: wallets = [],
    isFetching,
    error,
    refetch: refetchWallets,
  } = useWalletPoolQuery(roleFilter || undefined, {
    // The background funder PUSHES a `wallet_pool` event over SSE
    // when a pass moves SOL / promotes wallets, and the subscription below refetches
    // off it. This poll is demoted to a slow 30s gap-heal covering a missed push.
    pollingInterval: 30000,
    skipPollingIfUnfocused: true,
  });

  // SSE push: refetch the pool the instant a funding pass reports a change, so the
  // page reflects autonomous background funding without fast polling.
  useEffect(() => {
    const h = connectWalletPoolStream(() => refetchWallets());
    return () => h.close();
  }, [refetchWallets]);
  const [generate, gen] = useGenerateWalletsMutation();
  const [fund, funding] = useFundPoolMutation();
  const [refreshBalances, refreshing] = useRefreshWalletBalancesMutation();

  // Keystore restore (recovery on a fresh box): kick the background job, then track
  // its SSE progress + final summary. `restoreOpen` gates the confirm dialog.
  const [restore, restoring] = useRestoreFromKeystoreMutation();
  const [restoreOpen, setRestoreOpen] = useState(false);
  const [restoreProgress, setRestoreProgress] = useState<RestoreProgress | null>(null);
  const [restoreSummary, setRestoreSummary] = useState<RestoreSummary | null>(null);
  const [restoreRunning, setRestoreRunning] = useState(false);

  // Subscribe to restore progress/complete for as long as the page is mounted, so a
  // long backfill keeps updating even if the operator navigates away and back.
  useEffect(() => {
    const p = connectRestoreProgressStream((prog) => {
      setRestoreRunning(true);
      setRestoreSummary(null);
      setRestoreProgress(prog);
    });
    const c = connectRestoreCompleteStream((summary) => {
      setRestoreRunning(false);
      setRestoreProgress(null);
      setRestoreSummary(summary);
      refetchWallets();
    });
    return () => {
      p.close();
      c.close();
    };
  }, [refetchWallets]);

  const onRestore = async () => {
    setMsg(null);
    setRestoreSummary(null);
    setRestoreOpen(false);
    try {
      setRestoreRunning(true);
      await restore().unwrap();
    } catch {
      setRestoreRunning(false);
      /* surfaced via mutation error below */
    }
  };

  // Operator sweep passes — fire-and-forget 202; live N/M over action_progress.
  const [sweep, sweeping] = useSweepWalletsMutation();
  const [consolidate, consolidating] = useConsolidateWalletsMutation();
  const [sweepResult, setSweepResult] = useState<{ title: string; report: SweepReport } | null>(
    null,
  );
  const [poolAction, setPoolAction] = useState<ActionProgress | null>(null);
  const dispatch = useDispatch();

  useEffect(() => {
    const h = connectActionProgressStream((p) => {
      if (p.kind !== 'sweep' && p.kind !== 'consolidate') return;
      // Wallet-wide frames carry null mint.
      if (p.mint_address) return;
      setPoolAction(p.status === 'running' ? p : null);
      if (p.status === 'done' || p.status === 'partial' || p.status === 'failed') {
        dispatch(api.util.invalidateTags(['Wallets']));
        setMsg(
          p.status === 'done'
            ? `${p.kind} finished — ${p.done}/${p.total} wallet(s).`
            : `${p.kind} ${p.status} — ${p.done}/${p.total}${p.error ? `: ${p.error}` : ''}`,
        );
      }
    });
    return () => h.close();
  }, [dispatch]);

  // "Consolidate → treasury" dialog: pick the destination treasury, then confirm.
  const [consolidateOpen, setConsolidateOpen] = useState(false);
  const [consolidateDest, setConsolidateDest] = useState('');
  const [consolidateErr, setConsolidateErr] = useState<string | null>(null);

  // Treasury wallets for the consolidate destination picker — queried unfiltered so
  // they're always available even when the pool table is scoped to another role.
  const { data: treasuryWallets = [] } = useWalletPoolQuery('treasury');
  const treasuryDestinations = useMemo(
    () => treasuryWallets.filter((w) => w.status !== 'retired'),
    [treasuryWallets],
  );

  // Wallet-to-wallet transfer dialog, keyed by the source wallet.
  const [transferTarget, setTransferTarget] = useState<ManagedWalletPool | null>(null);
  const [transferDest, setTransferDest] = useState('');
  const [transferAmount, setTransferAmount] = useState('');
  const [transferMax, setTransferMax] = useState(false);
  const [transferResult, setTransferResult] = useState<TransferReport | null>(null);
  const [transferErr, setTransferErr] = useState<string | null>(null);
  const [transferSol, transferring] = useTransferSolMutation();

  // Private-key export dialog. The secret + revealed key live ONLY in local
  // state (never Redux/RTK cache) and are wiped when the dialog closes.
  const [exportTarget, setExportTarget] = useState<ManagedWalletPool | null>(null);
  const [exportSecret, setExportSecret] = useState('');
  const [revealedKey, setRevealedKey] = useState<string | null>(null);
  const [exportErr, setExportErr] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [exportKey, exporting] = useExportWalletKeyMutation();

  const closeExport = () => {
    setExportTarget(null);
    setExportSecret('');
    setRevealedKey(null);
    setExportErr(null);
    setCopied(false);
  };

  const onExport = async () => {
    if (!exportTarget || !exportSecret) return;
    setExportErr(null);
    try {
      const res = await exportKey({ id: exportTarget.id, secret: exportSecret }).unwrap();
      setRevealedKey(res.private_key_base58);
    } catch (e) {
      setRevealedKey(null);
      setExportErr(apiErrorMessage(e as Parameters<typeof apiErrorMessage>[0]) ?? 'Export failed.');
    }
  };

  const onCopyKey = async () => {
    if (!revealedKey) return;
    await navigator.clipboard.writeText(revealedKey);
    setCopied(true);
  };

  const openTransfer = (w: ManagedWalletPool) => {
    setTransferTarget(w);
    setTransferDest('');
    setTransferAmount('');
    setTransferMax(false);
    setTransferResult(null);
    setTransferErr(null);
  };

  const closeTransfer = () => {
    setTransferTarget(null);
    setTransferDest('');
    setTransferAmount('');
    setTransferMax(false);
    setTransferResult(null);
    setTransferErr(null);
  };

  // Candidate destinations for the open transfer: any other wallet that isn't
  // retired (topping up a shredded wallet is pointless).
  const transferDestinations = useMemo(
    () =>
      transferTarget
        ? wallets.filter((w) => w.id !== transferTarget.id && w.status !== 'retired')
        : [],
    [wallets, transferTarget],
  );

  const onTransfer = async () => {
    if (!transferTarget || !transferDest) return;
    if (!transferMax && !(Number(transferAmount) > 0)) return;
    setTransferErr(null);
    try {
      const res = await transferSol({
        from_id: transferTarget.id,
        to_id: transferDest,
        ...(transferMax ? { max: true } : { amount_sol: Number(transferAmount) }),
      }).unwrap();
      setTransferResult(res);
    } catch (e) {
      setTransferResult(null);
      setTransferErr(apiErrorMessage(e as Parameters<typeof apiErrorMessage>[0]) ?? 'Transfer failed.');
    }
  };

  // Exact SOL the pool holds, read at a glance instead of adding up every row by hand.
  // The headline number is the COMPLETE on-chain total — EVERY wallet's balance,
  // including `used`/`retired` whose snapshot the steady poller lets go stale. It never
  // collapses to "just the live roles" after 90s: the total always reflects the full
  // pool, and the per-role breakdown shows where the SOL sits. We still track how much of
  // that total is a stale snapshot (`used`/`retired` not live-re-read, which may have
  // spent SOL post-launch) so the operator knows a slice isn't freshly confirmed — a
  // "Refresh balances" pass re-reads every wallet and makes the whole figure exact.
  // Respects the active role filter.
  const holdings = useMemo(() => {
    let totalLamports = 0;
    let walletCount = 0;
    let snapshotLamports = 0;
    let snapshotCount = 0;
    let oldestCheck: number | null = null;
    const byRole: Record<string, number> = {};
    const byTerminalStatus: Record<string, number> = {};
    for (const w of wallets) {
      const bal = w.balance_lamports ?? 0;
      totalLamports += bal;
      walletCount += 1;
      // Terminal wallets (used/retired) break out by status; the rest by role — together
      // they partition the total with no double-counting.
      if (TERMINAL_STATUSES.has(w.status)) {
        byTerminalStatus[w.status] = (byTerminalStatus[w.status] ?? 0) + bal;
      } else {
        byRole[w.role] = (byRole[w.role] ?? 0) + bal;
      }
      if (w.balance_checked_at) {
        const t = new Date(w.balance_checked_at).getTime();
        if (oldestCheck == null || t < oldestCheck) oldestCheck = t;
      }
      // Stale used/retired snapshots are still summed into the total, but flagged so the
      // freshness note can say how much of it hasn't been live-confirmed recently.
      if (!isBalanceCountable(w)) {
        snapshotLamports += bal;
        snapshotCount += 1;
      }
    }
    return {
      totalLamports,
      walletCount,
      snapshotLamports,
      snapshotCount,
      oldestCheck,
      byRole,
      byTerminalStatus,
    };
  }, [wallets]);

  const counts = useMemo(() => {
    const c: Record<string, Record<string, number>> = {};
    for (const role of ROLES) c[role] = Object.fromEntries(STATUSES.map((s) => [s, 0]));
    for (const w of wallets) {
      if (!c[w.role]) c[w.role] = Object.fromEntries(STATUSES.map((s) => [s, 0]));
      c[w.role][w.status] = (c[w.role][w.status] ?? 0) + 1;
    }
    return c;
  }, [wallets]);

  // Cluster the flat pool by role (in canonical ROLES order) so same-role wallets
  // read as a group even without filtering; status/age keep a stable sub-order.
  // These groups are hidden unless the operator toggles them on.
  const hiddenRetiredCount = useMemo(
    () => wallets.filter(isRetiredEmpty).length,
    [wallets],
  );
  const hiddenGeneratedCount = useMemo(() => wallets.filter(isGenerated).length, [wallets]);

  const sortedWallets = useMemo(() => {
    const roleIdx = (r: string) => {
      const i = ROLES.indexOf(r as WalletRole);
      return i === -1 ? ROLES.length : i;
    };
    const visible = wallets.filter(
      (w) =>
        (showRetiredEmpty || !isRetiredEmpty(w)) && (showGenerated || !isGenerated(w)),
    );
    return [...visible].sort(
      (a, b) =>
        roleIdx(a.role) - roleIdx(b.role) ||
        STATUSES.indexOf(a.status) - STATUSES.indexOf(b.status) ||
        (a.label ?? '').localeCompare(b.label ?? ''),
    );
  }, [wallets, showRetiredEmpty, showGenerated]);

  const pinning = usePinnedRows('wallet-pool', (w: ManagedWalletPool) => w.id, sortedWallets);

  const lowPoolRoles = Object.entries(counts).filter(
    ([, s]) => (s.funded ?? 0) < LOW_POOL_THRESHOLD,
  );
  // The low roles the treasury pass can actually top up (dev/bundler). The banner
  // Fund button scopes to exactly these, so it never spends on a role that isn't
  // fundable — and never runs a redundant all-roles pass when only one is short.
  const lowFundableRoles = lowPoolRoles
    .map(([role]) => role)
    .filter((role) => FUNDABLE_ROLES.includes(role as WalletRole));

  const onGenerate = async () => {
    if (genCount < 1) return;
    setMsg(null);
    try {
      await generate({ role: genRole, count: genCount, label_prefix: genLabel || undefined }).unwrap();
      setGenLabel('');
    } catch {
      /* surfaced via mutation error below */
    }
  };

  // Live-read every wallet's on-chain balance (respecting the active role filter) so
  // the holdings total becomes exact, incl. used/retired the poller leaves frozen.
  const onRefreshBalances = async () => {
    setMsg(null);
    try {
      const rows = await refreshBalances(roleFilter || undefined).unwrap();
      setMsg(`Balances refreshed — ${rows.length} wallet${rows.length === 1 ? '' : 's'} read on-chain.`);
    } catch {
      /* surfaced via mutation error below */
    }
  };

  // Treasury → pool funding. `roles` undefined = one pass over all fundable roles
  // (the header "Fund pool" catch-all); a list = one scoped pass per role, used by
  // the low-pool banner and the per-role card buttons so we only top up what's
  // actually short instead of every role. Outcomes across the passes are tallied
  // into a single summary line.
  const onFund = async (roles?: string[]) => {
    setMsg(null);
    const passes = roles && roles.length > 0 ? roles.map((r) => r as string) : [undefined];
    try {
      let spentLamports = 0;
      const tally: Record<string, number> = {};
      for (const role of passes) {
        const report = await fund({ role }).unwrap();
        spentLamports += report.spent_lamports;
        for (const o of report.outcomes) tally[o.result] = (tally[o.result] ?? 0) + 1;
      }
      const touched = Object.values(tally).reduce((a, b) => a + b, 0);
      if (touched === 0) {
        setMsg('Nothing to fund — pool already warm (or no treasury configured).');
      } else {
        const by = (r: string) => tally[r] ?? 0;
        const parts = [
          by('sent') && `${by('sent')} sent`,
          by('dry_run') && `${by('dry_run')} dry-run`,
          by('failed') && `${by('failed')} failed`,
          by('skipped_cap') && `${by('skipped_cap')} skipped (safety cap)`,
        ].filter(Boolean);
        setMsg(`Funding pass: ${parts.join(', ') || 'no transfers'} — ${formatSol(spentLamports)} spent.`);
      }
    } catch {
      /* surfaced via mutation error below */
    }
  };

  // "Sweep & retire": dust-sweep `used` wallets + reclaim `retired` ones. Direct
  // action (like Fund) — used wallets are terminal and retired ones are shredded.
  const onSweep = async () => {
    setMsg(null);
    setSweepResult(null);
    try {
      await sweep().unwrap();
      setMsg('Sweep started — progress streams live…');
    } catch {
      /* surfaced via mutation error below */
    }
  };

  const onConsolidate = async () => {
    if (!consolidateDest) return;
    setConsolidateErr(null);
    setSweepResult(null);
    try {
      await consolidate({ dest_treasury_id: consolidateDest }).unwrap();
      setMsg('Consolidate started — progress streams live…');
      setConsolidateOpen(false);
    } catch (e) {
      setConsolidateErr(
        apiErrorMessage(e as Parameters<typeof apiErrorMessage>[0]) ?? 'Consolidate failed.',
      );
    }
  };

  const poolActionBusy = poolAction?.status === 'running';

  const columns: Column<ManagedWalletPool>[] = [
    {
      header: 'Address',
      render: (w) => <AddressDisplay value={w.address} lead={6} tail={6} />,
      filterValue: (w) => w.address,
    },
    {
      header: 'Label',
      render: (w) => w.label ?? <span className="muted">—</span>,
      filterValue: (w) => w.label ?? '',
    },
    {
      header: 'Role',
      render: (w) => <RolePill role={w.role} />,
      filterValue: (w) => w.role,
    },
    {
      header: 'Status',
      render: (w) => <StatusPill status={w.status} />,
      filterValue: (w) => w.status,
    },
    {
      header: 'Balance',
      align: 'right',
      // Displayed units: SOL (lamports ÷ 1e9), matching the formatSol cell.
      filterNumber: (w) =>
        w.balance_lamports == null ? null : w.balance_lamports / LAMPORTS_PER_SOL,
      // Show every wallet's balance (incl. used/retired). A countable balance (freshly
      // read, or a stable funded/reserved wallet) renders normal — matching what folds
      // into the holdings total; a stale used/retired snapshot renders dimmed with its
      // age, so a number that may have drifted never masquerades as current.
      render: (w) => {
        if (w.balance_lamports == null) return <span className="muted">—</span>;
        // An empty wallet (typically a spent `used`/`retired` one) renders dimmed so a
        // real holding never visually blends with a zero — regardless of freshness.
        if (w.balance_lamports === 0) {
          return <span className="mono muted">{formatSol(0)}</span>;
        }
        if (isBalanceCountable(w)) {
          return <span className="mono">{formatSol(w.balance_lamports)}</span>;
        }
        const age = balanceAgeMs(w);
        return (
          <span
            className="mono muted"
            title={`Snapshot${age != null ? ` from ${formatAgo(age)}` : ''} — not live-polled in this status. Click "Refresh balances" for a live on-chain figure.`}
          >
            {formatSol(w.balance_lamports)}
          </span>
        );
      },
    },
    { header: 'Age', align: 'right', render: (w) => <AgeCell iso={w.created_at} /> },
    {
      header: '',
      align: 'right',
      render: (w) => (
        <div className="flex justify-end gap-1">
          <IconButton
            icon="transfer"
            label="Transfer"
            variant="ghost"
            disabled={NON_SOURCE_STATUSES.has(w.status)}
            title={
              NON_SOURCE_STATUSES.has(w.status)
                ? 'A launch is mid-flight against this wallet, or it is retired — not a valid source'
                : 'Transfer SOL to another wallet'
            }
            onClick={() => openTransfer(w)}
          />
          <IconButton
            icon="key"
            label="Export key"
            variant="ghost"
            onClick={() => {
              closeExport();
              setExportTarget(w);
            }}
          />
        </div>
      ),
    },
  ];

  const mutationError =
    apiErrorMessage(gen.error) ??
    apiErrorMessage(funding.error) ??
    apiErrorMessage(refreshing.error) ??
    apiErrorMessage(sweeping.error) ??
    apiErrorMessage(consolidating.error) ??
    apiErrorMessage(restoring.error) ??
    apiErrorMessage(error);

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h1 className="text-lg font-semibold">Wallet Pool</h1>
        <div className="flex flex-wrap gap-2">
          <Button
            variant="ghost"
            icon="broom"
            loading={sweeping.isLoading || (poolActionBusy && poolAction?.kind === 'sweep')}
            disabled={poolActionBusy}
            title={
              'Clean-up pass — gathers leftover SOL back into the treasury. Two things:\n\n' +
              '1) USED wallets (ones that already finished a launch): sends their leftover SOL home to the treasury and marks them retired. A wallet that still holds tokens is skipped — it keeps its SOL to pay the fee to sell those tokens later.\n\n' +
              '2) RETIRED wallets: sends home any SOL still sitting in them, and closes their empty token accounts to reclaim the ~0.002 SOL of rent locked inside each one. (Accounts that still hold tokens are left alone and just reported.)\n\n' +
              'Does not touch wallets you are actively using. Safe to run anytime.'
            }
            onClick={onSweep}
          >
            {poolActionBusy && poolAction?.kind === 'sweep'
              ? `Sweeping ${poolAction.done}/${poolAction.total}`
              : 'Sweep & retire'}
          </Button>
          <Button
            variant="ghost"
            icon="merge"
            loading={
              consolidating.isLoading || (poolActionBusy && poolAction?.kind === 'consolidate')
            }
            disabled={treasuryDestinations.length === 0 || poolActionBusy}
            title={
              treasuryDestinations.length === 0
                ? 'No treasury wallet exists to gather funds into. Create one first — use "Generate wallets" below with the role set to "treasury".'
                : 'Empties EVERYTHING into one treasury wallet you choose.\n\n' +
                  'Takes all the SOL out of every wallet — dev, bundler, trading, even other treasuries (every role, every status) — and closes their empty token accounts to reclaim the rent, moving it all into the one treasury you pick next.\n\n' +
                  'Wallets in the middle of a launch (funding or reserved) are skipped, so a live launch is never broken. Wallets are emptied but NOT retired — they can be funded and reused.\n\n' +
                  'Use this when winding down or pulling all funds into one place.'
            }
            onClick={() => {
              setConsolidateErr(null);
              setConsolidateDest(
                treasuryDestinations.length === 1 ? treasuryDestinations[0].id : '',
              );
              setConsolidateOpen(true);
            }}
          >
            Consolidate → treasury
          </Button>
          <Button
            variant="ghost"
            icon="refresh"
            loading={restoring.isLoading || restoreRunning}
            title={
              'Recovery: rebuild the database from the copied keystore + on-chain history.\n\n' +
              'Use this on a fresh box whose Postgres came up empty but whose keystore (.enc files) was copied across. It re-derives every managed wallet from the keystore, discovers which mints the dev wallets launched, and backfills all past trades, held mints (incl. co-buys), and current positions from the chain.\n\n' +
              'Safe to run more than once — every write is idempotent. Needs the same keystore + KEK passphrase that encrypted the wallets.'
            }
            onClick={() => setRestoreOpen(true)}
          >
            Restore from keystore
          </Button>
          <Button variant="primary" icon="coins" loading={funding.isLoading} onClick={() => onFund()}>
            Fund pool
          </Button>
        </div>
      </div>

      {poolActionBusy && poolAction && (
        <Banner tone="warn">
          <strong>
            {poolAction.kind === 'sweep' ? 'Sweeping' : 'Consolidating'}…{' '}
            {poolAction.done}/{poolAction.total}
          </strong>{' '}
          wallet(s) processed.
        </Banner>
      )}

      {(restoreRunning || restoreProgress || restoreSummary) && (
        <Banner tone={restoreSummary ? 'good' : 'info'}>
          {restoreSummary ? (
            <>
              <strong>Restore complete.</strong> {restoreSummary.wallets_total} wallets (
              {restoreSummary.wallets_inserted} new), {restoreSummary.launches} launches,{' '}
              {restoreSummary.trades} trades, {restoreSummary.mints} mints,{' '}
              {restoreSummary.positions_reconciled} positions reconciled.
            </>
          ) : (
            <>
              <strong>Restoring from keystore…</strong>{' '}
              {restoreProgress ? (
                <span className="mono">
                  {restoreProgress.phase}
                  {restoreProgress.total > 0
                    ? ` ${restoreProgress.done}/${restoreProgress.total}`
                    : ''}
                </span>
              ) : (
                'starting…'
              )}
            </>
          )}
        </Banner>
      )}

      {lowPoolRoles.length > 0 && (
        <Banner
          tone="warn"
          actions={
            lowFundableRoles.length > 0 ? (
              <Button
                variant="primary"
                size="sm"
                icon="coins"
                loading={funding.isLoading}
                title={`Top up ${lowFundableRoles.join(' + ')} from the treasury — only the low role${lowFundableRoles.length > 1 ? 's' : ''}, not the whole pool`}
                onClick={() => onFund(lowFundableRoles)}
              >
                Fund {lowFundableRoles.join(' + ')}
              </Button>
            ) : undefined
          }
        >
          <strong>Low pool:</strong>{' '}
          {lowPoolRoles.map(([role, s]) => `${role} (${s.funded ?? 0} funded)`).join(', ')} — below{' '}
          {LOW_POOL_THRESHOLD}.
        </Banner>
      )}
      {msg && <Banner tone="info">{msg}</Banner>}
      {mutationError && <Banner tone="bad">{mutationError}</Banner>}

      {sweepResult && (
        <Card
          title={`${sweepResult.title} — result`}
          actions={
            <Button variant="ghost" size="sm" onClick={() => setSweepResult(null)}>
              Dismiss
            </Button>
          }
        >
          {(() => {
            const failed = sweepResult.report.outcomes.filter((o) => o.failed);
            if (failed.length === 0) return null;
            const addrs = failed
              .map((o) => `${o.address.slice(0, 6)}…${o.address.slice(-4)}`)
              .join(', ');
            return (
              <Banner tone="bad">
                <strong>
                  {failed.length} wallet{failed.length === 1 ? '' : 's'} failed to drain
                </strong>{' '}
                — {addrs}. Their SOL is untouched (nothing was lost); re-run{' '}
                <strong>{sweepResult.title}</strong> to retry just the failures. Per-wallet
                reasons are in the table below.
              </Banner>
            );
          })()}
          <div className="mt-3 flex flex-wrap gap-x-6 gap-y-1 text-sm">
            <span>
              <span className="muted">SOL swept</span>{' '}
              <span className="mono">{formatSol(sweepResult.report.total_sol_swept_lamports)}</span>
            </span>
            <span>
              <span className="muted">Accounts closed</span>{' '}
              <span className="mono">{sweepResult.report.accounts_closed}</span>
            </span>
            <span>
              <span className="muted">Rent reclaimed</span>{' '}
              <span className="mono">
                {formatSol(sweepResult.report.total_rent_reclaimed_lamports)}
              </span>
            </span>
            <span>
              <span className="muted">Retired</span>{' '}
              <span className="mono">{sweepResult.report.wallets_retired}</span>
            </span>
          </div>
          <div className="mt-3">
            <DataTable
              columns={[
                {
                  header: 'Address',
                  render: (o) => <AddressDisplay value={o.address} lead={6} tail={6} />,
                },
                { header: 'Role', render: (o) => <RolePill role={o.role} /> },
                { header: 'Status', render: (o) => <StatusPill status={o.status} /> },
                {
                  header: 'SOL swept',
                  align: 'right',
                  render: (o) =>
                    o.sol_swept_lamports > 0 ? (
                      <span className="mono">{formatSol(o.sol_swept_lamports)}</span>
                    ) : (
                      <span className="muted">—</span>
                    ),
                },
                {
                  header: 'Accts closed',
                  align: 'right',
                  render: (o) => (
                    <span className="mono">
                      {o.accounts_closed}
                      {o.accounts_nonempty_skipped > 0 && (
                        <span
                          className="muted"
                          title={`${o.accounts_nonempty_skipped} non-empty account(s) left open — can't close a token account that still holds a balance`}
                        >
                          {' '}
                          (+{o.accounts_nonempty_skipped} held)
                        </span>
                      )}
                    </span>
                  ),
                },
                {
                  header: 'Rent',
                  align: 'right',
                  render: (o) =>
                    o.rent_reclaimed_lamports > 0 ? (
                      <span className="mono">{formatSol(o.rent_reclaimed_lamports)}</span>
                    ) : (
                      <span className="muted">—</span>
                    ),
                },
                {
                  header: 'Note',
                  render: (o) =>
                    o.note ? (
                      <span
                        className="text-xs"
                        style={{ color: o.failed ? 'var(--color-bad)' : undefined }}
                        title={o.note}
                      >
                        {o.failed && <span className="mr-1">⚠</span>}
                        <span className={o.failed ? '' : 'muted'}>{o.note}</span>
                      </span>
                    ) : (
                      <span className="muted">—</span>
                    ),
                },
              ] satisfies Column<SweepReport['outcomes'][number]>[]}
              rows={sweepResult.report.outcomes.filter(
                (o) => o.sol_swept_lamports > 0 || o.accounts_closed > 0 || o.note,
              )}
              rowKey={(o) => o.wallet_id}
              empty="No wallets needed sweeping — the pool is already clean."
            />
          </div>
        </Card>
      )}

      <Card title={roleFilter ? `Holdings · ${roleFilter}` : 'Holdings'}>
        <div className="flex flex-wrap items-end justify-between gap-4">
          <div>
            <div className="flex items-baseline gap-2">
              <span className="text-3xl font-semibold leading-none">
                {formatSol(holdings.totalLamports)}
              </span>
              <IconButton
                icon="refresh"
                label="Refresh balances"
                variant="ghost"
                loading={refreshing.isLoading}
                onClick={onRefreshBalances}
              />
            </div>
            <p className="mt-1 text-xs muted">
              across {holdings.walletCount} wallet{holdings.walletCount === 1 ? '' : 's'}
              {holdings.oldestCheck != null
                ? ` · as of ${formatAgo(Date.now() - holdings.oldestCheck)}`
                : ''}
              {holdings.snapshotCount > 0
                ? ` · incl. ${formatSol(holdings.snapshotLamports)} snapshot (${holdings.snapshotCount} not live-checked) — Refresh to confirm`
                : ''}
            </p>
          </div>
          <div className="flex flex-col items-end gap-1.5 text-sm">
            <div className="flex flex-wrap justify-end gap-x-4 gap-y-1">
              {ROLES.filter((r) => (holdings.byRole[r] ?? 0) > 0).map((r) => (
                <span key={r} className="flex items-center gap-1.5">
                  <RolePill role={r} />
                  <span className="mono">{formatSol(holdings.byRole[r])}</span>
                </span>
              ))}
            </div>
            {/* Terminal holdings (used/retired) on their own line beneath the role chips,
                dimmed + prefixed so they read as a distinct "parked, awaiting sweep" group. */}
            <div className="flex flex-wrap items-center justify-end gap-x-3 gap-y-1 border-t border-[var(--color-line)] pt-1.5 opacity-75">
              {STATUSES.filter((s) => (holdings.byTerminalStatus[s] ?? 0) > 0).map((s) => (
                <span key={s} className="flex items-center gap-1.5">
                  <StatusPill status={s} />
                  <span className="mono text-xs">{formatSol(holdings.byTerminalStatus[s])}</span>
                </span>
              ))}
            </div>
          </div>
        </div>
        <p className="mt-2 text-xs muted">
          On-chain total across <em>every</em> wallet and role (incl. <code>used</code>/
          <code>retired</code>). The background poller keeps only <code>generated</code>/
          <code>funding</code> + treasury live and <code>funded</code>/<code>reserved</code>{' '}
          balances are stable until a launch spends them, so <code>used</code>/
          <code>retired</code> wallets carry a last-known snapshot between refreshes — that
          slice is noted above; click <strong>Refresh balances</strong> to re-read every
          wallet on-chain and make the whole figure exact.
        </p>
      </Card>

      <Card title="Generate wallets">
        <div className="flex flex-wrap items-end gap-3">
          <Field label="Role" htmlFor="gen-role" className="w-40">
            <Select id="gen-role" value={genRole} onChange={(e) => setGenRole(e.target.value as WalletRole)}>
              {ROLES.map((r) => (
                <option key={r} value={r}>
                  {r}
                </option>
              ))}
            </Select>
          </Field>
          <Field label="Count" htmlFor="gen-count" className="w-28">
            <Input
              id="gen-count"
              type="number"
              min={1}
              max={200}
              value={genCount}
              onChange={(e) => setGenCount(Number(e.target.value))}
            />
          </Field>
          <Field label="Label prefix (optional)" htmlFor="gen-label" className="flex-1 min-w-48">
            <Input
              id="gen-label"
              value={genLabel}
              onChange={(e) => setGenLabel(e.target.value)}
              placeholder="e.g. batch-07"
            />
          </Field>
          <Button variant="primary" icon="plus" loading={gen.isLoading} onClick={onGenerate}>
            Generate
          </Button>
        </div>
        <p className="mt-2 text-xs muted">
          Creates fresh ed25519 keypairs, envelope-encrypts each into the keystore, and inserts them
          as <code>generated</code>. Fund them (manually, or the treasury pass above) — the balance
          poller promotes to <code>funded</code> once SOL lands.
        </p>
      </Card>

      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
        {ROLES.map((role) => {
          const s = counts[role] ?? {};
          const total = STATUSES.reduce((n, st) => n + (s[st] ?? 0), 0);
          return (
            <RoleSummaryCard
              key={role}
              role={role}
              counts={s}
              total={total}
              active={roleFilter === role}
              onSelect={() => setRoleFilter(roleFilter === role ? '' : role)}
              onFund={FUNDABLE_ROLES.includes(role) ? () => onFund([role]) : undefined}
              funding={funding.isLoading}
            />
          );
        })}
      </div>

      <Card
        title={`Pool (${sortedWallets.length})`}
        actions={
          <div className="flex items-center gap-2">
            <FilterToggle
              tone={statusTone('retired')}
              active={showRetiredEmpty}
              count={hiddenRetiredCount}
              disabled={hiddenRetiredCount === 0}
              title={
                hiddenRetiredCount === 0
                  ? 'No retired, empty wallets to show.'
                  : 'Toggle retired wallets holding no SOL in/out of the table.'
              }
              onClick={() => setShowRetiredEmpty((v) => !v)}
            >
              Retired
            </FilterToggle>
            <FilterToggle
              tone={statusTone('generated')}
              active={showGenerated}
              count={hiddenGeneratedCount}
              disabled={hiddenGeneratedCount === 0}
              title={
                hiddenGeneratedCount === 0
                  ? 'No generated (unfunded) wallets to show.'
                  : 'Toggle freshly-generated, not-yet-funded wallets in/out of the table.'
              }
              onClick={() => setShowGenerated((v) => !v)}
            >
              Generated
            </FilterToggle>
            <Select value={roleFilter} onChange={(e) => setRoleFilter(e.target.value)} className="w-40">
              <option value="">All roles</option>
              {ROLES.map((r) => (
                <option key={r} value={r}>
                  {r}
                </option>
              ))}
            </Select>
          </div>
        }
      >
        <DataTable
          columns={columns}
          rows={sortedWallets}
          rowKey={(w) => w.id}
          loading={isFetching}
          empty="No wallets yet — generate a batch above."
          pinning={pinning}
          filterable
        />
      </Card>

      {exportTarget && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
          onClick={closeExport}
        >
          <div className="w-full max-w-lg" onClick={(e) => e.stopPropagation()}>
            <Card title="Export private key">
              <Banner tone="bad">
                <strong>Danger:</strong> this reveals the raw private key for{' '}
                <span className="mono">{exportTarget.address}</span>. Anyone with this key controls
                the wallet's funds. Only export to import into a wallet you control, over a trusted
                connection.
              </Banner>

              {!revealedKey ? (
                <div className="mt-3 space-y-3">
                  <Field label="Export secret" htmlFor="export-secret">
                    <Input
                      id="export-secret"
                      type="password"
                      autoComplete="off"
                      value={exportSecret}
                      onChange={(e) => setExportSecret(e.target.value)}
                      placeholder="WALLET_EXPORT_SECRET"
                    />
                  </Field>
                  {exportErr && <Banner tone="bad">{exportErr}</Banner>}
                  <div className="flex justify-end gap-2">
                    <Button variant="ghost" onClick={closeExport}>
                      Cancel
                    </Button>
                    <Button
                      variant="primary"
                      loading={exporting.isLoading}
                      disabled={!exportSecret}
                      onClick={onExport}
                    >
                      Reveal key
                    </Button>
                  </div>
                </div>
              ) : (
                <div className="mt-3 space-y-3">
                  <Field label="Private key (base58)" htmlFor="export-key">
                    <Input id="export-key" readOnly value={revealedKey} className="mono" />
                  </Field>
                  <p className="text-xs muted">
                    Copy it now — it is not stored anywhere and disappears when you close this
                    dialog.
                  </p>
                  <div className="flex justify-end gap-2">
                    <Button variant="ghost" onClick={onCopyKey}>
                      {copied ? 'Copied' : 'Copy'}
                    </Button>
                    <Button variant="primary" onClick={closeExport}>
                      Done
                    </Button>
                  </div>
                </div>
              )}
            </Card>
          </div>
        </div>
      )}

      {transferTarget && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
          onClick={closeTransfer}
        >
          <div className="w-full max-w-lg" onClick={(e) => e.stopPropagation()}>
            <Card title="Transfer SOL">
              <p className="text-xs muted">
                From <RolePill role={transferTarget.role} />{' '}
                <span className="mono">{transferTarget.address}</span>. The source signs and pays
                the fee.
              </p>

              {!transferResult ? (
                <div className="mt-3 space-y-3">
                  <Field label="Destination wallet" htmlFor="transfer-dest">
                    <Select
                      id="transfer-dest"
                      value={transferDest}
                      onChange={(e) => setTransferDest(e.target.value)}
                    >
                      <option value="">Select a wallet…</option>
                      {transferDestinations.map((w) => (
                        <option key={w.id} value={w.id}>
                          {w.role} · {w.address.slice(0, 6)}…{w.address.slice(-6)}
                          {w.label ? ` · ${w.label}` : ''}
                        </option>
                      ))}
                    </Select>
                  </Field>
                  <div className="flex items-end gap-3">
                    <Field label="Amount (SOL)" htmlFor="transfer-amount" className="flex-1">
                      <Input
                        id="transfer-amount"
                        type="number"
                        min={0}
                        step="any"
                        value={transferAmount}
                        disabled={transferMax}
                        onChange={(e) => setTransferAmount(e.target.value)}
                        placeholder="0.05"
                      />
                    </Field>
                    <label className="mb-2 flex items-center gap-2 text-sm">
                      <input
                        type="checkbox"
                        checked={transferMax}
                        onChange={(e) => setTransferMax(e.target.checked)}
                      />
                      Max (sweep to ~0)
                    </label>
                  </div>
                  {transferErr && <Banner tone="bad">{transferErr}</Banner>}
                  <div className="flex justify-end gap-2">
                    <Button variant="ghost" onClick={closeTransfer}>
                      Cancel
                    </Button>
                    <Button
                      variant="primary"
                      loading={transferring.isLoading}
                      disabled={!transferDest || (!transferMax && !(Number(transferAmount) > 0))}
                      onClick={onTransfer}
                    >
                      Send
                    </Button>
                  </div>
                </div>
              ) : (
                <div className="mt-3 space-y-3">
                  <Banner tone="good">
                    {transferResult.signature ? (
                      <>
                        Sent <strong>{formatSol(transferResult.lamports_sent)}</strong> —{' '}
                        <span className="mono break-all">{transferResult.signature}</span>
                      </>
                    ) : (
                      'Nothing to send — the source balance was at or below the sweep floor.'
                    )}
                  </Banner>
                  <div className="flex justify-end">
                    <Button variant="primary" onClick={closeTransfer}>
                      Done
                    </Button>
                  </div>
                </div>
              )}
            </Card>
          </div>
        </div>
      )}

      {restoreOpen && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
          onClick={() => setRestoreOpen(false)}
        >
          <div className="w-full max-w-lg" onClick={(e) => e.stopPropagation()}>
            <Card title="Restore from keystore">
              <Banner tone="info">
                Rebuilds <code>managed_wallets</code>, <code>launches</code>, <code>tokens</code>,{' '}
                <code>trades</code>, and <code>token_positions</code> from the copied keystore
                folder + on-chain history — the recovery path for a fresh box whose Postgres came
                up empty.
              </Banner>
              <ul className="mt-3 list-disc space-y-1 pl-5 text-sm muted">
                <li>Re-derives every managed wallet from its <code>.enc</code> blob (fresh ids; on-chain data joins by address).</li>
                <li>Discovers which mints the dev wallets launched, and backfills all past trades + held mints (incl. co-buys).</li>
                <li>
                  Runs in the background — potentially thousands of RPC calls; progress shows above.
                  Safe to run more than once (every write is idempotent).
                </li>
              </ul>
              <div className="mt-4 flex justify-end gap-2">
                <Button variant="ghost" onClick={() => setRestoreOpen(false)}>
                  Cancel
                </Button>
                <Button
                  variant="primary"
                  icon="refresh"
                  loading={restoring.isLoading}
                  onClick={onRestore}
                >
                  Start restore
                </Button>
              </div>
            </Card>
          </div>
        </div>
      )}

      {consolidateOpen && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
          onClick={() => setConsolidateOpen(false)}
        >
          <div className="w-full max-w-lg" onClick={(e) => e.stopPropagation()}>
            <Card title="Consolidate → treasury">
              <Banner tone="warn">
                <strong>Drains every wallet.</strong> Sweeps the SOL of <em>all</em> managed
                wallets (all roles, incl. other treasuries) into the chosen treasury and closes
                their empty token accounts to reclaim rent. Wallets mid-launch (
                <code>funding</code>/<code>reserved</code>) are skipped.
              </Banner>
              <div className="mt-3 space-y-3">
                <Field label="Destination treasury" htmlFor="consolidate-dest">
                  <Select
                    id="consolidate-dest"
                    value={consolidateDest}
                    onChange={(e) => setConsolidateDest(e.target.value)}
                  >
                    <option value="">Select a treasury wallet…</option>
                    {treasuryDestinations.map((w) => (
                      <option key={w.id} value={w.id}>
                        {w.address.slice(0, 6)}…{w.address.slice(-6)}
                        {w.label ? ` · ${w.label}` : ''}
                      </option>
                    ))}
                  </Select>
                </Field>
                {consolidateErr && <Banner tone="bad">{consolidateErr}</Banner>}
                <div className="flex justify-end gap-2">
                  <Button variant="ghost" onClick={() => setConsolidateOpen(false)}>
                    Cancel
                  </Button>
                  <Button
                    variant="primary"
                    loading={consolidating.isLoading}
                    disabled={!consolidateDest}
                    onClick={onConsolidate}
                  >
                    Consolidate
                  </Button>
                </div>
              </div>
            </Card>
          </div>
        </div>
      )}
    </div>
  );
}
