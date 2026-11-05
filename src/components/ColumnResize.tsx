import { useCallback, useEffect, useState } from "react";

export const DEFAULT_WIDTH = 160;
const MIN_WIDTH = 48;
const MAX_FIT = 600;
/** Rough width of one character of the grid's 12px monospace font, for "fit to content". */
const CHAR_PX = 7.4;
const PADDING_PX = 24;

/** Column widths for a grid, reset when the columns change. */
export function useColumnWidths(columns: string[]) {
  const key = columns.join("\u0000");
  const [widths, setWidths] = useState<number[]>(() => columns.map(() => DEFAULT_WIDTH));

  useEffect(() => {
    setWidths(key ? key.split("\u0000").map(() => DEFAULT_WIDTH) : []);
  }, [key]);

  const setWidth = useCallback((i: number, w: number) => {
    setWidths((ws) => ws.map((old, k) => (k === i ? Math.max(MIN_WIDTH, Math.round(w)) : old)));
  }, []);

  return { widths, setWidth };
}

/** The width that fits a column's header and the given values, within limits. */
export function fitWidth(header: string, values: (string | null)[]): number {
  const longest = values.reduce((n, v) => Math.max(n, (v ?? "NULL").length), header.length + 3);
  return Math.min(MAX_FIT, Math.max(MIN_WIDTH, Math.ceil(longest * CHAR_PX + PADDING_PX)));
}

/** The drag handle on a header's right edge. Double-click fits the column to its content. */
export function ResizeHandle({
  width,
  onResize,
  onFit,
  label,
}: {
  width: number;
  onResize: (width: number) => void;
  onFit: () => void;
  label: string;
}) {
  function start(e: React.MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    const x0 = e.clientX;
    const move = (ev: MouseEvent) => onResize(width + ev.clientX - x0);
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      document.body.classList.remove("resizing");
    };
    document.body.classList.add("resizing");
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  }

  return (
    <span
      className="col-resize"
      role="separator"
      aria-orientation="vertical"
      aria-label={`Resize ${label}`}
      onMouseDown={start}
      onDoubleClick={(e) => {
        e.stopPropagation();
        onFit();
      }}
      onClick={(e) => e.stopPropagation()}
    />
  );
}
