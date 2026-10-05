import { describe, expect, it } from "vitest";
import { canonicalTimestamp, classShareSymbol, parseQuery } from "../src/query.ts";

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

  it("reads `Crypto.BASE/QUOTE` as the pair BASE/QUOTE (V1.10)", () => {
    expect(parseQuery("Crypto.AAPLX/USD")).toStrictEqual({
      kind: "pair",
      base: "AAPLX",
      quote: "USD",
    });
    expect(parseQuery("crypto.AAPL.US/USD")).toStrictEqual({
      kind: "pair",
      base: "AAPL.US",
      quote: "USD",
    });
    // Only the one prefix; anything else with a dot stays as it was.
    expect(parseQuery("Equity.AAPL/USD")).toStrictEqual({
      kind: "pair",
      base: "Equity.AAPL",
      quote: "USD",
    });
  });

  it("parses CAIP-2 chains and CAIP-19 deployments (V1.4)", () => {
    expect(parseQuery("caip2:eip155:8453")).toStrictEqual({
      kind: "chain",
      caip2: { namespace: "eip155", reference: "8453" },
    });
    const address = "0x6b175474e89094c44da98b954eedeac495271d0f";
    const upper = `caip19:eip155:1/erc20:0x${address.slice(2).toUpperCase()}`;
    // EVM hex is compared in lowercase; the chain stays part of the key.
    expect(parseQuery(upper)).toStrictEqual({
      kind: "deployment",
      caip19: {
        chain: { namespace: "eip155", reference: "1" },
        assetNamespace: "erc20",
        assetReference: address,
      },
    });
    // Base58 (Solana) is case-sensitive and kept exactly.
    const mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    expect(
      parseQuery(`caip19:solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp/token:${mint}`),
    ).toMatchObject({ kind: "deployment", caip19: { assetReference: mint } });
    // Malformed CAIP text is invalid, never a venue symbol.
    for (const bad of ["caip2:eip155", "caip19:eip155:1", "caip19:eip155:1/erc20", "caip2:E:1"]) {
      expect(parseQuery(bad), bad).toBeNull();
    }
    // Without a prefix, a bare chain id stays a venue-symbol query.
    expect(parseQuery("eip155:1")).toMatchObject({ kind: "venue_symbol" });
  });

  it("rejects empty or control-character queries", () => {
    expect(parseQuery("   ")).toBeNull();
    expect(parseQuery("BTC\n")).toStrictEqual({ kind: "alias", text: "BTC" });
    expect(parseQuery("B\u0000TC")).toBeNull();
  });
});

describe("class-share punctuation", () => {
  it("maps one hyphenated share class to the dotted listing spelling, nothing else", () => {
    expect(classShareSymbol("BRK-B")).toBe("BRK.B");
    expect(classShareSymbol("bf-b")).toBe("bf.b");
    for (const s of [
      "BRK.B",
      "BRKB",
      "BTC-USD",
      "BTC-PERP",
      "HENRY-HUB",
      "A-",
      "-B",
      "TOOLONG-B",
      "B1-B",
    ]) {
      expect(classShareSymbol(s), s).toBeNull();
    }
  });
});

describe("timestamps", () => {
  it("formats PostgreSQL text as canonical Timestamp text", () => {
    expect(canonicalTimestamp("2026-09-25T03:51:57.000000")).toBe("2026-09-25T03:51:57Z");
    expect(canonicalTimestamp("2026-09-25T03:52:18.500000")).toBe("2026-09-25T03:52:18.500Z");
    expect(canonicalTimestamp("2026-09-25T04:13:17.520101")).toBe("2026-09-25T04:13:17.520101Z");
  });
});
