import type { InputHTMLAttributes, ReactNode } from 'react';
import { Switch } from 'components/ui/Switch';
import { Input } from 'components/ui/Input';
import { InfoTooltip } from 'components/ui/InfoTooltip';
import { cn } from 'lib/cn';

export function SettingsPanel({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn('rounded-xl border border-white/8 bg-bg-panel', className)}
    >
      {children}
    </div>
  );
}

export function SettingsPanelIntro({
  title,
  description,
  tip,
}: {
  title: string;
  description: string;
  tip?: { title?: string; body: string };
}) {
  return (
    <div className="border-b border-white/6 px-4 py-3">
      <div className="flex items-center gap-1.5">
        <h3 className="text-sm font-semibold text-text">{title}</h3>
        {tip && <InfoTooltip title={tip.title ?? title} body={tip.body} />}
      </div>
      <p className="mt-0.5 text-xs leading-relaxed text-text-dim">{description}</p>
    </div>
  );
}

export function SettingsRows({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return <div className={cn('divide-y divide-white/6 px-4', className)}>{children}</div>;
}

interface ToggleRowProps {
  title: string;
  /** One-line summary kept beside the switch. */
  description?: string;
  /** Longer help parked behind ⓘ. */
  tip?: string;
  checked: boolean;
  disabled: boolean;
  onChange: (next: boolean) => void;
  /** Optional controls nested under this toggle (always visible; disable inside). */
  children?: ReactNode;
}

export function ToggleRow({
  title,
  description,
  tip,
  checked,
  disabled,
  onChange,
  children,
}: ToggleRowProps) {
  return (
    <div className="py-3">
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <div className="flex items-center gap-1.5">
            <div className="text-sm font-medium text-text">{title}</div>
            {tip && <InfoTooltip title={title} body={tip} />}
          </div>
          {description && (
            <p className="mt-0.5 text-xs leading-relaxed text-text-dim">{description}</p>
          )}
        </div>
        <div className="pt-0.5">
          <Switch checked={checked} disabled={disabled} onChange={onChange} label={title} />
        </div>
      </div>
      {children ? (
        <div
          className={cn(
            'mt-3 flex flex-wrap gap-4 border-l-2 border-white/8 pl-3 transition-opacity',
            !checked && 'opacity-45',
          )}
        >
          {children}
        </div>
      ) : null}
    </div>
  );
}

type NumberFieldProps = {
  label: string;
  hint?: string;
  value: string;
  disabled?: boolean;
  onChange: (next: string) => void;
  onCommit: () => void;
} & Omit<InputHTMLAttributes<HTMLInputElement>, 'value' | 'onChange' | 'size'>;

export function NumberField({
  label,
  hint,
  value,
  disabled,
  onChange,
  onCommit,
  className,
  ...inputProps
}: NumberFieldProps) {
  return (
    <label className={cn('flex w-[11.5rem] flex-col gap-1.5', className)}>
      <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">{label}</span>
      <Input
        type="number"
        fieldSize="md"
        {...inputProps}
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value)}
        onBlur={onCommit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
        }}
      />
      {hint && <p className="text-[11px] text-text-dim">{hint}</p>}
    </label>
  );
}
