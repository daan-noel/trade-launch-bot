import { Component, type ErrorInfo, type ReactNode } from 'react';
import { useLocation } from 'react-router-dom';
import { Button } from './Button';

interface ErrorBoundaryProps {
  children: ReactNode;
  /** Clear the caught error (re-render children) whenever any value here changes. */
  resetKeys?: unknown[];
  /** Custom fallback; receives the caught error + a manual reset callback. */
  fallback?: (error: Error, reset: () => void) => ReactNode;
  /** `root` = full-screen catch-all; `page` = contained card under the live nav. */
  variant?: 'root' | 'page';
}

interface ErrorBoundaryState {
  error: Error | null;
}

/**
 * Render-error firewall. A throw in any descendant cell/formatter (e.g. a null in
 * a live trade tick) is caught here instead of unmounting the whole tree, so the
 * dashboard never white-screens mid-trade. Reset is automatic on `resetKeys`
 * change (we pass the route path — see {@link RouteErrorBoundary}) and manual via
 * the fallback's "Try again".
 */
export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    // Degrade to the fallback UI, but keep the crash visible for diagnostics.
    console.error('Render error caught by ErrorBoundary:', error, info.componentStack);
  }

  componentDidUpdate(prev: ErrorBoundaryProps) {
    if (this.state.error && !sameKeys(prev.resetKeys, this.props.resetKeys)) {
      this.reset();
    }
  }

  reset = () => this.setState({ error: null });

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    if (this.props.fallback) return this.props.fallback(error, this.reset);
    return <Fallback error={error} reset={this.reset} variant={this.props.variant ?? 'page'} />;
  }
}

function sameKeys(a?: unknown[], b?: unknown[]) {
  if (a === b) return true;
  if (!a || !b || a.length !== b.length) return false;
  return a.every((v, i) => Object.is(v, b[i]));
}

function Fallback({
  error,
  reset,
  variant,
}: {
  error: Error;
  reset: () => void;
  variant: 'root' | 'page';
}) {
  const card = (
    <div className="mx-auto flex max-w-md flex-col items-center gap-3 rounded-lg border border-red/40 bg-red/5 px-6 py-8 text-center">
      <div className="text-sm font-semibold text-red">Something went wrong rendering this view.</div>
      <p className="text-xs text-text-dim">
        The rest of the dashboard is still live. Try again, or reload if it persists.
      </p>
      {error.message && (
        <pre className="max-w-full overflow-x-auto whitespace-pre-wrap break-words rounded bg-black/30 px-3 py-2 text-left text-[11px] text-text-dim">
          {error.message}
        </pre>
      )}
      <div className="flex gap-2">
        <Button variant="primary" size="sm" onClick={reset}>
          Try again
        </Button>
        <Button variant="ghost" size="sm" onClick={() => window.location.reload()}>
          Reload
        </Button>
      </div>
    </div>
  );

  if (variant === 'root') {
    return <div className="flex min-h-screen items-center justify-center bg-bg px-6 text-text">{card}</div>;
  }
  return <div className="py-10">{card}</div>;
}

/**
 * Route-aware boundary: resets automatically when the path changes, so navigating
 * away from a broken page recovers without a manual reset or a reload.
 */
export function RouteErrorBoundary({
  children,
  variant,
}: {
  children: ReactNode;
  variant?: 'root' | 'page';
}) {
  const { pathname } = useLocation();
  return (
    <ErrorBoundary resetKeys={[pathname]} variant={variant}>
      {children}
    </ErrorBoundary>
  );
}
