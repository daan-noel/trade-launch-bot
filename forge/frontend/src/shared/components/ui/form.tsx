import { InputHTMLAttributes, ReactNode, SelectHTMLAttributes, TextareaHTMLAttributes } from 'react';
import clsx from 'clsx';
import { Icon } from './Icon';

/**
 * Hover/focus info tooltip — an inline info glyph that reveals a styled bubble of
 * explanatory prose. Keyboard-reachable (`tabIndex`) and screen-reader labelled.
 * `align="end"` grows the bubble leftward for fields near the container's right edge.
 */
export function InfoTip({ text, align = 'start' }: { text: ReactNode; align?: 'start' | 'end' }) {
  return (
    <span
      className={clsx('infotip', align === 'end' && 'infotip--end')}
      tabIndex={0}
      role="note"
      aria-label={typeof text === 'string' ? text : undefined}
    >
      <Icon name="info" size={13} />
      <span className="infotip__bubble" role="tooltip">
        {text}
      </span>
    </span>
  );
}

export function Field({
  label,
  htmlFor,
  hint,
  tip,
  tipAlign,
  className,
  children,
}: {
  label?: ReactNode;
  htmlFor?: string;
  hint?: ReactNode;
  tip?: ReactNode;
  tipAlign?: 'start' | 'end';
  className?: string;
  children: ReactNode;
}) {
  return (
    <div className={clsx('flex flex-col gap-1 min-w-0', className)}>
      {label && (
        <label htmlFor={htmlFor} className="field-label inline-flex items-center gap-1">
          {label}
          {tip && <InfoTip text={tip} align={tipAlign} />}
        </label>
      )}
      {children}
      {hint && <span className="text-xs muted">{hint}</span>}
    </div>
  );
}

export function Input({ className, ...rest }: InputHTMLAttributes<HTMLInputElement>) {
  return <input className={clsx('input', className)} {...rest} />;
}

export function Textarea({ className, ...rest }: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return <textarea className={clsx('input', className)} {...rest} />;
}

export function Select({
  className,
  children,
  ...rest
}: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select className={clsx('select', className)} {...rest}>
      {children}
    </select>
  );
}

export function Checkbox({
  label,
  className,
  id,
  ...rest
}: InputHTMLAttributes<HTMLInputElement> & { label?: ReactNode }) {
  return (
    <label htmlFor={id} className={clsx('inline-flex items-center gap-2 text-sm cursor-pointer', className)}>
      <input id={id} type="checkbox" className="h-4 w-4 accent-[var(--color-accent)]" {...rest} />
      {label}
    </label>
  );
}
