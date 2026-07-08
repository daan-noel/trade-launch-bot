// One status pill for the whole app (was duplicated in App.tsx + WalletPool.tsx,
// and they'd drifted — App normalized the CSS class, WalletPool didn't, so a
// multi-word status like "in-progress" matched different/no rules). Normalize to
// `[a-z]` so the class is stable regardless of separators/casing.
export function StatusPill({ status }: { status: string }) {
  const cls = status.toLowerCase().replace(/[^a-z]/g, '');
  return <span className={`status-pill ${cls}`}>{status}</span>;
}
