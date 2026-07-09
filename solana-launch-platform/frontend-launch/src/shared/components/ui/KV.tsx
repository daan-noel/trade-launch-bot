import { ReactNode } from 'react';

/** Definition-list detail grid (label → value). */
export function KV({ children, cols = 1 }: { children: ReactNode; cols?: 1 | 2 }) {
  return (
    <dl
      className="grid gap-x-6 gap-y-2 text-sm"
      style={{ gridTemplateColumns: cols === 2 ? '140px 1fr 140px 1fr' : '160px 1fr' }}
    >
      {children}
    </dl>
  );
}

export function KVRow({ label, children }: { label: ReactNode; children: ReactNode }) {
  return (
    <>
      <dt className="muted">{label}</dt>
      <dd className="m-0 break-all">{children}</dd>
    </>
  );
}
