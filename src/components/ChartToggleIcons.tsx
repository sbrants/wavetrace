/** Icons for the chart display-toggle buttons shared by Dashboard and
 * History (single-run and compare views). Several of these intentionally
 * use real colors instead of `currentColor` to match the game's own badge
 * art, so they stay recognizable regardless of the button's active/hover
 * state. */

/** Matches the game's gold "Coin" ring badge — a double ring with a C. */
export function CoinIcon() {
  return (
    <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
      <circle cx="12" cy="12" r="10.5" fill="#16203a" stroke="#e8c86a" strokeWidth="1" />
      <circle cx="12" cy="12" r="8.5" fill="none" stroke="#e8c86a" strokeWidth="1.8" />
      <text
        x="12"
        y="16.3"
        textAnchor="middle"
        fontSize="11"
        fontWeight="800"
        fontFamily="Georgia, 'Times New Roman', serif"
        fill="#e8c86a"
      >
        C
      </text>
    </svg>
  );
}

/** Matches the game's "Wave Skip" badge — a rounded navy square with a
 * dashed cycle arc and a down chevron. */
export function WaveJumpIcon() {
  return (
    <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
      <rect
        x="2"
        y="2"
        width="20"
        height="20"
        rx="6"
        fill="#123a63"
        stroke="#4fc3f7"
        strokeWidth="1.5"
      />
      <path
        d="M6.5 9a6 6 0 0 1 11 2"
        fill="none"
        stroke="#ffffff"
        strokeWidth="1.6"
        strokeLinecap="round"
        strokeDasharray="2.2 2.4"
      />
      <path
        d="M8.5 12.5 12 16l3.5-3.5"
        fill="none"
        stroke="#ffffff"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

/** Matches the game's glowing "Golden Combo" wheel — a spoked ring on a
 * dark badge. */
export function GcActivationIcon() {
  return (
    <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
      <circle cx="12" cy="12" r="10" fill="#0f1830" stroke="#4fd1e8" strokeWidth="1.5" />
      <circle cx="12" cy="12" r="4" fill="none" stroke="#4fd1e8" strokeWidth="1.3" />
      <g stroke="#4fd1e8" strokeWidth="1.2" strokeLinecap="round">
        <line x1="12" y1="2.5" x2="12" y2="7.2" />
        <line x1="12" y1="16.8" x2="12" y2="21.5" />
        <line x1="2.5" y1="12" x2="7.2" y2="12" />
        <line x1="16.8" y1="12" x2="21.5" y2="12" />
        <line x1="5.2" y1="5.2" x2="8.6" y2="8.6" />
        <line x1="15.4" y1="15.4" x2="18.8" y2="18.8" />
        <line x1="18.8" y1="5.2" x2="15.4" y2="8.6" />
        <line x1="8.6" y1="15.4" x2="5.2" y2="18.8" />
      </g>
    </svg>
  );
}

/** A fluctuating pulse line — used for the Lead/lag band toggle. */
export function LeadLagIcon() {
  return (
    <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
      <path
        d="M3 12h4l3 8 4-16 3 8h4"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

/** A pin with a tracking arc — used for the "Follow new run" toggle. */
export function FollowRunIcon() {
  return (
    <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
      <path
        d="M12 21s-6.5-5.6-6.5-11A6.5 6.5 0 0 1 18.5 10c0 5.4-6.5 11-6.5 11z"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinejoin="round"
      />
      <circle cx="12" cy="10" r="2.2" fill="currentColor" />
      <path
        d="M19.5 5.5a9 9 0 0 1 1.8 5"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinecap="round"
      />
      <path
        d="M21.6 9 22 11l-2-1.1"
        fill="currentColor"
        stroke="currentColor"
        strokeWidth="1.2"
        strokeLinejoin="round"
      />
    </svg>
  );
}

/** A rewind-to-previous glyph — used for the "vs. previous run" toggle. */
export function PreviousRunIcon() {
  return (
    <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
      <path
        d="M5 5v14"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
      />
      <path
        d="M19 6.5v11a1 1 0 0 1-1.55.84L9 12.84a1 1 0 0 1 0-1.68l8.45-5.5A1 1 0 0 1 19 6.5z"
        fill="currentColor"
      />
    </svg>
  );
}
