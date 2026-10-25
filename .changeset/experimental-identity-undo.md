---
{}
---

Add a Rust-only opt-in `experimental-identity-undo` constructor for the native
plain Text/List identity-history candidate. It replans mixed inverses against
actual historical creation IDs and manager-owned successful restoration links.
The ordinary `UndoManager::new` remains unchanged. This is experimental: one
plain container per item, non-shallow history, no styles or nested containers,
manager-lifetime in-memory links only, unbounded full-history replay/retention,
and no settled selection or recovery contract. A known cross-manager restoration
import case still fails; this must not be deployed for user data. No Wasm/browser adoption.
