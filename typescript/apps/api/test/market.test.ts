import { describe, expect, it } from "vitest";
import { ageMs, policyElapsedMs } from "../src/market.ts";

describe("ageMs", () => {
  it("floors elapsed microseconds to whole milliseconds", () => {
    expect(ageMs("2026-09-25T07:28:06.386017Z", new Date("2026-09-25T07:28:10Z"))).toBe(3613);
    expect(ageMs("2026-09-25T07:28:06Z", new Date("2026-09-25T07:28:06.999Z"))).toBe(999);
    expect(ageMs("2026-09-25T07:28:06.500Z", new Date("2026-09-25T07:28:07.500Z"))).toBe(1000);
  });

  it("is never negative", () => {
    expect(ageMs("2026-09-25T07:28:10Z", new Date("2026-09-25T07:28:06Z"))).toBe(0);
    expect(ageMs("2026-09-25T07:28:06.386017Z", new Date("2026-09-25T07:28:06.386Z"))).toBe(0);
  });
});

describe("policyElapsedMs", () => {
  const at = (t: string) => new Date(t);
  it("continuous counts every millisecond; weekdays skip Saturday and Sunday (UTC)", () => {
    const fri = at("2026-09-25T12:00:00Z");
    const mon = at("2026-09-28T12:00:00Z");
    expect(policyElapsedMs(fri, mon, "continuous")).toBe(72 * 3_600_000);
    expect(policyElapsedMs(fri, mon, "weekdays")).toBe(24 * 3_600_000);
    expect(policyElapsedMs(mon, fri, "weekdays")).toBe(0);
  });
});
