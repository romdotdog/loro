---
{}
---

Finish undo/redo stack bookkeeping before resuming unwinding metadata callback
panics. Failed push callbacks use default metadata; committed edits are not rolled
back, and invisible no-op undo still consumes one item without an opposite.
