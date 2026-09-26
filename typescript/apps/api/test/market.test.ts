import { describe, expect, it } from "vitest";
import { ageMs, meanVenueMid } from "../src/market.ts";

describe("meanVenueMid matches undrly_core::quote (same vectors as its tests)", () => {
  it("two inputs, exact", () => {
    // mids 84145.950000 and 83984.365; mean at scale 7.
    expect(
      meanVenueMid([
        { bid: "84145.90000", ask: "84146.00000" },
        { bid: "83984.36", ask: "83984.37" },
      ]),
    ).toBe("84065.1575000");
    expect(
      meanVenueMid([
        { bid: "1.1819", ask: "1.1821" },
        { bid: "1.1803", ask: "1.1805" },
      ]),
    ).toBe("1.181200");
  });

  it("is independent of input order", () => {
    const a = { bid: "84145.90000", ask: "84146.00000" };
    const b = { bid: "84006.45", ask: "84006.46" };
    expect(meanVenueMid([a, b])).toBe(meanVenueMid([b, a]));
    expect(meanVenueMid([a, b])).toBe("84076.2025000");
  });

  it("one input is its own mid, at the aggregate's scale", () => {
    expect(meanVenueMid([{ bid: "100.0", ask: "100.2" }])).toBe("100.100");
  });

  it("three inputs round half to even", () => {
    expect(
      meanVenueMid([
        { bid: "0.01", ask: "0.02" },
        { bid: "0.01", ask: "0.01" },
        { bid: "0.02", ask: "0.02" },
      ]),
    ).toBe("0.0150");
  });

  it("nothing to average", () => {
    expect(meanVenueMid([])).toBeNull();
    expect(meanVenueMid([{ bid: "x", ask: "1" }])).toBeNull();
  });
});

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
