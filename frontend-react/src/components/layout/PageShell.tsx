interface PageShellProps {
  title: string;
  subtitle?: string;
}

export function PageShell({ title, subtitle }: PageShellProps) {
  return (
    <div>
      <div className="mb-3.5 flex items-center gap-2.5">
        <h1 className="text-base font-bold text-primary">{title}</h1>
      </div>
      {subtitle && <p className="mb-4 text-sm text-text-mid">{subtitle}</p>}
      <div className="rounded-lg border border-border bg-bg-panel/50 p-8 text-center text-text-dim">
        Coming soon — page scaffold ready.
      </div>
    </div>
  );
}
