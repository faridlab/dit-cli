//! DIT command-line interface. The CLI is not a second-class citizen: it and the
//! server share exactly the same `dit-core`.

fn main() {
    // `dit fetch-assets` pre-warms optional downloads (the embedding model,
    // §7.1) so a workspace can go fully offline on purpose. Explicit, resumable
    // and inspectable — never a silent fetch at first run. See ADR 0002.
    eprintln!("dit 0.0.1 — scaffold. See DESIGN.md §10 for the v0.1 milestone.");
}
