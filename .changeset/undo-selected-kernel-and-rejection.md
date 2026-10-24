---
{}
---

Release the native undo preparation transaction guard before renewing auto-commit after an invalid history span, so rejection leaves the document usable. Factor the inverse implementation through a private validated range-selection kernel for further causal-history work; the public UndoManager's membership and stack semantics remain unchanged, and selective redo placement is not yet repaired.
