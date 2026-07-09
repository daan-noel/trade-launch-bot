import { forwardRef, type InputHTMLAttributes } from 'react';
import { cn } from 'lib/cn';

type CheckboxSize = 'sm' | 'md';

const sizeClasses: Record<CheckboxSize, string> = {
  sm: 'size-3',
  md: 'size-4',
};

export const Checkbox = forwardRef<
  HTMLInputElement,
  Omit<InputHTMLAttributes<HTMLInputElement>, 'type' | 'size'> & { boxSize?: CheckboxSize }
>(function Checkbox({ className, boxSize = 'md', ...props }, ref) {
  return (
    <input
      ref={ref}
      type="checkbox"
      className={cn('rounded border-white/20 accent-primary', sizeClasses[boxSize], className)}
      {...props}
    />
  );
});
