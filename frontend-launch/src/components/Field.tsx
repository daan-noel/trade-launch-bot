import { ReactNode } from 'react';

/**
 * The `<div className="field"><label/>…</div>` wrapper repeated across every
 * form. `htmlFor`/`id` stay the caller's responsibility so labels bind to inputs.
 */
export function Field({
  label,
  htmlFor,
  children,
  style,
}: {
  label: ReactNode;
  htmlFor?: string;
  children: ReactNode;
  style?: React.CSSProperties;
}) {
  return (
    <div className="field" style={style}>
      <label htmlFor={htmlFor}>{label}</label>
      {children}
    </div>
  );
}
