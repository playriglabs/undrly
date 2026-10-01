/**
 * External API contract v1, plus the TypeScript side of the shared-fixture
 * drift guard (`tests/fixtures/shared`, also tested by Rust).
 */
import { readdirSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { v1 } from "../src/index.ts";

const FIXTURES = new URL("../../../../tests/fixtures/", import.meta.url);

function readText(path: string): string {
  return readFileSync(new URL(path, FIXTURES), "utf8");
}

function readJson(path: string): unknown {
  return JSON.parse(readText(path));
}

type Cases = { valid: string[]; invalid: string[] };
type IdCase = { category: v1.Category; uuid: string; text: string };
const primitives = readJson("shared/primitives.json") as {
  canonicalIds: { valid: IdCase[]; invalid: string[] };
  decimals: Cases;
  timestamps: Cases;
  sourceIds: Cases;
};
const vocabulary = readJson("shared/vocabulary.json") as {
  categories: string[];
  relationshipTypes: string[];
  relationshipRules: [string, string, string][];
  observationBases: string[];
  instrumentClasses: string[];
  priceTypes: string[];
  unitsOfMeasure: string[];
  universeKeys: string[];
  aggregationMethods: string[];
};

describe("API v1 valid documents", () => {
  const cases = [
    ["api/v1/market-observation.venue.json", v1.MarketObservationV1],
    ["api/v1/market-observation.aggregated.json", v1.MarketObservationV1],
    ["api/v1/market-observation.asset-unit.json", v1.MarketObservationV1],
    ["api/v1/market-observation.asset-unit-no-code.json", v1.MarketObservationV1],
    ["api/v1/relationship.issued-by.json", v1.RelationshipV1],
    ["api/v1/quote.perpetual-usdc.json", v1.QuoteV1],
    ["api/v1/quote.fx.json", v1.QuoteV1],
    ["api/v1/quote.btc-usd-aggregated.json", v1.QuoteV1],
    ["api/v1/quote.xau-reference.json", v1.QuoteV1],
    ["api/v1/quote.fx-pair-aggregated.json", v1.QuoteV1],
    ["api/v1/quote.fx-reference.json", v1.QuoteV1],
    ["api/v1/quote.perpetual-mark-with-book.json", v1.QuoteV1],
    ["api/v1/graph.deployments-markets.json", v1.GraphV1],
    ["api/v1/explain.perpetual.json", v1.ExplainV1],
    ["api/v1/explain.deployment.json", v1.ExplainV1],
    ["api/v1/explain.tracked-by.json", v1.ExplainV1],
  ] as const;

  for (const [path, schema] of cases) {
    it(`accepts ${path} without altering it`, () => {
      const text = readText(path);
      const parsed = schema.parse(JSON.parse(text));
      expect(parsed).toStrictEqual(JSON.parse(text));
      expect(`${JSON.stringify(parsed, null, 2)}\n`).toBe(text);
    });
  }

  it("keeps the price as an exact decimal string", () => {
    const o = v1.MarketObservationV1.parse(readJson("api/v1/market-observation.venue.json"));
    expect(o.price).toBe("183.4200");
    expect(typeof o.price).toBe("string");
  });

  it("identifies units canonically; the code is display only", () => {
    const currency = v1.MarketObservationV1.parse(readJson("api/v1/market-observation.venue.json"));
    expect(currency.unit.kind).toBe("currency");
    expect(v1.canonicalIdCategory(currency.unit.id)).toBe("currency");
    const asset = v1.MarketObservationV1.parse(
      readJson("api/v1/market-observation.asset-unit.json"),
    );
    expect(asset.unit.kind).toBe("asset");
    expect(v1.canonicalIdCategory(asset.unit.id)).toBe("instrument");
    expect(asset.unit.code).toBe("USDC");
  });
});

describe("API v1 invalid documents", () => {
  const names = readdirSync(new URL("api/v1/invalid/", FIXTURES)).filter((n) =>
    n.endsWith(".json"),
  );

  it("has fixtures to check", () => {
    expect(names.length).toBeGreaterThan(0);
  });

  for (const name of names) {
    it(`rejects ${name}`, () => {
      const schema = name.startsWith("market-observation.")
        ? v1.MarketObservationV1
        : name.startsWith("relationship.")
          ? v1.RelationshipV1
          : name.startsWith("quote.")
            ? v1.QuoteV1
            : name.startsWith("graph.")
              ? v1.GraphV1
              : name.startsWith("explain.")
                ? v1.ExplainV1
                : undefined;
      expect(schema, `unrecognized fixture ${name}`).toBeDefined();
      expect(schema?.safeParse(readJson(`api/v1/invalid/${name}`)).success).toBe(false);
    });
  }
});

describe("shared primitives agree with Rust", () => {
  for (const { category, uuid, text } of primitives.canonicalIds.valid) {
    it(`formats and parses ${text}`, () => {
      expect(v1.formatCanonicalId(category, uuid)).toBe(text);
      expect(v1.canonicalIdCategory(text)).toBe(category);
    });
  }
  for (const text of primitives.canonicalIds.invalid) {
    it(`rejects id ${JSON.stringify(text)}`, () => {
      expect(v1.canonicalIdCategory(text)).toBeNull();
    });
  }

  it("refuses to format a non-v7 uuid", () => {
    expect(() => v1.formatCanonicalId("entity", "4f1c2d3e-0000-4000-8000-000000000009")).toThrow();
  });

  const validators: Record<"decimals" | "timestamps" | "sourceIds", (value: string) => boolean> = {
    decimals: (value) => v1.DecimalString.safeParse(value).success,
    timestamps: (value) => v1.TimestampString.safeParse(value).success,
    sourceIds: (value) => v1.SourceId.safeParse(value).success,
  };
  for (const [group, accepts] of Object.entries(validators)) {
    const cases = primitives[group as keyof typeof validators];
    for (const value of cases.valid) {
      it(`${group}: accepts ${JSON.stringify(value)}`, () => expect(accepts(value)).toBe(true));
    }
    for (const value of cases.invalid) {
      it(`${group}: rejects ${JSON.stringify(value)}`, () => expect(accepts(value)).toBe(false));
    }
  }

  it("rejects JSON numbers for decimals", () => {
    // Why decimals are strings: JSON.parse silently drops scale and precision.
    expect(JSON.parse('{"price":183.4200}').price).toBe(183.42);
    expect(v1.DecimalString.safeParse(183.42).success).toBe(false);
  });
});

describe("shared vocabulary agrees with Rust and the database", () => {
  it("categories", () => {
    expect([...v1.CATEGORIES]).toStrictEqual(vocabulary.categories);
  });

  it("relationship types and rules", () => {
    expect([...v1.RELATIONSHIP_TYPES]).toStrictEqual(vocabulary.relationshipTypes);
    const key = (rule: readonly string[]) => rule.join(" ");
    expect(v1.RELATIONSHIP_RULES.map(key).sort()).toStrictEqual(
      vocabulary.relationshipRules.map(key).sort(),
    );
  });

  it("observation bases", () => {
    expect([...v1.OBSERVATION_BASES].sort()).toStrictEqual([...vocabulary.observationBases].sort());
  });

  it("instrument classes, price types, aggregation methods", () => {
    expect([...v1.INSTRUMENT_CLASSES]).toStrictEqual(vocabulary.instrumentClasses);
    expect([...v1.PRICE_TYPES]).toStrictEqual(vocabulary.priceTypes);
    expect([...v1.UNITS_OF_MEASURE]).toStrictEqual(vocabulary.unitsOfMeasure);
    expect([...v1.UNIVERSE_KEYS]).toStrictEqual(vocabulary.universeKeys);
    expect([...v1.AGGREGATION_METHODS]).toStrictEqual(vocabulary.aggregationMethods);
  });
});

describe("exact quote arithmetic", () => {
  it("spread is ask - bid and spreadBps is spread / price × 10 000, half to even at 4 places", () => {
    // SUI/USD mean of Coinbase 1.1819/1.1821 and Kraken 1.1803/1.1805.
    expect(v1.spreadOf("1.181200", "1.181100", "1.181300")).toStrictEqual({
      spread: "0.000200",
      spreadBps: "1.6932", // 1.69319...
    });
    // Scale is the larger of bid's and ask's; bid and ask keep theirs.
    expect(v1.spreadOf("1.13680", "1.13679", "1.13680").spread).toBe("0.00001");
    expect(v1.spreadOf("84076.2025000", "84076.1750000", "84076.2300000")).toStrictEqual({
      spread: "0.0550000",
      spreadBps: "0.0065", // 0.006541...
    });
    // Exact halves round to even: 1 / 20 000 × 10 000 = 0.5 bps → at 4 places exact;
    // 0.00005 / 1 × 10 000 = 0.5; 0.000000005 / 1 × 10 000 = 0.00005 → 0.0000 (even), 0.000000015 → 0.0002.
    expect(v1.spreadOf("1", "1.000000000", "1.000000005").spreadBps).toBe("0.0000");
    expect(v1.spreadOf("1", "1.000000000", "1.000000015").spreadBps).toBe("0.0002");
    expect(v1.spreadOf("1", "0.99999", "1.00004").spreadBps).toBe("0.5000");
  });

  it("no bid/ask, no spread; a non-positive price has no spreadBps", () => {
    expect(v1.spreadOf("4286.200195", null, null)).toStrictEqual({ spread: null, spreadBps: null });
    expect(v1.spreadOf("0", "-0.01", "0.01")).toStrictEqual({ spread: "0.02", spreadBps: null });
  });

  it("uses no floating point: 0.1 + 0.2 style values stay exact", () => {
    expect(v1.spreadOf("0.3", "0.1", "0.3").spread).toBe("0.2");
    expect(v1.changeOf("0.3", "0.1")).toStrictEqual({ absolute: "0.2", percent: "200.0000" });
  });

  it("cross rate: the fewer significant digits of its legs (mirrors undrly_core)", () => {
    // USD/IDR = USDT/IDR 17910.5 / USDT/USD 1.00005 (6 digits each).
    expect(v1.crossRate("17910.5", "1.00005")).toBe("17909.6");
    expect(v1.crossRate("33.57", "1.0001")).toBe("33.57");
    expect(v1.crossRate("2.00", "1.00")).toBe("2.00");
    // Below one: leading zeros are not significant (0.88444… → 3 digits).
    expect(v1.crossRate("0.99942", "1.13")).toBe("0.884");
    // Half to even at the last kept digit: 1 / 8 = 0.125 at 2 digits.
    expect(v1.crossRate("1.0", "8.0")).toBe("0.12");
    expect(v1.crossRate("1", "0")).toBeNull();
    expect(v1.crossRate("-1", "1")).toBeNull();
    expect(v1.crossRate("x", "1")).toBeNull();
  });

  it("24h change: positive, negative, zero, exact percent", () => {
    expect(v1.changeOf("84076.2025000", "82950.1150000")).toStrictEqual({
      absolute: "1126.0875000",
      percent: "1.3575", // 1.357546...
    });
    expect(v1.changeOf("0.2659000000", "0.2800000000")).toStrictEqual({
      absolute: "-0.0141000000",
      percent: "-5.0357", // -5.035714...
    });
    expect(v1.changeOf("1.10", "1.1")).toStrictEqual({ absolute: "0.00", percent: "0.0000" });
    // Half to even at 4 places: 1/16 % = 0.0625 %; 1/32 % → 0.03125 → 0.0312.
    expect(v1.changeOf("100.0625", "100")?.percent).toBe("0.0625");
    expect(v1.changeOf("100.03125", "100")?.percent).toBe("0.0312");
    expect(v1.changeOf("100.09375", "100")?.percent).toBe("0.0938");
  });

  it("a zero baseline has no change", () => {
    expect(v1.changeOf("1", "0")).toBeNull();
    expect(v1.changeOf("1", "0.000")).toBeNull();
  });

  it("formats like rust_decimal: keeps scale, never a negative zero", () => {
    expect(v1.formatDecimal({ mantissa: -5n, scale: 3 })).toBe("-0.005");
    expect(v1.formatDecimal({ mantissa: 0n, scale: 2 })).toBe("0.00");
    const neg = v1.divide({ mantissa: -1n, scale: 6 }, { mantissa: 1n, scale: 0 }, 2);
    expect(neg && v1.formatDecimal(neg)).toBe("0.00");
  });
});
