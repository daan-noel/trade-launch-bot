import { useEffect, useState } from 'react';
import { cn } from '../../lib/cn';

export const DEFAULT_PAGE_SIZE = 10;

interface PaginationProps {
  currentPage: number;
  totalPages: number;
  totalItems: number;
  pageSize?: number;
  pageSizeOptions?: number[];
  onPageChange: (page: number) => void;
  onPageSizeChange: (size: number) => void;
}

function buildPageButtons(current: number, total: number): number[] {
  if (total <= 9) return Array.from({ length: total }, (_, i) => i + 1);
  if (current <= 5) return [...Array.from({ length: 6 }, (_, i) => i + 1), 0, total];
  if (current + 4 >= total) {
    return [1, 0, ...Array.from({ length: 6 }, (_, i) => total - 5 + i)];
  }
  return [1, 0, current - 1, current, current + 1, 0, total];
}

function ChevronIcon({ direction }: { direction: 'left' | 'right' }) {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 14 14"
      fill="none"
      aria-hidden
      className={direction === 'right' ? 'rotate-180' : undefined}
    >
      <path
        d="M8.5 10.5L5 7l3.5-3.5"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

const navBtn =
  'inline-flex h-8 w-8 shrink-0 items-center justify-center text-text-dim transition-colors hover:bg-bg-hover hover:text-text disabled:pointer-events-none disabled:opacity-35';
const pageBtn =
  'inline-flex h-8 min-w-8 shrink-0 items-center justify-center px-1.5 text-[13px] font-medium tabular-nums transition-colors';

export function Pagination({
  currentPage,
  totalPages,
  totalItems,
  pageSize = DEFAULT_PAGE_SIZE,
  pageSizeOptions = [5, 10, 25, 50, 100],
  onPageChange,
  onPageSizeChange,
}: PaginationProps) {
  const [pageInput, setPageInput] = useState(String(currentPage));
  const buttons = buildPageButtons(currentPage, totalPages);
  const rangeStart = totalItems === 0 ? 0 : (currentPage - 1) * pageSize + 1;
  const rangeEnd = totalItems === 0 ? 0 : Math.min(currentPage * pageSize, totalItems);

  useEffect(() => {
    setPageInput(String(currentPage));
  }, [currentPage]);

  function commitPageInput() {
    const parsed = parseInt(pageInput, 10);
    if (!Number.isFinite(parsed)) {
      setPageInput(String(currentPage));
      return;
    }
    const page = Math.min(totalPages, Math.max(1, parsed));
    setPageInput(String(page));
    if (page !== currentPage) onPageChange(page);
  }

  return (
    <div className="mt-1 flex flex-wrap items-center justify-between gap-3 rounded-lg border border-border bg-bg-panel px-3 py-1.5">
      <p className="min-w-0 flex-1 text-xs text-text-dim">
        {totalItems === 0 ? (
          'No results'
        ) : (
          <>
            Showing{' '}
            <span className="font-medium tabular-nums text-text">
              {rangeStart}–{rangeEnd}
            </span>{' '}
            of{' '}
            <span className="font-medium tabular-nums text-text">{totalItems}</span>
            {totalPages > 1 && (
              <span className="text-text-mid">
                {' '}
                · page {currentPage} of {totalPages}
              </span>
            )}
          </>
        )}
      </p>

      {totalPages > 1 && (
        <nav
          className="inline-flex items-center overflow-hidden rounded-md border border-border bg-bg-card"
          aria-label="Pagination"
        >
          <button
            type="button"
            disabled={currentPage <= 1}
            onClick={() => onPageChange(currentPage - 1)}
            className={cn(navBtn, 'border-r border-border')}
            aria-label="Previous page"
          >
            <ChevronIcon direction="left" />
          </button>

          {buttons.map((page, i) =>
            page === 0 ? (
              <span
                key={`e-${i}`}
                className="inline-flex h-8 w-7 items-center justify-center border-r border-border text-xs tracking-widest text-text-dim select-none"
                aria-hidden
              >
                ···
              </span>
            ) : (
              <button
                key={page}
                type="button"
                onClick={() => onPageChange(page)}
                aria-label={`Page ${page}`}
                aria-current={page === currentPage ? 'page' : undefined}
                className={cn(
                  pageBtn,
                  'border-r border-border last:border-r-0',
                  page === currentPage
                    ? 'bg-primary/15 text-primary'
                    : 'text-text-dim hover:bg-bg-hover hover:text-text',
                )}
              >
                {page}
              </button>
            ),
          )}

          <label className="inline-flex h-8 shrink-0 items-center gap-1 border-r border-border px-2 text-xs text-text-dim">
            <span className="sr-only">Go to page</span>
            <input
              type="number"
              min={1}
              max={totalPages}
              value={pageInput}
              onChange={(e) => setPageInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') e.currentTarget.blur();
              }}
              onBlur={commitPageInput}
              aria-label={`Page number, 1 to ${totalPages}`}
              className="h-6 w-11 rounded border border-border bg-bg-card px-1 text-center text-[13px] font-medium tabular-nums text-text transition-colors [appearance:textfield] hover:border-white/20 focus:border-primary/40 focus:outline-none focus:ring-1 focus:ring-primary/25 [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none"
            />
            <span className="tabular-nums">/ {totalPages}</span>
          </label>

          <button
            type="button"
            disabled={currentPage >= totalPages}
            onClick={() => onPageChange(currentPage + 1)}
            className={navBtn}
            aria-label="Next page"
          >
            <ChevronIcon direction="right" />
          </button>
        </nav>
      )}

      <label className="flex shrink-0 items-center gap-2 text-xs text-text-dim">
        <span className="hidden sm:inline">Rows per page</span>
        <span className="sm:hidden">Rows</span>
        <span className="relative inline-flex">
          <select
            value={pageSize}
            onChange={(e) => onPageSizeChange(Number(e.target.value))}
            aria-label="Rows per page"
            className="h-8 cursor-pointer appearance-none rounded-md border border-border bg-bg-card py-0 pr-7 pl-2.5 text-xs font-medium text-text transition-colors hover:border-white/20 focus:border-primary/40 focus:outline-none focus:ring-1 focus:ring-primary/25"
          >
            {pageSizeOptions.map((size) => (
              <option key={size} value={size}>
                {size}
              </option>
            ))}
          </select>
          <svg
            className="pointer-events-none absolute top-1/2 right-2 -translate-y-1/2 text-text-dim"
            width="10"
            height="10"
            viewBox="0 0 10 10"
            fill="none"
            aria-hidden
          >
            <path
              d="M2.5 4L5 6.5 7.5 4"
              stroke="currentColor"
              strokeWidth="1.25"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        </span>
      </label>
    </div>
  );
}
