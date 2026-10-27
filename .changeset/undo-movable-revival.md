---
{}
---

Fix ordinary Rust MovableList undo confusing a remotely revived original with
an older undo-created replacement (#1234). Restoration links retain actual
creation IDs and resolve against each reference's observed native version;
new operations and old anchors keep their own branches through repeated undo
and redo. Application does not globally remap already-planned physical IDs.
The existing writer gate, whole-batch preflight, exact error rollback and
one-item/no-op semantics are retained. No codec, experimental activity protocol
or Dark pin/deployment change is included.
