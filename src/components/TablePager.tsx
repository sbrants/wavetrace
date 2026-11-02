import { TABLE_PAGE_SIZE } from "../tablePage";

export default function TablePager({
  page,
  totalPages,
  totalRows,
  setPage,
}: {
  page: number;
  totalPages: number;
  totalRows: number;
  setPage: (page: number) => void;
}) {
  if (totalPages <= 1) return null;
  const first = (page - 1) * TABLE_PAGE_SIZE + 1;
  const last = Math.min(page * TABLE_PAGE_SIZE, totalRows);
  return (
    <div className="history-pagination">
      <span className="muted">
        Showing {first}–{last} of {totalRows}
      </span>
      <div className="history-pagination-controls">
        <button type="button" disabled={page <= 1} onClick={() => setPage(page - 1)}>
          Previous
        </button>
        <span className="page-indicator">
          Page {page} of {totalPages}
        </span>
        <button
          type="button"
          disabled={page >= totalPages}
          onClick={() => setPage(page + 1)}
        >
          Next
        </button>
      </div>
    </div>
  );
}
