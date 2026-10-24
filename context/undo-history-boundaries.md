# Undo/redo history boundaries

Verified against code 2026-10-17.

`crates/loro-internal/src/undo.rs`: `UndoManager::perform` processes at most
one recorded `StackItem` per call to `undo` or `redo`. Grouping and the merge
interval can put several edits, including edits to different containers, into
that one item.

Remote edits can make the item's inverse a no-op. For example, inserting a
MovableList element locally and then deleting it remotely leaves nothing for
the inverse delete to remove. A move/set whose element was deleted remotely
also cannot write an inverse. That item's lack of generated operations must
not cause `perform` to pop another item: the next item may create an unrelated
parent container, which would then disappear in the same undo call (#1183).

A consumed no-op still runs the existing `on_pop` and cursor bookkeeping. It
returns `false`, generates no opposite-stack entry, and leaves other items for
later calls. Callers must check `can_undo` / `can_redo` to decide whether history
remains. The public Rust and WASM wrappers share this implementation.

`loro-js/src/runtime/undo.ts`: `UndoManager.undo` and `redo` also consume one
item. Their result is whether `#invert` produced an opposite-stack item, not
whether an item was popped. An empty inverse returns `false` while preserving
pop callbacks, tracked-range cleanup and the remaining history.

`LoroDoc._subscribeUndo` in `loro-js/src/runtime/document.ts` also delivers
local commits with empty state diffs to the manager, while public state
subscriptions remain silent. Without this notification, a local MovableList
insertion and deletion grouped into one item is never recorded, and undo
instead consumes an earlier item. This is the `insert/delete undo (one peer)`
differential profile, seed 2, step 33; changing the boolean return alone does
not fix it. The subscription cleanup removes both listeners.

This boundary rule does not change grouping or error handling. MovableList
identity replay (#1209) deletes an inserted element even if it moved remotely,
so remote movement alone is no longer a no-op trigger. The regression tests
use remote deletion instead, including a missing move target for redo.

Regressions:

- `crates/loro/tests/integration_test/undo_test.rs`: `noop_history_boundaries`
  covers undo before a scalar edit or parent creation, redo before a parent
  creation, unchanged versions for no-ops, subsequent history and convergence.
- `crates/loro-internal/tests/undo.rs`:
  `consecutive_noops_consume_one_item_and_notify_once` covers multiple no-ops
  and one pop callback per item.
- `crates/loro-wasm/tests/undo.test.ts` covers the exposed JS methods.
- `loro-js/tests/undo-noop.test.ts` covers no-op undo and redo return values,
  unchanged versions/frontiers, pop callbacks, subsequent parent history and
  notification cleanup when the manager is destroyed.

## Automatic grouping and clock retreat

Verified against code 2026-10-24.

`UndoManager::record_checkpoint` only automatically merges when the configured
interval is positive and checked elapsed wall-clock time is nonnegative and
strictly below that interval. A clock retreat splits rather than merging
unrelated edits (#1227); subtraction overflow also splits. Explicit groups
continue to merge through their separate group rule. Private native regressions
in `src/undo/clock_tests.rs` simulate a recorded previous timestamp in the future
at intervals zero and 500, and check explicit grouping separately.

## Grouped inverse frontier

Verified against code 2026-10-24.

The native `undo` helper in `src/undo.rs` splits a local item into contiguous
spans separated by external dependencies. After inverting one span, carry the
inverse to the next span's **dependency frontier**, not its last operation.
The next inverse ends at that frontier, so its output composes directly with the
carried inverse. Transforming the carried inverse again against the next inverse
can preserve recreated intermediate text (#1228).

`crates/loro/tests/undo_dark_history.rs` checks replacement chains, remote map
updates dependent on intermediate/final states, independent and dependent text
insertions, deletion and net-zero remote updates, and split delete/insert
commits. It also checks redo, remaining group boundaries and remote map values.
These schedules do not establish arbitrary text-import behavior inside an open
group, identity-sensitive concurrent deletion, or late-derived-edit membership.
The patch changes inverse calculation only; it retains native application,
rollback and one-item/no-op bookkeeping.

## Text import limits and the next native repair

Verified against code 2026-10-24.

`UndoManager::new`'s import subscription clears the active group when an imported
state diff touches an affected container. `Stack::push_with_merge` also separates
rows when the accumulated remote diff overlaps the group. Therefore an open
manual group does not promise one item across an overlapping text import.
`undo_text_import::overlapping_text_import_ends_the_active_group` demonstrates
this: y| → yp| → import S → yqS| needs two undos, first ypS| then yS|.
That first result is the earlier item's text, not by itself evidence of a bad
inverse. The explicit split-before-import control has the same history boundary.

A different native failure remains: start with aB shared by A and B. A replaces
B with x while B concurrently deletes the original B. Importing B's deletion
leaves A visibly unchanged at ax, but undoing A restores aB rather than a.
`undo_text_import::concurrent_delete_of_locally_overwritten_text_is_not_restored`
preserves the desired assertion for both peer-ID orders and ASCII/non-BMP text.
It is explicitly ignored pending repair; running it with `--ignored` fails.
This is not counted as a passing regression or a new upstream issue family.

Next repair must use original native text-operation identity/removal intent,
including operations invisible in state diffs. Do not infer remote deletion
from before/after text alone. The existing MovableList capture/plan path in
`LoroDoc::undo_internal_with` is a native integration example, not a text proof.
Keep its held transaction, unknown-container precheck, rollback and one-item/no-op
contracts; the parked Dark sidecar inverse and virtual opposite entries are not
an adopted replacement for those contracts.
