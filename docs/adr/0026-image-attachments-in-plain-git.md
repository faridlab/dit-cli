---
id: 0026
title: "Images are attachments in plain git: stored beside their page, capped at 1 MB, served by the server that wrote them"
status: accepted
date: 2026-10-03
supersedes: null
---

## Context

The rich editor can show an image (`![alt](src)` round-trips through the
bridge), but nobody can put one there. The "/" menu asks for an address, and
the address almost never works:

- The server's CSP is `img-src 'self' data:` (§17.2). An image on another
  host is blocked, by design: every document opened would otherwise make the
  browser contact whatever server its author chose, which is a tracking pixel
  in a pull request.
- A path inside the repo (`attachments/flow.png`) resolves against the page
  URL, `http://127.0.0.1:7700/`, where nothing serves repo files.

So the only image that renders today is a `data:` URI pasted into source
mode. People who write PRDs, flows and bug reports paste screenshots; DESIGN
§4.1 and §13 already reserve an `attachments/` folder for them, and §8 already
decided how big they may be (plain git below 1 MB, above that refuse — never
silent LFS). What was missing is the write path, the read path, and how a
relative `src` reaches the bytes.

## Options considered

| Option | Cost | Consequence |
|---|---|---|
| Keep addresses only | None | Screenshots cannot be added without a terminal; outside images stay blocked |
| Loosen CSP to `img-src https:` | One header | Every opened document can ping a server its author picked — a read becomes an outbound request (the I7 spirit, and §17's threat model) |
| Store bytes in the index, serve from SQLite | Index table, reindex and absorb work, bytes duplicated into the cache | Satisfies the strictest reading of I2, but ADR 0010 already scoped I2 to the issue read surface; docs are file-backed |
| **Attachments as files beside their page, written through `Transaction`, read file-backed like docs** | One value object, one staged-bytes write, two routes | Same sandbox and same history as the page they belong to; no new cache to keep in step |
| Git LFS | A dependency and a second server | §8 rejects it as a default: a clone stops being a full backup |

## Decision

**An image is an attachment: a file in plain git beside the page that uses
it, added through `Transaction`, read back by the server that wrote it.**

- **Where it lands.** An issue keeps attachments in `attachments/` beside its
  `README.md`, exactly as §4.1 draws it. A page is always a loose
  `<name>.md` to the editor (`DocPath` does not address folder `README.md`
  pages), so it shares its folder's `attachments/` and prefixes the file with
  its own name (`docs/attachments/guide-…`) — rather than being turned into a
  folder on first paste, which would move the page out from under an open
  editor.
- **What it is called.** `<words from the original name>-<8 hex of the git
  blob id>.<ext>`. The hash makes a re-upload of the same picture a no-op and
  two different pictures with one name two files, so attachments never
  conflict in a merge.
- **What it may be.** PNG, JPEG, GIF or WebP, identified by its first bytes,
  never by its name or the browser's content type. **SVG is refused** — it
  can carry script, and a `dit-diagram` fence is where vector drawings go
  (ADR 0012, sanitized). At most **1 MB** (§8); larger files are refused with
  a sentence that says so.
- **How it is linked.** Relative, the way GitHub reads it: `attachments/x.png`
  from a page or an issue body, `../attachments/x.png` from a comment. The
  markdown stays portable; nothing in the file names the server.
- **How it is read.** `Dit::read_attachment(path)` reads the file, file-backed
  like `read_doc` (ADR 0010), through the same `dit-model` sandbox: a content
  root first segment, an `attachments` parent, no `..`, an image extension —
  and the bytes are sniffed again on the way out.
- **How the browser gets it.** `GET /api/attachments/<path>`, under the
  session token like every other `/api` route. An `<img>` cannot send an
  `Authorization` header, so this route also accepts `?token=`, as
  `/api/events` already does. The editor and the comment renderer turn a
  relative `src` into that URL at display time; the bytes on disk never hold
  it. The response is served with its sniffed type and `nosniff`.
- **How it is added.** `POST /api/attachments?doc=<path>` or
  `?issue=<ref>`, raw bytes, one commit (`dit attach: <path>`). The editor
  calls it on paste, on drop, and from the Image dialog's file picker, then
  inserts the returned link.

Invariant check: I1 holds (bytes are staged in `Transaction` and written by
`dit-store::atomic`); I3 holds (the blob id comes from `dit-vcs`); I7 holds
(a relative path is not a URL that is fetched automatically — the browser
fetches it from DIT itself, and an absolute `src` is still blocked by CSP);
I2 is unchanged in scope (ADR 0010: the issue read surface).

## Consequences

- A screenshot pasted into an issue, a doc or a comment is in the repo, in
  the same history as the words around it, and in every clone.
- Attachments are not indexed. Nothing lists "every image in the workspace"
  yet, and a deleted page leaves its attachments behind until someone removes
  them — the same as any file in a folder.
- Rendering a pasted image needs the server: a page opened on GitHub reads
  the same relative link and shows the same picture; a page opened as a bare
  file in another markdown tool does too, because the link is relative.
- The token now appears in `<img src>` URLs inside the page. They are
  same-origin and never written to disk, and the token was already readable
  by any script on the page (it sits in `sessionStorage`); nothing new is
  exposed.
- Raising the 1 MB cap is a §8 decision, not a constant to bump.

## Verification

The type is decided by content, not by name — a PNG renamed `.jpg` is a PNG,
and a text file renamed `.png` is not an image:

```
$ printf '\x89PNG\r\n\x1a\n' | xxd -p
89504e470d0a1a0a
$ printf 'GIF89a' | xxd -p
474946383961
```

Pinned by `dit-model` tests (`attachment` module): sniffing, the path
sandbox (traversal, absolute paths, a non-`attachments` parent, SVG, a
foreign root), and the file-name rule; by `dit-core` tests for one commit per
attachment, the 1 MB refusal and the re-upload no-op; and by the server's
security suite for the `?token=` rule on this route only.
