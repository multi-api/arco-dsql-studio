import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import ResultsGrid from "../components/ResultsGrid";

describe("ResultsGrid", () => {
  it("results_show_null_distinctly", () => {
    render(
      <ResultsGrid
        result={{ columns: ["a", "b", "c"], rows: [[null, "", "NULL"]], truncated: false }}
        durationMs={12}
      />,
    );
    const cells = screen.getAllByRole("gridcell");
    expect(cells).toHaveLength(3);
    // SQL NULL: a dimmed badge.
    expect(cells[0].querySelector(".null")).toHaveTextContent("NULL");
    // Empty string: an empty cell.
    expect(cells[1]).toHaveTextContent("");
    expect(cells[1].querySelector(".null")).toBeNull();
    // The text "NULL": plain text, no badge.
    expect(cells[2]).toHaveTextContent("NULL");
    expect(cells[2].querySelector(".null")).toBeNull();
    expect(screen.getByText(/1 row/)).toHaveTextContent("12 ms");
  });

  it("says when rows were cut off", () => {
    render(
      <ResultsGrid
        result={{ columns: ["n"], rows: Array.from({ length: 3 }, (_, i) => [String(i)]), truncated: true }}
        durationMs={1}
      />,
    );
    expect(screen.getByText(/first 3 rows/i)).toBeInTheDocument();
  });
});
