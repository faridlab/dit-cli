---
id: 0001
title: Implement Crockford base32 in dit-model instead of depending on `ulid`
status: accepted
date: 2026-08-16
supersedes: null
---

## Context

`dit-model` must compile to `wasm32-unknown-unknown` with zero I/O dependencies
(invariant I4). It needs to parse and inspect ULIDs, and to serialize them.

## Options considered

| Option | Cost | Consequence |
|---|---|---|
| `ulid` with default features | none | Pulls `rand` -> `getrandom`, which does not build for `wasm32-unknown-unknown` without an explicit backend. I4 fails on day one. |
| `ulid` with `default-features = false, features = ["serde"]` | none | Does not compile: `ulid`'s serde impl needs `String`, which its `std` feature provides — and `std = ["rand"]`. No combination gives serde without rand. |
| Implement Crockford base32 here | ~60 lines to own and test | `dit-model` keeps zero non-derive dependencies. |

## Decision

Implement it. `dit-model` depends only on `serde` and `thiserror`.

## Consequences

There is no `IssueId::new()`. Minting an ID requires entropy, entropy is I/O, and
I/O does not belong in the pure core — so generation lives in `dit-store`.

That fell out of the dependency constraint rather than being designed, but it is
the correct boundary and we keep it.

## Verification

```
$ cargo tree -p dit-model -e normal | grep -cE 'rand|getrandom'
0

$ sed -n '/^\[features\]/,/^\[/p' ~/.cargo/registry/src/*/ulid-1.2.1/Cargo.toml
[features]
default = ["std"]
std = ["rand"]
```
