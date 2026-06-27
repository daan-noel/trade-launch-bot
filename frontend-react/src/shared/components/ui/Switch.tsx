import { cn } from 'lib/cn';

interface SwitchProps {
  checked: boolean;
  onChange: (next: boolean) => void;
  disabled?: boolean;
  /** Accessible name, e.g. "Track Mayhem-mode tokens" */
  label: string;
}

/** Accessible on/off toggle. Controlled via `checked` / `onChange`. */
export function Switch({ checked, onChange, disabled, label }: SwitchProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        'relative inline-flex h-6 w-11 shrink-0 items-center rounded-full border transition',
        'focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/40',
        'disabled:cursor-not-allowed disabled:opacity-50',
        checked ? 'border-primary/40 bg-primary/30' : 'border-white/10 bg-white/5',
      )}
    >
      <span
        className={cn(
          'inline-block size-4 rounded-full shadow transition-transform',
          checked ? 'translate-x-5 bg-primary' : 'translate-x-1 bg-text-dim',
        )}
      />
    </button>
  );
}
