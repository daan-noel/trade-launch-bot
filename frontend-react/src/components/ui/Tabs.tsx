import {
  createContext,
  useContext,
  type ButtonHTMLAttributes,
  type HTMLAttributes,
  type ReactNode,
} from 'react';
import { cn } from '../../lib/cn';

type TabVariant = 'underline' | 'contained';

interface TabsContextValue {
  value: string;
  onValueChange: (value: string) => void;
  variant: TabVariant;
}

const TabsContext = createContext<TabsContextValue | null>(null);

function useTabsContext() {
  const ctx = useContext(TabsContext);
  if (!ctx) throw new Error('Tabs components must be used within <Tabs>');
  return ctx;
}

interface TabsProps extends HTMLAttributes<HTMLDivElement> {
  value: string;
  onValueChange: (value: string) => void;
  variant?: TabVariant;
  children: ReactNode;
}

export function Tabs({
  value,
  onValueChange,
  variant = 'underline',
  className,
  children,
  ...props
}: TabsProps) {
  return (
    <TabsContext.Provider value={{ value, onValueChange, variant }}>
      <div className={cn('flex flex-col', className)} {...props}>
        {children}
      </div>
    </TabsContext.Provider>
  );
}

interface TabsListProps extends HTMLAttributes<HTMLDivElement> {
  children: ReactNode;
}

export function TabsList({ className, children, ...props }: TabsListProps) {
  const { variant } = useTabsContext();

  return (
    <div
      role="tablist"
      className={cn(
        'flex shrink-0 gap-0',
        variant === 'underline' && 'border-b border-border',
        variant === 'contained' && 'border-b border-white/8',
        className,
      )}
      {...props}
    >
      {children}
    </div>
  );
}

interface TabsTriggerProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'value'> {
  value: string;
  children: ReactNode;
}

export function TabsTrigger({ value, className, children, ...props }: TabsTriggerProps) {
  const { value: activeValue, onValueChange, variant } = useTabsContext();
  const active = activeValue === value;

  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      onClick={() => onValueChange(value)}
      className={cn(
        'relative shrink-0 px-4 py-2.5 text-[13px] font-medium transition-colors',
        variant === 'underline' &&
          cn(
            '-mb-px border-b-2',
            active
              ? 'border-primary font-semibold text-primary'
              : 'border-transparent text-text-mid hover:border-white/15 hover:text-text',
          ),
        variant === 'contained' &&
          cn(
            '-mb-px rounded-t-md border border-transparent',
            active
              ? 'z-10 border-white/8 border-b-transparent bg-bg-card/40 font-semibold text-primary'
              : 'text-text-mid hover:bg-white/3 hover:text-text',
          ),
        className,
      )}
      {...props}
    >
      {children}
    </button>
  );
}

interface TabsPanelProps extends HTMLAttributes<HTMLDivElement> {
  value: string;
  children: ReactNode;
}

export function TabsPanel({ value, className, children, ...props }: TabsPanelProps) {
  const { value: activeValue, variant } = useTabsContext();

  if (activeValue !== value) return null;

  return (
    <div
      role="tabpanel"
      className={cn(variant === 'underline' ? 'pt-4' : 'pt-3', className)}
      {...props}
    >
      {children}
    </div>
  );
}
