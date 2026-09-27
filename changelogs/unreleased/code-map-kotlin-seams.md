# Kotlin in the code map, and calls read against the specs

- **Kotlin.** `dit code` maps Kotlin alongside TypeScript and Rust. Imports resolve
  through each file's `package`, and a file using a declaration of its own package —
  which Kotlin never imports — counts as depending on it, so `dit code users` on a
  KMP client finds the files that only name a type.
- **`dit code api`** reads every path literal in the code against the registered
  specs, with the constants it is built from put back in. It lists **orphan** calls —
  paths no spec describes, the class that answers 404 — and operations the code calls
  but no scenario has proven on any environment. `dit morse check` prints both counts.
- The closest fit wins (`items/${id}` calls `items/{id}`, not `items/bulk`), and a
  literal made mostly of wildcards is left out instead of matching everything.
