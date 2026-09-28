/**
 * Wire primitives for API contract v1. Formats shared with Rust (canonical
 * ids, decimals, timestamps, source ids) are pinned by
 * `tests/fixtures/shared/primitives.json`, which both languages test.
 *
 * Decimals and timestamps stay strings: a JavaScript number cannot hold
 * `183.4200` (scale is lost) or large decimals exactly, and `Date` cannot
 * hold microseconds.
 */
import { z } from "zod";

/** Mirrors `undrly_core::Category`. */
export const CATEGORIES = [
  "entity",
  "instrument",
  "listing",
  "venue",
  "currency",
  "chain",
  "deployment",
] as const;
export type Category = (typeof CATEGORIES)[number];

const ID_ALPHABET = "0123456789abcdefghjkmnpqrstvwxyz";
const ID_PATTERN = /^undrly:([a-z]+):([0-7][0-9a-hjkmnp-tv-z]{25})$/;

function isCategory(value: string): value is Category {
  return (CATEGORIES as readonly string[]).includes(value);
}

/** Decodes the 26-character base32 body to the 128-bit UUID value. */
function decodeIdBody(body: string): bigint {
  let value = 0n;
  for (const char of body) {
    value = (value << 5n) | BigInt(ID_ALPHABET.indexOf(char));
  }
  return value;
}

/**
 * Returns the category of a valid canonical id, or `null`. Valid means
 * `undrly:<category>:<base32 UUIDv7>` with an RFC 9562 variant.
 */
export function canonicalIdCategory(value: string): Category | null {
  const match = ID_PATTERN.exec(value);
  if (match === null) return null;
  const [, category = "", body = ""] = match;
  if (!isCategory(category)) return null;
  const uuid = decodeIdBody(body);
  const version = (uuid >> 76n) & 0xfn;
  const variant = (uuid >> 62n) & 0x3n;
  return version === 7n && variant === 2n ? category : null;
}

/** The UUID (PostgreSQL text form) of a valid canonical id, or `null`. */
export function canonicalIdUuid(value: string): string | null {
  if (canonicalIdCategory(value) === null) return null;
  const hex = decodeIdBody(value.slice(value.lastIndexOf(":") + 1))
    .toString(16)
    .padStart(32, "0");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

/** Formats a UUID (as stored in PostgreSQL) as canonical id text. */
export function formatCanonicalId(category: Category, uuid: string): string {
  const hex = uuid.replaceAll("-", "");
  if (!/^[0-9a-f]{32}$/i.test(hex)) throw new Error(`invalid uuid ${uuid}`);
  let value = BigInt(`0x${hex}`);
  let body = "";
  for (let i = 0; i < 26; i++) {
    body = ID_ALPHABET.charAt(Number(value & 31n)) + body;
    value >>= 5n;
  }
  const id = `undrly:${category}:${body}`;
  if (canonicalIdCategory(id) !== category) throw new Error(`not a UUIDv7: ${uuid}`);
  return id;
}

export const CanonicalId = z
  .string()
  .refine((value) => canonicalIdCategory(value) !== null, "invalid canonical id")
  .brand<"CanonicalId">();
export type CanonicalId = z.infer<typeof CanonicalId>;

function idOfCategory<const Name extends string>(category: Category, _name: Name) {
  return z
    .string()
    .refine((value) => canonicalIdCategory(value) === category, `expected ${category} id`)
    .brand<Name>();
}

export const EntityId = idOfCategory("entity", "EntityId");
export type EntityId = z.infer<typeof EntityId>;
export const InstrumentId = idOfCategory("instrument", "InstrumentId");
export type InstrumentId = z.infer<typeof InstrumentId>;
export const VenueId = idOfCategory("venue", "VenueId");
export type VenueId = z.infer<typeof VenueId>;
export const CurrencyId = idOfCategory("currency", "CurrencyId");
export type CurrencyId = z.infer<typeof CurrencyId>;
export const ChainId = idOfCategory("chain", "ChainId");
export type ChainId = z.infer<typeof ChainId>;
export const DeploymentId = idOfCategory("deployment", "DeploymentId");
export type DeploymentId = z.infer<typeof DeploymentId>;

const DECIMAL = /^-?(0|[1-9][0-9]*)(?:\.([0-9]+))?$/;
const MAX_DECIMAL_SCALE = 28;
const MAX_DECIMAL_MANTISSA = 2n ** 96n - 1n;

/** Canonical decimal text exactly as `rust_decimal::Decimal` prints it. */
export function isCanonicalDecimal(value: string): boolean {
  const match = DECIMAL.exec(value);
  if (match === null) return false;
  const fraction = match[2] ?? "";
  if (fraction.length > MAX_DECIMAL_SCALE) return false;
  const mantissa = BigInt(`${match[1]}${fraction}`);
  if (mantissa > MAX_DECIMAL_MANTISSA) return false;
  return !(value.startsWith("-") && mantissa === 0n);
}

export const DecimalString = z
  .string()
  .refine(isCanonicalDecimal, "invalid canonical decimal string")
  .brand<"DecimalString">();
export type DecimalString = z.infer<typeof DecimalString>;

/**
 * Microseconds since the epoch of a canonical timestamp (exact, unlike
 * `Date.parse`, which drops microseconds). `0n` for anything else.
 */
export function timestampMicros(timestamp: string): bigint {
  const match = /^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})(?:\.(\d{3}|\d{6}))?Z$/.exec(timestamp);
  if (match === null) return 0n;
  const [, seconds = "", fraction = ""] = match;
  return BigInt(Date.parse(`${seconds}Z`)) * 1000n + BigInt(fraction.padEnd(6, "0"));
}

// Fraction is omitted, 3 digits (not `000`), or 6 digits (not ending `000`).
const TIMESTAMP =
  /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.(?!000Z)\d{3}|\.\d{3}(?!000)\d{3})?Z$/;

function daysInMonth(year: number, month: number): number {
  const leap = (year % 4 === 0 && year % 100 !== 0) || year % 400 === 0;
  return [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][month - 1] ?? 0;
}

/** Canonical RFC 3339 UTC text as `undrly_core::Timestamp` prints it. */
export function isCanonicalTimestamp(value: string): boolean {
  const match = TIMESTAMP.exec(value);
  if (match === null) return false;
  const [year, month, day, hour, minute, second] = match.slice(1, 7).map(Number) as [
    number,
    number,
    number,
    number,
    number,
    number,
  ];
  return (
    year >= 1 &&
    month >= 1 &&
    month <= 12 &&
    day >= 1 &&
    day <= daysInMonth(year, month) &&
    hour <= 23 &&
    minute <= 59 &&
    second <= 59
  );
}

export const TimestampString = z
  .string()
  .refine(isCanonicalTimestamp, "invalid canonical UTC timestamp")
  .brand<"TimestampString">();
export type TimestampString = z.infer<typeof TimestampString>;

/** ISO 4217 alphabetic code. Display/convenience only; never identity. */
export const CurrencyCode = z
  .string()
  .regex(/^[A-Z]{3}$/, "invalid ISO 4217 code")
  .brand<"CurrencyCode">();
export type CurrencyCode = z.infer<typeof CurrencyCode>;

export const SourceId = z
  .string()
  .max(64)
  .regex(/^[a-z0-9]+(?:-[a-z0-9]+)*$/, "invalid source id")
  .brand<"SourceId">();
export type SourceId = z.infer<typeof SourceId>;
