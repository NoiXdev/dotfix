import { describe, expect, it } from "vitest";

/**
 * Dark mode works because every colour in this app comes from a token that a
 * media query can redefine. A component that named a colour directly would
 * keep its light value on a dark window, and nothing else would catch it —
 * a screenshot might, but this project takes none.
 *
 * Sources are pulled in through Vite's glob rather than `node:fs`, which the
 * project has no types for: a dependency added to satisfy one test is a
 * dependency to carry forever.
 */
const sources = import.meta.glob("./**/*.tsx", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

const NAMES_A_COLOUR =
  /#[0-9a-fA-F]{3,8}\b|\b(?:bg|text|border)-(?:white|black|gray-\d|slate-\d|zinc-\d)/;

describe("colour tokens", () => {
  it("no component names a colour directly", () => {
    const offenders = Object.entries(sources)
      .filter(([path]) => !path.includes(".test."))
      .filter(([, body]) => NAMES_A_COLOUR.test(body))
      .map(([path]) => path);

    expect(offenders).toEqual([]);
  });

  it("actually looked at the components", () => {
    // A glob that matched nothing would make the test above pass for the
    // wrong reason — the most common way a guard like this quietly dies.
    const scanned = Object.keys(sources).filter((p) => !p.includes(".test."));
    expect(scanned.length).toBeGreaterThan(8);
  });

  it("outlines controls with `control`, not the divider colour", () => {
    // `hairline` is deliberately faint — it separates rows without drawing
    // attention — and a button wearing it measured 1.2 against the canvas,
    // where a control needs 3.0 to be seen. The rule that keeps the two
    // apart is readable in the markup: a full `border` is an element, a
    // single edge is a divider.
    const offenders = Object.entries(sources)
      .filter(([path]) => !path.includes(".test."))
      .filter(([, body]) => /\bborder border-hairline\b/.test(body))
      .map(([path]) => path);

    expect(offenders).toEqual([]);
  });
});
