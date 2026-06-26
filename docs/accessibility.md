# Accessibility roadmap

WaveTrace is a desktop Tauri app with a React UI. This document tracks accessibility
work in phases. **Phases A and B are implemented** (v0.2.24+); C–E are planned.

## Current status (after A + B)

- **Keyboard:** Tab order follows visual layout; sortable History columns use real
  buttons with `aria-sort`; `:focus-visible` rings on interactive controls.
- **Screen readers:** Scanner status, coin warning, update banner, export/backup/saved
  messages use `role="status"` / `role="alert"` and `aria-live` where content changes.
  Main nav uses `aria-current="page"`.
- **Forms:** History filters, Settings window/backup controls, and scanner log toolbar
  have visible or visually hidden labels; Background and Backup use `<fieldset>` /
  `<legend>`.
- **Lint:** `eslint-plugin-jsx-a11y` (`recommended`) on `src/` via `npm run lint`.

### Known limits

- **Charts (Recharts):** SVG line charts are not fully accessible. Data is visual-only
  unless a table fallback is added (Phase C). Tooltips are mouse/hover-oriented.
- **History table rows:** Row selection is click-driven; no roving `tabIndex` on rows
  yet.
- **Not audited:** No formal WCAG 2.2 AA sign-off; contrast and motion preferences
  are only partially addressed (Phase D).

---

## Phase A — Foundations (done)

| Item | Implementation |
| ---- | -------------- |
| Focus visibility | Global `:focus-visible` in `src/styles.css` |
| Live regions | Header scanner status; coin warning `role="alert"`; updater banner |
| Navigation | `aria-current="page"` on active tab |
| Tooling | `eslint.config.js` + `eslint-plugin-jsx-a11y` |

---

## Phase B — Forms & tables (done)

| Area | Implementation |
| ---- | -------------- |
| History sort | `SortableTh` — button in `<th>`, `aria-sort`, `scope="col"` |
| History filters | Labeled run type, min wave/tier, date range |
| Settings | `htmlFor`/`id` on window select and title; fieldsets for Background & Backup |
| Scanner log | Labeled lines select and search; `aria-label` on log `<pre>` |
| Dashboard | `role="group"` on live stat cards |

---

## Phase C — Chart data fallback (planned)

**Goal:** Users who cannot use the chart still get the underlying numbers.

1. **Collapsible data table** under each `CoinVsWaveChart` (Dashboard, History single
   run, compare view): columns e.g. snapshot index, wave, coin/min, optional skip
   markers on matching rows.
2. **Toggle** “Show chart data table” (default collapsed to avoid clutter).
3. **Compare mode:** one table per run or a wide table with run color / name column.
4. **Document** in `Goal.md` that charts are supplementary; table is the accessible
   source of truth for numeric series.
5. **Optional:** `aria-describedby` from chart card heading to the table region when
   expanded.

**Non-goals:** Recharts keyboard navigation inside the SVG (low ROI vs table).

---

## Phase D — Visual & motion (planned)

1. **Contrast pass** — audit `--muted`, warning banner, badge, and chart line colors
   against dark background (target WCAG 2.2 AA for text and UI components).
2. **`prefers-reduced-motion`** — extend beyond global CSS throttle: disable or shorten
   Recharts animations if any are added later; respect reduced motion for live “pulse”
   indicators if introduced.
3. **Optional chart keyboard nav** — only if table fallback is insufficient: focusable
   legend items, arrow keys between data points (custom layer or replace chart lib).

---

## Phase E — Process & release (planned)

1. **`Goal.md` “Accessibility”** section — link here; state target (pragmatic AA for
   core flows, not certified).
2. **Manual release checklist** (add to maintainer notes or `CONTRIBUTING.md`):
   - Tab through Dashboard, History, Settings without traps
   - NVDA/VoiceOver: scanner status announces; warning banner reads once
   - Sort columns with keyboard; filters have audible names
   - Save settings / backup status announced
3. **Regression:** run `npm run lint` in CI when frontend CI exists.
4. **User feedback** channel for a11y issues (GitHub label `accessibility`).

---

## References

- [WCAG 2.2](https://www.w3.org/TR/WCAG22/)
- [WAI-ARIA APG — Sortable table](https://www.w3.org/WAI/ARIA/apg/patterns/table/examples/sortable-table/)
- [eslint-plugin-jsx-a11y](https://github.com/jsx-eslint/eslint-plugin-jsx-a11y)
