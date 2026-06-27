import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { NavLink } from 'react-router-dom';
import { cn } from 'lib/cn';

export type NavDropdownItem = {
  to: string;
  label: string;
};

type NavDropdownProps = {
  label: string;
  items: NavDropdownItem[];
  isActive?: boolean;
};

function ChevronDown({ open, className }: { open: boolean; className?: string }) {
  return (
    <svg
      viewBox="0 0 12 12"
      fill="none"
      aria-hidden
      className={cn(
        'size-3 opacity-50 transition-transform duration-150',
        open && 'rotate-180',
        className,
      )}
    >
      <path
        d="M3 4.5 6 7.5 9 4.5"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

const HOVER_CLOSE_MS = 180;
const MENU_GAP_PX = 4;
const MENU_OVERLAP_PX = 6;

type MenuPosition = { top: number; left: number };

export function NavDropdown({ label, items, isActive = false }: NavDropdownProps) {
  const menuId = useId();
  const triggerRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const closeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [open, setOpen] = useState(false);
  const [menuPos, setMenuPos] = useState<MenuPosition>({ top: 0, left: 0 });

  const clearCloseTimer = useCallback(() => {
    if (closeTimerRef.current != null) {
      clearTimeout(closeTimerRef.current);
      closeTimerRef.current = null;
    }
  }, []);

  const openMenu = useCallback(() => {
    clearCloseTimer();
    setOpen(true);
  }, [clearCloseTimer]);

  const scheduleClose = useCallback(() => {
    clearCloseTimer();
    closeTimerRef.current = setTimeout(() => setOpen(false), HOVER_CLOSE_MS);
  }, [clearCloseTimer]);

  const toggleMenu = useCallback(() => {
    clearCloseTimer();
    setOpen((prev) => !prev);
  }, [clearCloseTimer]);

  const updateMenuPosition = useCallback(() => {
    const el = triggerRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    setMenuPos({
      top: rect.bottom - MENU_OVERLAP_PX + MENU_GAP_PX,
      left: rect.left,
    });
  }, []);

  useLayoutEffect(() => {
    if (!open) return;
    updateMenuPosition();
    window.addEventListener('resize', updateMenuPosition);
    window.addEventListener('scroll', updateMenuPosition, true);
    return () => {
      window.removeEventListener('resize', updateMenuPosition);
      window.removeEventListener('scroll', updateMenuPosition, true);
    };
  }, [open, updateMenuPosition]);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (e: PointerEvent) => {
      const target = e.target as Node;
      if (triggerRef.current?.contains(target)) return;
      if (menuRef.current?.contains(target)) return;
      setOpen(false);
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false);
    };
    document.addEventListener('pointerdown', onPointerDown);
    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown);
      document.removeEventListener('keydown', onKeyDown);
    };
  }, [open]);

  useEffect(() => () => clearCloseTimer(), [clearCloseTimer]);

  const menu = open
    ? createPortal(
        <div
          ref={menuRef}
          id={menuId}
          role="menu"
          aria-label={label}
          style={{ top: menuPos.top, left: menuPos.left }}
          onMouseEnter={openMenu}
          onMouseLeave={scheduleClose}
          className="fixed z-[200] pt-1.5"
        >
          <div className="flex min-w-[148px] flex-col gap-0.5 rounded-lg border border-white/8 bg-bg-panel p-1 shadow-[0_12px_32px_rgba(0,0,0,0.5)]">
            {items.map((item) => (
              <NavLink
                key={item.to}
                to={item.to}
                role="menuitem"
                onClick={() => setOpen(false)}
                className={({ isActive: itemActive }) =>
                  cn(
                    'rounded-md px-3 py-2 text-xs font-medium whitespace-nowrap text-text-mid transition-colors hover:bg-white/6 hover:text-text',
                    itemActive && 'bg-primary/10 text-primary',
                  )
                }
              >
                {item.label}
              </NavLink>
            ))}
          </div>
        </div>,
        document.body,
      )
    : null;

  return (
    <>
      <div
        ref={triggerRef}
        className="relative shrink-0"
        onMouseEnter={openMenu}
        onMouseLeave={scheduleClose}
      >
        <button
          type="button"
          aria-expanded={open}
          aria-haspopup="menu"
          aria-controls={menuId}
          onClick={toggleMenu}
          className={cn(
            'flex cursor-pointer items-center gap-1 rounded-md px-3 py-1.5 text-[13px] font-medium transition-all duration-150',
            isActive
              ? 'bg-primary/12 text-primary shadow-[inset_0_1px_0_rgba(19,206,175,0.15)]'
              : 'text-text-mid hover:bg-white/4 hover:text-text',
          )}
        >
          {label}
          <ChevronDown open={open} />
        </button>
      </div>
      {menu}
    </>
  );
}
