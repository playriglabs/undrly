import { Avatar, Style } from "@dicebear/core";
import waves from "@dicebear/styles/waves.json" with { type: "json" };
import { useMemo } from "react";

/** One style instance, reused by every avatar (DiceBear's guidance). */
const style = new Style(waves);

/**
 * A DiceBear "waves" avatar, generated locally from the seed: the same
 * account always gets the same picture, and nothing is sent to DiceBear.
 */
export function UserAvatar({ seed, size = 40 }: { seed: string; size?: number }) {
  const src = useMemo(() => new Avatar(style, { seed, size }).toDataUri(), [seed, size]);
  return <img src={src} alt="" width={size} height={size} className="shrink-0 rounded-md" />;
}
