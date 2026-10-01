import { Avatar, Style } from "@dicebear/core";
import glass from "@dicebear/styles/glass.json" with { type: "json" };
import { useMemo } from "react";

/** One style instance, reused by every avatar (DiceBear's guidance). */
const style = new Style(glass);

/**
 * A DiceBear "glass" avatar, generated locally from the seed: the same
 * account always gets the same picture, and nothing is sent to DiceBear.
 */
export function UserAvatar({ seed, size = 36 }: { seed: string; size?: number }) {
  const src = useMemo(() => new Avatar(style, { seed, size }).toDataUri(), [seed, size]);
  return <img src={src} alt="" width={size} height={size} className="shrink-0 rounded-full" />;
}
