---
{}
---

Enforce the undo-stack bound when redo creates a new undo item, including zero/one limits. Reset the merge timestamp when a requested merge actually starts a new item after a remote import. Preserve redo history, explicit grouping, one-item/no-op semantics and failed-inverse rollback; no historical placement or identity repair is implied.
