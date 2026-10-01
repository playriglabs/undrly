import type { v1 } from "@undrly/contracts";

/** A square tile with a single-stroke glyph per asset class (no third-party logos). */
export function ClassIcon({ subject, size = 36 }: { subject: v1.PriceSubjectV1; size?: number }) {
  const cls = subject.kind === "instrument" ? subject.class : "currency";
  return (
    <span
      className="flex shrink-0 items-center justify-center border border-line-strong bg-card text-forest"
      style={{ width: size, height: size }}
      aria-hidden="true"
    >
      {/* The glyph fills 60% of the tile: 22px in the table, 31px on a detail page. */}
      <span style={{ width: Math.round(size * 0.6), height: Math.round(size * 0.6) }}>
        <ClassGlyph cls={cls} className="size-full" />
      </span>
    </span>
  );
}

/** The perpetual mark: a loop that never expires, around a price line. */
const PERPETUAL =
  "M19.5 12a7.5 7.5 0 0 1-13.4 4.6M4.5 12a7.5 7.5 0 0 1 13.4-4.6M18.2 3.8v3.9h-3.9M5.8 20.2v-3.9h3.9M9 13.5l2-2 1.6 1.6L15 10.5";

export function ClassGlyph({
  cls,
  className = "size-[18px]",
}: {
  cls: v1.InstrumentClass | "currency" | "all";
  className?: string;
}) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {cls === "all" ? <path d="M4 4v16h16M8 15l3-4 3 2 5-6" /> : null}
      {cls === "equity" ? (
        <path d="M4 20h16M6 20V10M10 20V10M14 20V10M18 20V10M3 9l9-5 9 5Z" />
      ) : null}
      {cls === "perpetual_future" ? <path d={PERPETUAL} /> : null}
      {cls === "crypto_asset" ? (
        <>
          <circle cx="12" cy="12" r="8.5" />
          <path d="M10 8.5v7M10 8.5h2.5a1.75 1.75 0 0 1 0 3.5H10M10 12h3a1.75 1.75 0 0 1 0 3.5h-3M11 7v1.5M11 15.5V17" />
        </>
      ) : null}
      {cls === "fx" || cls === "currency" ? <path d="M5 8h13l-3-3M19 16H6l3 3" /> : null}
      {cls === "commodity" ? (
        <path d="M2.5 19.5 5 13.5h8.5l2.5 6ZM5 13.5l1.5-2.5h5.5l1.5 2.5M19 4.5s-3 3.9-3 6a3 3 0 0 0 6 0c0-2.1-3-6-3-6Z" />
      ) : null}
      {cls === "tokenized_security" ? (
        <path d="M12 3l8 4.5v9L12 21l-8-4.5v-9ZM12 12l8-4.5M12 12v9M12 12 4 7.5" />
      ) : null}
    </svg>
  );
}
