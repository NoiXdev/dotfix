import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import Answers from "./Answers";

afterEach(cleanup);

const base = {
  machine: "",
  mode: "new" as const,
  url: "",
  secretProvider: "keychain" as const,
  vault: "",
};

describe("Answers", () => {
  it("asks for a url only when cloning", () => {
    const { rerender } = render(<Answers value={base} onChange={vi.fn()} />);
    expect(screen.queryByLabelText(/repository url/i)).toBeNull();

    rerender(<Answers value={{ ...base, mode: "clone" }} onChange={vi.fn()} />);
    expect(screen.getByLabelText(/repository url/i)).toBeInTheDocument();
  });

  it("asks for a vault only for 1Password", () => {
    const { rerender } = render(<Answers value={base} onChange={vi.fn()} />);
    expect(screen.queryByLabelText(/vault/i)).toBeNull();

    rerender(
      <Answers value={{ ...base, secretProvider: "1password" }} onChange={vi.fn()} />,
    );
    expect(screen.getByLabelText(/vault/i)).toBeInTheDocument();
  });

  it("reports every change to the parent", () => {
    const onChange = vi.fn();
    render(<Answers value={base} onChange={onChange} />);
    fireEvent.change(screen.getByLabelText(/machine name/i), {
      target: { value: "box-one" },
    });
    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({ machine: "box-one" }),
    );
  });
});
