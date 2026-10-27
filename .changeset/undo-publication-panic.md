---
{}
---

Keep undo/redo history and actual restoration receipts after unwinding inverse
publication callbacks. Complete commit, event/update delivery and opposite-item
bookkeeping before resuming the first pre-commit/subscriber/metadata panic.
Application errors still roll back; no-op/activity semantics are unchanged.
