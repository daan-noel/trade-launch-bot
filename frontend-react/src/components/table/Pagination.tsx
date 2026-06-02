import { cn } from '../../lib/cn';

interface PaginationProps {
  currentPage: number;
  totalPages: number;
  totalItems: number;
  pageSize: number;
  pageSizeOptions: number[];
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

export function Pagination({
  currentPage,
  totalPages,
  totalItems,
  pageSize,
  pageSizeOptions,
  onPageChange,
  onPageSizeChange,
}: PaginationProps) {
  const buttons = buildPageButtons(currentPage, totalPages);

  return (
    <div className="my-3 flex flex-wrap items-center justify-between gap-4 rounded-xl border border-primary/18 bg-[rgba(15,23,42,0.9)] px-4 py-1 shadow-[inset_0_0_0_1px_rgba(255,255,255,0.03)]">
      <span className="min-w-[120px] flex-1 font-mono text-xs text-text-mid">
        Page {currentPage} of {totalPages} • {totalItems} total
      </span>

      <div className="flex flex-wrap items-center gap-2">
        <button
          type="button"
          disabled={currentPage <= 1}
          onClick={() => onPageChange(currentPage - 1)}
          className="h-[34px] min-w-[34px] rounded-full bg-[rgba(15,23,42,0.8)] font-bold text-text transition hover:-translate-y-px hover:bg-primary/20 hover:text-primary disabled:cursor-default disabled:opacity-45"
        >
          ‹
        </button>
        {buttons.map((page, i) =>
          page === 0 ? (
            <span key={`e-${i}`} className="px-1.5 text-sm text-text-mid">
              ...
            </span>
          ) : (
            <button
              key={page}
              type="button"
              disabled={page === currentPage}
              onClick={() => onPageChange(page)}
              className={cn(
                'h-[34px] min-w-[34px] rounded-full font-bold transition hover:-translate-y-px hover:bg-primary/20 hover:text-primary disabled:cursor-default',
                page === currentPage
                  ? 'bg-[linear-gradient(135deg,rgba(56,189,248,0.22),rgba(56,189,248,0.1))] text-primary shadow-[0_0_0_1px_rgba(56,189,248,0.18)]'
                  : 'bg-[rgba(15,23,42,0.8)] text-text',
              )}
            >
              {page}
            </button>
          ),
        )}
        <button
          type="button"
          disabled={currentPage >= totalPages}
          onClick={() => onPageChange(currentPage + 1)}
          className="h-[34px] min-w-[34px] rounded-full bg-[rgba(15,23,42,0.8)] font-bold text-text transition hover:-translate-y-px hover:bg-primary/20 hover:text-primary disabled:cursor-default disabled:opacity-45"
        >
          ›
        </button>
      </div>

      <label className="flex items-center gap-2 rounded-full bg-white/4 px-3 py-2 text-xs text-text">
        Show
        <select
          value={pageSize}
          onChange={(e) => onPageSizeChange(Number(e.target.value))}
          className="cursor-pointer rounded border border-primary bg-bg-panel px-2 py-1 text-xs font-medium text-text outline-none"
        >
          {pageSizeOptions.map((size) => (
            <option key={size} value={size}>
              {size}
            </option>
          ))}
        </select>
      </label>
    </div>
  );
}
