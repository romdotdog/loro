---
{}
---

Add a Rust-only opt-in `experimental-identity-undo` constructor for the native
plain Text/List identity-history candidate. It replans mixed inverses against
actual historical creation IDs and manager-owned successful restoration links.
The ordinary `UndoManager::new` remains unchanged. This is experimental: one
plain container per item, non-shallow history, no styles or nested containers,
unbounded full-history replay/retention, and no settled selection/general topology
contract. A closed trusted-producer protocol carries actual receipt edges in
existing persisted change messages; strict staged import rejects missing/invalid
metadata before target publication. All editing peers must enable production
before initial edits; untagged existing history rejects. Constructor/producer
enablement are fallible. No undo-stack recovery or retrofit of ordinary histories.
Selected two-manager restoration/import continuations now pass. This must not be
deployed for user data. No Wasm/browser adoption or new binary codec layout.
