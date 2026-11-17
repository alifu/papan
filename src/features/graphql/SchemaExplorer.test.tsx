import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { buildSchema } from "graphql";
import { describe, expect, it } from "vitest";
import { SchemaExplorer } from "./SchemaExplorer";

const SDL = `
  "Who."
  type User { id: ID! name: String! old: String @deprecated(reason: "gone") posts(first: Int = 10): [Post!]! }
  type Post { title: String! }
  type Query { user(id: ID!): User }
  type Big { ${Array.from({ length: 205 }, (_, i) => `f${i}: Int`).join(" ")} }
`;
const schema = buildSchema(SDL);

describe("SchemaExplorer", () => {
  it("starts at the root types and navigates into a type and back", async () => {
    render(<SchemaExplorer schema={schema} />);
    expect(screen.getByRole("list", { name: "Root types" })).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Query" }));
    expect(screen.getByRole("heading", { name: "Query" })).toBeVisible();

    await userEvent.click(screen.getByRole("button", { name: "User" }));
    expect(screen.getByRole("heading", { name: "User" })).toBeVisible();
    expect(screen.getByText("Who.")).toBeVisible();

    await userEvent.click(screen.getByRole("button", { name: "Back" }));
    expect(screen.getByRole("heading", { name: "Query" })).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Back" }));
    expect(screen.getByRole("button", { name: "Back" })).toBeDisabled();
  });

  it("shows arguments, return types and deprecation", async () => {
    render(<SchemaExplorer schema={schema} />);
    await userEvent.click(screen.getByRole("button", { name: "Query" }));
    await userEvent.click(screen.getByRole("button", { name: "User" }));
    expect(screen.getByText("Deprecated: gone")).toBeVisible();
    const posts = screen.getByText("posts").closest("li")!;
    expect(within(posts).getByRole("button", { name: "[Post!]!" })).toBeVisible();
    expect(within(posts).getByRole("button", { name: "Int" })).toBeVisible();
  });

  it("searches types and fields and opens a hit", async () => {
    render(<SchemaExplorer schema={schema} />);
    await userEvent.type(screen.getByLabelText("Search schema"), "title");
    await userEvent.click(screen.getByRole("button", { name: "Post.title" }));
    expect(screen.getByRole("heading", { name: "Post" })).toBeVisible();
    expect(screen.getByLabelText("Search schema")).toHaveValue("");

    await userEvent.type(screen.getByLabelText("Search schema"), "zzz");
    expect(screen.getByText("No matches.")).toBeVisible();
  });

  it("renders at most 200 members until asked for more", async () => {
    render(<SchemaExplorer schema={schema} />);
    await userEvent.type(screen.getByLabelText("Search schema"), "Big");
    await userEvent.click(screen.getByRole("button", { name: "Big" }));
    const members = screen.getByRole("region", { name: "Members" });
    expect(within(members).getAllByRole("listitem")).toHaveLength(200);
    await userEvent.click(screen.getByRole("button", { name: "Show more (5)" }));
    expect(within(members).getAllByRole("listitem")).toHaveLength(205);
  });
});
