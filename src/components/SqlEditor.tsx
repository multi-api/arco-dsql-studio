import { autocompletion } from "@codemirror/autocomplete";
import { PostgreSQL, sql } from "@codemirror/lang-sql";
import { Compartment, EditorState, Prec } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { basicSetup } from "codemirror";
import { useEffect, useRef } from "react";

export interface Selection {
  /** Cursor offset in the text (JavaScript units). */
  cursor: number;
  /** Selected text, or "" when nothing is selected. */
  selected: string;
}

interface Props {
  value: string;
  onChange: (value: string) => void;
  onSelection?: (selection: Selection) => void;
  /** ⌘↵ / Ctrl+↵: the statement at the cursor, or the selection. */
  onRun?: () => void;
  /** ⇧⌘↵ / Shift+Ctrl+↵: the whole script. */
  onRunAll?: () => void;
  /** Tables of the current schema and their columns, for autocomplete. */
  completions?: Record<string, string[]>;
  defaultSchema?: string | null;
}

const theme = EditorView.theme({
  "&": { height: "100%", fontSize: "13px", backgroundColor: "var(--surface)", color: "var(--text)" },
  ".cm-scroller": { fontFamily: "var(--mono)" },
  ".cm-gutters": { backgroundColor: "var(--surface)", color: "var(--muted)", borderRight: "1px solid var(--border)" },
  ".cm-activeLine, .cm-activeLineGutter": { backgroundColor: "var(--hover)" },
  "&.cm-focused": { outline: "none" },
  ".cm-cursor": { borderLeftColor: "var(--text)" },
});

/** CodeMirror with PostgreSQL highlighting and autocomplete from the current schema. */
export default function SqlEditor({
  value,
  onChange,
  onSelection,
  onRun,
  onRunAll,
  completions,
  defaultSchema,
}: Props) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const language = useRef(new Compartment());
  // Latest callbacks, so the editor (created once) never calls stale ones.
  const handlers = useRef({ onChange, onSelection, onRun, onRunAll });
  handlers.current = { onChange, onSelection, onRun, onRunAll };

  useEffect(() => {
    if (!host.current) return;
    const runKeys = Prec.highest(
      keymap.of([
        { key: "Mod-Enter", run: () => (handlers.current.onRun?.(), true) },
        { key: "Shift-Mod-Enter", run: () => (handlers.current.onRunAll?.(), true) },
      ]),
    );
    view.current = new EditorView({
      parent: host.current,
      state: EditorState.create({
        doc: value,
        extensions: [
          runKeys,
          basicSetup,
          language.current.of(sql({ dialect: PostgreSQL })),
          autocompletion(),
          theme,
          EditorView.lineWrapping,
          EditorView.updateListener.of((u) => {
            if (u.docChanged) handlers.current.onChange(u.state.doc.toString());
            if (u.docChanged || u.selectionSet) {
              const r = u.state.selection.main;
              handlers.current.onSelection?.({ cursor: r.head, selected: u.state.sliceDoc(r.from, r.to) });
            }
          }),
        ],
      }),
    });
    view.current.focus();
    return () => view.current?.destroy();
    // The document is only seeded here; later outside changes go through the effect below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const v = view.current;
    if (v && v.state.doc.toString() !== value) {
      v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: value } });
    }
  }, [value]);

  useEffect(() => {
    view.current?.dispatch({
      effects: language.current.reconfigure(
        sql({ dialect: PostgreSQL, schema: completions ?? {}, defaultSchema: defaultSchema ?? undefined }),
      ),
    });
  }, [completions, defaultSchema]);

  return <div className="editor" ref={host} />;
}
