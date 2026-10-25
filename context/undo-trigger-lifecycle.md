# Experimental native trigger membership

Verified against code 2026-10-25. This is normal-built Rust on the identity
candidate, not an adopted Dark history implementation. Ordinary UndoManager
behavior is unchanged. Do not deploy for user data.

## What the caller holds

After a primary item is committed, `capture_trigger()` returns an opaque
manager-local `UndoTrigger`. Pass it into synchronous `with_derived` work when a
late result is ready to edit the document:

```rust,ignore
let undo = UndoManager::new_identity(&doc)?;
text.insert(0, "(")?;
doc.commit();
let opening = undo.capture_trigger()?;
// Another primary command, or an import, may happen here.
undo.with_derived(&opening, || {
    text.insert(2, ")")?;
    Ok::<_, LoroError>(())
})??;
```

The example is public Rust API shape; exact insertion positions remain the
caller's admitted edit, not an anchor/cursor/rebasing API. The runtime tests use
actual native Text/List handlers. A future/async job carries the token and calls
`with_derived` around the synchronous native edits on completion; returning an
unpolled future from that closure does not extend the scope across an await.

The token names a retained history item, not a view position or caller-supplied
counter range. It survives that item's successful undo/redo roundtrip. It cannot
be used with another manager; inactive, cleared and evicted tokens reject before
running the work. Token capture does not change primary grouping policy. A token
may name an already-coalesced item. The source-local undo stack/tokens are not
serialized by the restoration-metadata protocol.

## Actual native ownership boundary

The scope owns the document's native writer gate, checkpoints earlier pending
primary edits, then annotates **before** executing the child. Native queries,
explicit commits and implicit barriers inside the work keep that annotation.
The manager captures the actual local counter interval across those commits;
no fixture provides ranges, old/fresh identity relations or inactive flags.
A net-zero child with native operations is retained in membership even without a
projected state event. Empty work adds no range or item. No child calls on_push,
starts a timer/group or clears a waiting redo stack.

This is grouping, not an atomic edit transaction. A closure's returned error
value does not reverse earlier native edits/commits. On ordinary Rust unwinding,
remaining native effects are finalized and attributed before the original panic
is resumed; selected work/subscriber panic cases are tested. This is not general
callback-reentrancy, poisoned-engine, trap or OOM recovery. A callback that clears
history during the commit can make attribution fail after publication; the error
states that the child already committed. Application failure/publication proofs
must account for this boundary before adoption.

A StackItem retains its first span and additional actual selected segments.
Undo passes the complete ordered selection into the existing native range kernel.
Redo pushes the actual inverse span with the same trigger identity. Candidate
primary coalescing may add a later segment around another trigger's child,
without widening the item over those excluded counters. Ordinary single-span
stack behavior is unchanged. Primary clock/Enter/direction/multi-selection policy
is still the settled application rule, not implemented by token allocation.

A shared positional remote-diff row cannot treat a late edit as both selected
for one item and external to another. Candidate inverses therefore query actual
native gaps through the current history head rather than use that cached row as
semantic input. Derived events do not create an external row that would itself
split primary grouping. Restoration origins, complete-inverse provenance,
metadata recovery, native application handlers and rollback remain in charge.
This does not repair cached selection coordinates; trigger selection/cursor
acceptance is still unfinished.

## Executed continuation and explicit gaps

`trigger_history` executes the original `(`, intervening B, late `)` schedule
through one actual candidate manager. Undo B → (), undo trigger → empty, redo
trigger → (), redo B → (B). Text/List, both peer orders, a non-BMP middle,
intervening peer insertion/import and repeated cycles pass. It also covers
multiple/net-zero children, real commit/export barriers inside a child, a token
reused after redo, retained primary coalescing across another trigger's child,
foreign/stale/evicted tokens and selected scope panic cases. Native injected
post-handler failures preserve both stacks' entire span membership/tokens,
source/versions and the prior restoration cache before retry.

Two **desired** hidden-creator tests are explicitly run and fail: A inserts a,
B inserts B, A's child replaces B with x; undo B is an invisible one-item no-op;
undo A then restores B, desired empty. Membership and placement links alone do
not preserve the consumed B creation's inactive intent. No virtual opposite is
silently added: native no-op retirement remains unchanged. Native creation/
removal activity through retirement is the next representation gap. Preserve the
earlier connected prototype's empty-source/hidden-redo oracles; that prototype's
virtual logical opposites are not adopted here.

A separate retained **policy-extension proposal** creates the closing child
while B is already undone, then asks B's redo to land inside it. Actual is ()!B,
proposed (B)!. This changes the original claim: the new child never observed B
live. Membership alone does not specify wrapping or this insertion tie. Its
ignored assertion remains explicit evidence, not an accepted-policy repair or a
new upstream defect. No general topology/tie rule is adopted from it.

Commands and failures are indexed in
[scripts/undo-issue-repros/README.md](../scripts/undo-issue-repros/README.md).
The original primary benchmarks and recovery/import cases remain acceptance
prerequisites. No browser/Wasm integration, Dark pin, runtime deployment,
application proof, GC bound or whole-history correctness claim accompanies this
API. The 500ms/split-only and live-preedit rules are unchanged.
