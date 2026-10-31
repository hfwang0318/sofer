export function Logo({ size = 24 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 32 32"
      fill="none"
      aria-hidden="true"
      className="logo-mark"
    >
      <rect
        x="1"
        y="1"
        width="30"
        height="30"
        rx="8"
        fill="rgba(255,255,255,0.035)"
        stroke="rgba(255,255,255,0.12)"
      />
      <g stroke="var(--accent)" strokeWidth="2.1" strokeLinecap="round">
        <path d="M9.5 13V10.5a1.5 1.5 0 0 1 1.5-1.5h2.5" />
        <path d="M18.5 9H21a1.5 1.5 0 0 1 1.5 1.5V13" />
        <path d="M22.5 19v2.5a1.5 1.5 0 0 1-1.5 1.5h-2.5" />
        <path d="M13.5 23H11a1.5 1.5 0 0 1-1.5-1.5V19" />
      </g>
      <g fill="rgba(240,242,238,0.9)">
        <rect x="12" y="14" width="8" height="1.7" rx="0.85" />
        <rect x="12" y="17.3" width="5" height="1.7" rx="0.85" />
      </g>
    </svg>
  );
}
