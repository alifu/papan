import {
  getNamedType,
  isEnumType,
  isInputObjectType,
  isInterfaceType,
  isObjectType,
  isUnionType,
  type GraphQLNamedType,
  type GraphQLSchema,
} from "graphql";

export interface ArgView {
  readonly name: string;
  readonly typeText: string;
  readonly typeName: string;
}

/** One row of a type: a field, an input field or an enum value. */
export interface MemberView {
  readonly name: string;
  readonly args: readonly ArgView[];
  /** The type as written (`[Post!]!`); empty for enum values. */
  readonly typeText: string;
  /** The named type to navigate to; empty for enum values. */
  readonly typeName: string;
  readonly description: string;
  /** The deprecation reason; null when not deprecated. */
  readonly deprecation: string | null;
}

export interface RootView {
  readonly label: string;
  readonly name: string;
}

export interface SearchHit {
  readonly typeName: string;
  /** The matching field, or null when the type name itself matched. */
  readonly member: string | null;
}

export const PAGE_SIZE = 200;

export function rootTypes(schema: GraphQLSchema): readonly RootView[] {
  const roots: [string, GraphQLNamedType | null | undefined][] = [
    ["Query", schema.getQueryType()],
    ["Mutation", schema.getMutationType()],
    ["Subscription", schema.getSubscriptionType()],
  ];
  return roots.flatMap(([label, type]) => (type ? [{ label, name: type.name }] : []));
}

const view = (m: {
  name: string;
  description?: string | null;
  deprecationReason?: string | null;
}): Pick<MemberView, "name" | "description" | "deprecation"> => ({
  name: m.name,
  description: m.description ?? "",
  deprecation: m.deprecationReason ?? null,
});

/** Fields (objects, interfaces), input fields or enum values of a type; none for scalars and unions. */
export function membersOf(type: GraphQLNamedType): readonly MemberView[] {
  if (isObjectType(type) || isInterfaceType(type)) {
    return Object.values(type.getFields()).map((f) => ({
      ...view(f),
      args: f.args.map((a) => ({ name: a.name, typeText: String(a.type), typeName: getNamedType(a.type).name })),
      typeText: String(f.type),
      typeName: getNamedType(f.type).name,
    }));
  }
  if (isInputObjectType(type)) {
    return Object.values(type.getFields()).map((f) => ({
      ...view(f),
      args: [],
      typeText: String(f.type),
      typeName: getNamedType(f.type).name,
    }));
  }
  if (isEnumType(type)) {
    return type.getValues().map((v) => ({ ...view(v), args: [], typeText: "", typeName: "" }));
  }
  return [];
}

/** Interfaces of an object or interface, or the members of a union, with the label to show them under. */
export function relatedTypes(type: GraphQLNamedType, schema: GraphQLSchema): { label: string; names: readonly string[] } | null {
  if (isUnionType(type)) return { label: "Possible types", names: type.getTypes().map((t) => t.name) };
  if ((isObjectType(type) || isInterfaceType(type)) && type.getInterfaces().length > 0) {
    return { label: "Implements", names: type.getInterfaces().map((t) => t.name) };
  }
  if (isInterfaceType(type)) {
    const names = schema.getPossibleTypes(type).map((t) => t.name);
    return names.length > 0 ? { label: "Implemented by", names } : null;
  }
  return null;
}

/** Case-insensitive search over type names and field names; introspection types are skipped. */
export function searchSchema(schema: GraphQLSchema, term: string): readonly SearchHit[] {
  const needle = term.trim().toLowerCase();
  if (needle === "") return [];
  const hits: SearchHit[] = [];
  for (const type of Object.values(schema.getTypeMap())) {
    if (type.name.startsWith("__")) continue;
    if (type.name.toLowerCase().includes(needle)) hits.push({ typeName: type.name, member: null });
    for (const member of membersOf(type)) {
      if (member.name.toLowerCase().includes(needle)) hits.push({ typeName: type.name, member: member.name });
    }
  }
  return hits;
}
