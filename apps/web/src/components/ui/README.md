# UI components

Copied in, not imported (ADR 0003). Add one with:

```bash
npx shadcn@latest add button dialog command
```

Then **read what it added.** These are our files now — adjust them to the token
set in `src/styles.css` rather than accumulating one-off overrides at call sites.

Two rules:

1. **Components consume tokens, never raw values.** No hex colors, no magic
   pixel sizes. Swapping the palette must never mean editing a component.
2. **Anything heavy is lazy.** `mermaid` is 83 MB unpacked; it belongs behind a
   dynamic `import()`, never in the entry graph. `scripts/check-bundle-size.mjs`
   fails CI if it drifts back in.
