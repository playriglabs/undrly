import { z } from "zod";
import {
  CanonicalId,
  type Category,
  canonicalIdCategory,
  SourceId,
  TimestampString,
} from "./primitives.ts";

/**
 * Mirrors `undrly_core::RelationshipType`: the stored vocabulary, in canonical
 * direction (`subject → object`). Inverse labels such as `UNDERLYING_OF` and
 * the projections `LISTED_ON` (from listings) and `DEPLOYED_ON` (from a
 * deployment's chain) are derived by the API at query time and are not part
 * of this contract.
 */
export const RELATIONSHIP_TYPES = [
  "ISSUED_BY",
  "TRADES_ON",
  "DENOMINATED_IN",
  "SETTLES_IN",
  "MARGINED_IN",
  "DERIVES_FROM",
  "HOLDS",
  "TRACKS",
  "MEMBER_OF",
  "TOKENIZES",
  "REPRESENTS",
  "PRICED_BY",
  "AVAILABLE_ON",
  "RELATED_TO",
] as const;
export type RelationshipType = (typeof RELATIONSHIP_TYPES)[number];

/**
 * Mirrors `RelationshipType::allowed_endpoints` and the `relationship_rules`
 * table. Types without rules are not storable yet.
 */
export const RELATIONSHIP_RULES: readonly (readonly [RelationshipType, Category, Category])[] = [
  ["ISSUED_BY", "instrument", "entity"],
  ["TRADES_ON", "instrument", "venue"],
  ["DENOMINATED_IN", "instrument", "currency"],
  ["DENOMINATED_IN", "instrument", "instrument"],
  ["SETTLES_IN", "instrument", "currency"],
  ["SETTLES_IN", "instrument", "instrument"],
  ["MARGINED_IN", "instrument", "currency"],
  ["MARGINED_IN", "instrument", "instrument"],
  ["DERIVES_FROM", "instrument", "instrument"],
  ["TOKENIZES", "instrument", "instrument"],
  ["REPRESENTS", "deployment", "instrument"],
];

export const ProvenanceV1 = z.strictObject({
  sourceId: SourceId,
  receivedAt: TimestampString,
});
export type ProvenanceV1 = z.infer<typeof ProvenanceV1>;

/** A relationship as currently asserted by one source (not eternal truth). */
export const RelationshipV1 = z
  .strictObject({
    schemaVersion: z.literal(1),
    subjectId: CanonicalId,
    relationshipType: z.enum(RELATIONSHIP_TYPES),
    objectId: CanonicalId,
    provenance: ProvenanceV1,
  })
  .superRefine((relationship, ctx) => {
    if (relationship.subjectId === relationship.objectId) {
      ctx.addIssue({ code: "custom", message: "subject and object are the same id" });
    }
    const subject = canonicalIdCategory(relationship.subjectId);
    const object = canonicalIdCategory(relationship.objectId);
    const allowed = RELATIONSHIP_RULES.some(
      ([type, s, o]) => type === relationship.relationshipType && s === subject && o === object,
    );
    if (!allowed) {
      ctx.addIssue({
        code: "custom",
        message: `${relationship.relationshipType} cannot connect ${subject} → ${object}`,
      });
    }
  });
export type RelationshipV1 = z.infer<typeof RelationshipV1>;
