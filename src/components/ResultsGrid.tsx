import { useEffect, useRef, useState } from "react";

import type { ResultSet } from "../types";

const ROW_HEIGHT = 26;
/** Rows drawn above and below the visible ones, so fast scrolling does not show blanks. */
const OVERSCAN = 20;
/** Used until the grid is laid out (and in tests, where there is no layout). */
const FALLBACK_HEIGHT = 600;

/** Read-only query results. Only the visible rows are in the DOM, so 10,000 rows scroll smoothly. */
export default function ResultsGrid({ result, durationMs }: { result: ResultSet; durationMs: number }) {
  const box = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [height, setHeight] = useState(FALLBACK_HEIGHT);

  useEffect(() => {
    const el = box.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(() => setHeight(el.clientHeight || FALLBACK_HEIGHT));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  useEffect(() => {
    setScrollTop(0);
    if (box.current) box.current.scrollTop = 0;
  }, [result]);

  const { columns, rows, truncated } = result;
  const first = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN);
  const last = Math.min(rows.length, Math.ceil((scrollTop + height) / ROW_HEIGHT) + OVERSCAN);
  const count = `${rows.length.toLocaleString()} ${rows.length === 1 ? "row" : "rows"}`;

  return (
    <div className="results">
      <div className="results-status">
        <span>
          {count} · {durationMs.toLocaleString()} ms
        </span>
        {truncated && (
          <span className="badge warn">
            Showing the first {rows.length.toLocaleString()} rows; add LIMIT or a WHERE to see the rest
          </span>
        )}
      </div>
      <div className="grid-scroll" ref={box} onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}>
        <table className="grid" role="grid" aria-rowcount={rows.length + 1}>
          <thead>
            <tr role="row">
              <th className="rownum" aria-hidden />
              {columns.map((c, i) => (
                <th key={i} role="columnheader">
                  {c}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {first > 0 && (
              <tr aria-hidden style={{ height: first * ROW_HEIGHT }}>
                <td colSpan={columns.length + 1} />
              </tr>
            )}
            {rows.slice(first, last).map((row, k) => (
              <tr key={first + k} role="row" style={{ height: ROW_HEIGHT }}>
                <td className="rownum" aria-hidden>
                  {first + k + 1}
                </td>
                {row.map((v, j) => (
                  <td key={j} role="gridcell" title={v ?? "NULL"}>
                    {v === null ? <span className="null">NULL</span> : v}
                  </td>
                ))}
              </tr>
            ))}
            {last < rows.length && (
              <tr aria-hidden style={{ height: (rows.length - last) * ROW_HEIGHT }}>
                <td colSpan={columns.length + 1} />
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
