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
