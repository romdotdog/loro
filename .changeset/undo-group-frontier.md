---
{}
---

Fix Rust UndoManager grouped undo restoring text that existed only between commits when a remote update depends on the intermediate state. Carry each span's inverse to the next span's dependency frontier and compose there without transforming it twice.
