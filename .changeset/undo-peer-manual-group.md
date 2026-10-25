---
{}
---

Fix Rust UndoManager splitting a manual group after changing peer IDs. Peer
change still clears both stacks, but an already-open group restarts at the new
peer's counter with no stale affected containers. It neither retains old-peer
items nor opens a group when none was active. The original #1231 desired native
reproduction and additional public counter/import/redo cases now pass. No
experimental activity-event protocol or invisible-no-op semantics change.
