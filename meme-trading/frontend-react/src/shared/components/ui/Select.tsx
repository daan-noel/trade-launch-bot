import { forwardRef, type SelectHTMLAttributes } from 'react';
import { fieldClassName, type FieldProps } from './Input';

export const Select = forwardRef<
  HTMLSelectElement,
  SelectHTMLAttributes<HTMLSelectElement> & FieldProps
>(function Select({ className, fieldSize = 'sm', variant = 'default', ...props }, ref) {
  return (
    <select
      ref={ref}
      className={fieldClassName({ size: fieldSize, variant, className })}
      {...props}
    />
  );
});
