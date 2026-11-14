import { useState } from "react";
import type { GraphQLNamedType, GraphQLSchema } from "graphql";
import { Button } from "../../ui/Button";
import { membersOf, PAGE_SIZE, relatedTypes, rootTypes, searchSchema, type MemberView } from "./explorerModel";

export interface SchemaExplorerProps {
  readonly schema: GraphQLSchema;
}

interface TypeLinkProps {
  readonly name: string;
  readonly text?: string;
  readonly onOpen: (name: string) => void;
}

function TypeLink({ name, text, onOpen }: TypeLinkProps) {
  return (
    <button type="button" className="schema-link" onClick={() => onOpen(name)}>
      {text ?? name}
    </button>
  );
}

function Member({ member, onOpen }: { readonly member: MemberView; readonly onOpen: (name: string) => void }) {
  return (
    <li className="schema-member" data-deprecated={member.deprecation !== null || undefined}>
      <span className="schema-member-name">{member.name}</span>
      {member.args.length > 0 ? (
        <span className="schema-args">
          ({member.args.map((a, i) => (
            <span key={a.name}>
              {i > 0 ? ", " : null}
              {a.name}: <TypeLink name={a.typeName} text={a.typeText} onOpen={onOpen} />
            </span>
          ))})
        </span>
      ) : null}
      {member.typeName !== "" ? (
        <>
          {": "}
          <TypeLink name={member.typeName} text={member.typeText} onOpen={onOpen} />
        </>
      ) : null}
      {member.deprecation !== null ? (
        <span className="schema-deprecated">Deprecated{member.deprecation ? `: ${member.deprecation}` : ""}</span>
      ) : null}
      {member.description ? <p className="schema-description">{member.description}</p> : null}
    </li>
  );
}

function ShowMore({ total, shown, onMore }: { readonly total: number; readonly shown: number; readonly onMore: () => void }) {
  return total > shown ? <Button label={`Show more (${total - shown})`} onClick={onMore} /> : null;
}

function TypeView({ type, schema, onOpen }: { readonly type: GraphQLNamedType; readonly schema: GraphQLSchema; readonly onOpen: (n: string) => void }) {
  const [limit, setLimit] = useState(PAGE_SIZE);
  const members = membersOf(type);
  const related = relatedTypes(type, schema);
  const relatedShown = related?.names.slice(0, limit) ?? [];
  return (
    <div className="schema-type">
      <h3>{type.name}</h3>
      {type.description ? <p className="schema-description">{type.description}</p> : null}
      {related ? (
        <section aria-label={related.label}>
          <h4>{related.label}</h4>
          <ul>
            {relatedShown.map((name) => (
              <li key={name}>
                <TypeLink name={name} onOpen={onOpen} />
              </li>
            ))}
          </ul>
          <ShowMore total={related.names.length} shown={relatedShown.length} onMore={() => setLimit(limit + PAGE_SIZE)} />
        </section>
      ) : null}
      {members.length > 0 ? (
        <section aria-label="Members">
          <ul>
            {members.slice(0, limit).map((m) => (
              <Member key={m.name} member={m} onOpen={onOpen} />
            ))}
          </ul>
          <ShowMore total={members.length} shown={Math.min(limit, members.length)} onMore={() => setLimit(limit + PAGE_SIZE)} />
        </section>
      ) : null}
    </div>
  );
}

function SearchResults({ schema, term, onOpen }: { readonly schema: GraphQLSchema; readonly term: string; readonly onOpen: (n: string) => void }) {
  const [limit, setLimit] = useState(PAGE_SIZE);
  const hits = searchSchema(schema, term);
  if (hits.length === 0) return <p className="empty-text">No matches.</p>;
  return (
    <>
      <ul aria-label="Search results">
        {hits.slice(0, limit).map((h) => (
          <li key={`${h.typeName}.${h.member ?? ""}`}>
            <TypeLink name={h.typeName} text={h.member === null ? h.typeName : `${h.typeName}.${h.member}`} onOpen={onOpen} />
          </li>
        ))}
      </ul>
      <ShowMore total={hits.length} shown={Math.min(limit, hits.length)} onMore={() => setLimit(limit + PAGE_SIZE)} />
    </>
  );
}

/** Side panel: root types, then fields, arguments and return types; clicking a type opens it. */
export function SchemaExplorer({ schema }: SchemaExplorerProps) {
  const [stack, setStack] = useState<readonly string[]>([]);
  const [term, setTerm] = useState("");
  const current = stack.length > 0 ? schema.getType(stack[stack.length - 1] ?? "") : undefined;

  const open = (name: string) => {
    setTerm("");
    setStack((s) => [...s, name]);
  };

  return (
    <aside className="schema-explorer" aria-label="Schema explorer">
      <div className="schema-toolbar">
        <Button label="Back" disabled={stack.length === 0} onClick={() => setStack((s) => s.slice(0, -1))} />
        <input
          type="search"
          aria-label="Search schema"
          placeholder="Search types and fields"
          value={term}
          onChange={(e) => setTerm(e.target.value)}
        />
      </div>
      {term.trim() !== "" ? (
        <SearchResults key={term} schema={schema} term={term} onOpen={open} />
      ) : current ? (
        <TypeView key={current.name} type={current} schema={schema} onOpen={open} />
      ) : (
        <ul aria-label="Root types">
          {rootTypes(schema).map((r) => (
            <li key={r.label}>
              {r.label}: <TypeLink name={r.name} onOpen={open} />
            </li>
          ))}
        </ul>
      )}
    </aside>
  );
}
