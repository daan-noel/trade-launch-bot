import type { TextareaHTMLAttributes } from 'react';
import { Textarea, type FieldSize, type FieldVariant } from 'components/ui/Input';
import { cn } from 'lib/cn';

const DEFAULT_PLACEHOLDER = '[\n  "Pump.Fun: Create",\n  "Pump.Fun: Buy"\n]';

export interface IxLabelsInputProps
  extends Omit<TextareaHTMLAttributes<HTMLTextAreaElement>, 'children'> {
  value: string;
  onValueChange: (value: string) => void;
  fieldSize?: FieldSize;
  variant?: FieldVariant;
  /** Parse / validation error shown under the field. */
  error?: string | null;
}

/**
 * Auto-growing ix_labels editor. Primary syntax: pretty-printed JSON string
 * array (see `parseIxLabelsText` / `formatIxLabelsText`).
 */
export function IxLabelsInput({
  value,
  onValueChange,
  fieldSize = 'sm',
  variant = 'default',
  error,
  className,
  placeholder = DEFAULT_PLACEHOLDER,
  disabled,
  title,
  ...rest
}: IxLabelsInputProps) {
  return (
    <div className="flex flex-col gap-0.5">
      <Textarea
        fieldSize={fieldSize}
        variant={variant}
        autoResize
        rows={1}
        value={value}
        disabled={disabled}
        title={title}
        placeholder={placeholder}
        onChange={(e) => onValueChange(e.target.value)}
        className={cn('font-mono', className)}
        {...rest}
      />
      {error && <span className="text-[10px] text-danger">{error}</span>}
    </div>
  );
}
