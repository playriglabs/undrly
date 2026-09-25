import { describe, expect, it } from "vitest";
import { canonicalTimestamp, parseQuery } from "../src/query.ts";

describe("query syntax", () => {
  it("recognizes each form", () => {
    expect(parseQuery("undrly:instrument:01m3bbjhndfhhb0kbmejmgabc3")).toMatchObject({
      kind: "canonical_id",
      category: "instrument",
    });
    expect(parseQuery("isin:us67066g1040")).toStrictEqual({
      kind: "identifier",
      scheme: "isin",
      value: "US67066G1040",
    });
    expect(parseQuery("CIK:1045810")).toStrictEqual({
      kind: "identifier",
      scheme: "cik",
      value: "0001045810",
    });
    expect(parseQuery("KRAKEN:XXBTZUSD")).toStrictEqual({
      kind: "venue_symbol",
      venue: "KRAKEN",
      symbol: "XXBTZUSD",
    });
    expect(parseQuery(" EUR/USD ")).toStrictEqual({ kind: "pair", base: "EUR", quote: "USD" });
    expect(parseQuery("BTC perpetual")).toStrictEqual({ kind: "alias", text: "BTC perpetual" });
  });

  it("rejects empty or control-character queries", () => {
    expect(parseQuery("   ")).toBeNull();
    expect(parseQuery("BTC\n")).toStrictEqual({ kind: "alias", text: "BTC" });
    expect(parseQuery("B\u0000TC")).toBeNull();
  });
});

describe("timestamps", () => {
  it("formats PostgreSQL text as canonical Timestamp text", () => {
    expect(canonicalTimestamp("2026-09-25T03:51:57.000000")).toBe("2026-09-25T03:51:57Z");
    expect(canonicalTimestamp("2026-09-25T03:52:18.500000")).toBe("2026-09-25T03:52:18.500Z");
    expect(canonicalTimestamp("2026-09-25T04:13:17.520101")).toBe("2026-09-25T04:13:17.520101Z");
  });
});
