type Props = {
  label: string;
  active: boolean;
  sortAsc: boolean;
  onSort: () => void;
};

/** Sortable table column header (keyboard + aria-sort). */
export default function SortableTh({ label, active, sortAsc, onSort }: Props) {
  const ariaSort = active ? (sortAsc ? "ascending" : "descending") : "none";
  const sortHint = active ? (sortAsc ? ", sorted ascending" : ", sorted descending") : "";

  return (
    <th scope="col" aria-sort={ariaSort}>
      <button
        type="button"
        className="sort-header"
        onClick={onSort}
        aria-label={`Sort by ${label}${sortHint}`}
      >
        {label}
        {active && (
          <span className="sort-indicator" aria-hidden="true">
            {sortAsc ? " ↑" : " ↓"}
          </span>
        )}
      </button>
    </th>
  );
}
