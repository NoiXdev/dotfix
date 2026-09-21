import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { Commit } from "../types";
import History from "./History";

const commits: Commit[] = [
  { hash: "abc1234", subject: "add jq to core", date: "2026-09-16" },
  { hash: "def5678", subject: "drop s3cmd from infra", date: "2026-09-15" },
];

describe("History", () => {
  it("lists commits newest first with date and subject", () => {
    render(<History commits={commits} />);
    const rows = screen.getAllByRole("listitem");
    expect(rows[0]).toHaveTextContent("add jq to core");
    expect(rows[0]).toHaveTextContent("2026-09-16");
    expect(rows[1]).toHaveTextContent("drop s3cmd from infra");
  });

  it("explains an empty history rather than showing a blank panel", () => {
    render(<History commits={[]} />);
    expect(screen.getByText(/no commits yet/i)).toBeInTheDocument();
  });
});
