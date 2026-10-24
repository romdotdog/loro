---
{}
---

Initially preserve other peers' concurrent text deletions when undoing a local overwrite in non-shallow history whose native inverse has one continuous span. Read original deletion target IDs even when the import produced no visible state diff, and omit only those characters from restoration. Preserve partial-run survivors, attributes, native rollback and one-item/no-op history semantics. The grouped-provenance change extends this repair to dependency-separated spans.
