/** A 24-hour close line; colour follows first-to-last direction. Plotting only uses floats. */
export function Sparkline({ values }: { values: string[] }) {
  if (values.length < 2) {
    return (
      <span className="flex h-7 w-28 items-center justify-center text-[12px] text-faint">
        No data
      </span>
    );
  }
  const nums = values.map(Number);
  const min = Math.min(...nums);
  const span = Math.max(...nums) - min || 1;
  const points = nums
    .map((n, i) => `${(i / (nums.length - 1)) * 100},${28 - ((n - min) / span) * 24 - 2}`)
    .join(" ");
  const up = (nums.at(-1) ?? 0) >= (nums[0] ?? 0);
  return (
    <svg className="h-7 w-28" viewBox="0 0 100 28" preserveAspectRatio="none" aria-hidden="true">
      <polyline
        points={points}
        fill="none"
        stroke={up ? "var(--color-up)" : "var(--color-down)"}
        strokeWidth="1.25"
        vectorEffect="non-scaling-stroke"
        strokeLinejoin="round"
      />
    </svg>
  );
}
