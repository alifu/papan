import type { KeyboardEvent } from "react";
import { Button } from "../../ui/Button";
import "./response.css";
import { stepIndex } from "./search";

export interface SearchBarProps {
  readonly query: string;
  readonly onQueryChange: (query: string) => void;
  readonly matchCount: number;
  readonly capped?: boolean;
  /** Zero-based index of the highlighted match. */
  readonly current: number;
  readonly onCurrentChange: (index: number) => void;
}

export function SearchBar({
  query,
  onQueryChange,
  matchCount,
  capped = false,
  current,
  onCurrentChange,
}: SearchBarProps) {
  const step = (delta: 1 | -1) => onCurrentChange(stepIndex(current, matchCount, delta));

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== "Enter") return;
    event.preventDefault();
    step(event.shiftKey ? -1 : 1);
  };

  const status =
    query === "" ? "" : matchCount === 0 ? "No matches" : `${current + 1} of ${matchCount}${capped ? "+" : ""}`;

  return (
    <div className="search-bar" role="search">
      <input
        type="search"
        aria-label="Find in response"
        placeholder="Find"
        value={query}
        onChange={(event) => onQueryChange(event.target.value)}
        onKeyDown={onKeyDown}
      />
      <span className="search-status" aria-live="polite">
        {status}
      </span>
      <Button label="Previous" disabled={matchCount === 0} onClick={() => step(-1)} />
      <Button label="Next" disabled={matchCount === 0} onClick={() => step(1)} />
    </div>
  );
}
