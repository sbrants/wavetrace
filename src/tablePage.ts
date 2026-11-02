import { useCallback, useEffect, useMemo, useRef, useState } from "react";

/** Rows rendered per page in History's run-detail tables. Their row count grows for as
 *  long as a run lasts, and a long run reached tens of thousands of DOM elements. */
export const TABLE_PAGE_SIZE = 100;

export function pageCount(rowCount: number, pageSize = TABLE_PAGE_SIZE): number {
  return Math.max(1, Math.ceil(rowCount / pageSize));
}

/** 1-based page holding the row at `index`. */
export function pageOfIndex(index: number, pageSize = TABLE_PAGE_SIZE): number {
  return Math.floor(index / pageSize) + 1;
}

/**
 * One page of `rows`, plus `reveal(id)` to bring a row on any page into view (a chart click
 * can select a row that isn't currently rendered). Returns to page 1 when `resetKey` changes.
 */
export function useTablePage<T extends { id: string }>(rows: T[], resetKey: unknown) {
  const [page, setPage] = useState(1);
  const [revealId, setRevealId] = useState<string | null>(null);
  const rowsRef = useRef(rows);
  rowsRef.current = rows;
  const wrapRef = useRef<HTMLDivElement>(null);
  const rowEls = useRef(new Map<string, HTMLTableRowElement>());

  const totalPages = pageCount(rows.length);
  const safePage = Math.min(page, totalPages);
  const start = (safePage - 1) * TABLE_PAGE_SIZE;
  const pageRows = useMemo(
    () => rows.slice(start, start + TABLE_PAGE_SIZE),
    [rows, start]
  );

  useEffect(() => {
    setPage(1);
  }, [resetKey]);

  useEffect(() => {
    wrapRef.current?.scrollTo({ top: 0 });
  }, [safePage]);

  // After the page-change effect above, and after commit so the row is mounted.
  useEffect(() => {
    if (revealId === null) return;
    // Scroll the table's own box only: scrollIntoView also scrolls the page, which took
    // the chart off screen on every point clicked in it.
    const wrap = wrapRef.current;
    const row = rowEls.current.get(revealId);
    if (wrap && row) {
      const w = wrap.getBoundingClientRect();
      const r = row.getBoundingClientRect();
      if (r.top < w.top || r.bottom > w.bottom) {
        wrap.scrollBy({
          top: r.top - w.top - (w.height - r.height) / 2,
          behavior: "smooth",
        });
      }
    }
    setRevealId(null);
  }, [revealId, safePage]);

  const reveal = useCallback((id: string) => {
    const index = rowsRef.current.findIndex((r) => r.id === id);
    if (index < 0) return;
    setPage(pageOfIndex(index));
    setRevealId(id);
  }, []);

  const rowRef = useCallback(
    (id: string) => (el: HTMLTableRowElement | null) => {
      if (el) rowEls.current.set(id, el);
      else rowEls.current.delete(id);
    },
    []
  );

  return {
    pageRows,
    page: safePage,
    totalPages,
    totalRows: rows.length,
    setPage,
    wrapRef,
    rowRef,
    reveal,
  };
}
