// The "[[" menu: link an issue or a page by picking it, instead of knowing
// that `[[Q2R7VN8]]` is the syntax and looking up the ref. Same look and
// keys as the "/" menu.

import { forwardRef, useEffect, useImperativeHandle, useState } from "react";
import { Extension } from "@tiptap/core";
import Suggestion, { exitSuggestion, type SuggestionProps } from "@tiptap/suggestion";
import { ReactRenderer } from "@tiptap/react";
import { PluginKey } from "@tiptap/pm/state";
import { CircleDot, FileText } from "lucide-react";

import { matchWikiItems, type WikiItem } from "../lib/wikilinks";

const WIKI_KEY = new PluginKey("wikiMenu");

export type WikiListHandle = { onKeyDown: (event: KeyboardEvent) => boolean };

const WikiList = forwardRef<WikiListHandle, SuggestionProps<WikiItem>>(function WikiList(props, ref) {
  const [selected, setSelected] = useState(0);
  useEffect(() => setSelected(0), [props.query]);
  const items = props.items;

  useImperativeHandle(ref, () => ({
    onKeyDown: (event) => {
      if (event.key === "ArrowDown") {
        setSelected((s) => (items.length === 0 ? 0 : (s + 1) % items.length));
        return true;
      }
      if (event.key === "ArrowUp") {
        setSelected((s) => (items.length === 0 ? 0 : (s - 1 + items.length) % items.length));
        return true;
      }
      if (event.key === "Enter" || event.key === "Tab") {
        const item = items[selected];
        if (item) props.command(item);
        return item !== undefined;
      }
      return false;
    },
  }));

  return (
    <div className="dit-slash" role="listbox" aria-label="Link to an issue or a page">
      <div className="dit-slash-list">
        {items.length === 0 ? (
          <div className="dit-slash-empty">
            {props.query ? "No issue or page matches." : "Type to find an issue or a page."}
          </div>
        ) : (
          items.map((item, index) => {
            const Icon = item.kind === "issue" ? CircleDot : FileText;
            return (
              <button
                key={`${item.kind}:${item.target}`}
                type="button"
                role="option"
                aria-selected={index === selected}
                className="dit-slash-item dit-wiki-item"
                data-selected={index === selected || undefined}
                onMouseEnter={() => setSelected(index)}
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => props.command(item)}
              >
                <Icon className="size-4 shrink-0 text-muted" aria-hidden />
                <span className="dit-wiki-label">{item.label}</span>
                <span className="dit-wiki-detail">{item.detail}</span>
              </button>
            );
          })
        )}
      </div>
    </div>
  );
});

/** `getItems` is read at every keystroke, so a list that loads after the
 *  editor mounted is picked up without rebuilding the editor. */
export const WikiMenu = Extension.create<{ getItems: () => readonly WikiItem[] }>({
  name: "wikiMenu",
  addOptions() {
    return { getItems: () => [] };
  },
  addProseMirrorPlugins() {
    const getItems = this.options.getItems;
    return [
      Suggestion<WikiItem>({
        editor: this.editor,
        // Its own key: the "/" menu holds the default one, and two plugins
        // under one key is an error.
        pluginKey: WIKI_KEY,
        char: "[[",
        allowSpaces: true,
        startOfLine: false,
        items: ({ query }) => matchWikiItems(getItems(), query),
        command: ({ editor, range, props: item }) => {
          editor
            .chain()
            .focus()
            .deleteRange(range)
            .insertContent([
              { type: "wikiLink", attrs: { target: item.target }, content: [{ type: "text", text: item.label }] },
              { type: "text", text: " " },
            ])
            .run();
        },
        render: () => {
          let component: ReactRenderer<WikiListHandle> | null = null;
          let unmount: (() => void) | undefined;
          return {
            onStart: (props) => {
              component = new ReactRenderer(WikiList, { props, editor: props.editor });
              unmount = props.mount?.(component.element);
            },
            onUpdate: (props) => component?.updateProps(props),
            onKeyDown: (props) => {
              if (props.event.key === "Escape") {
                exitSuggestion(props.view, WIKI_KEY);
                return true;
              }
              return component?.ref?.onKeyDown(props.event) ?? false;
            },
            onExit: () => {
              unmount?.();
              unmount = undefined;
              component?.destroy();
              component = null;
            },
          };
        },
      }),
    ];
  },
});
