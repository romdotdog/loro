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
Cursor metadata now prefers an actual still-live Text/plain-List ID in identity
mode, querying committed native state in scalar coordinates instead of replacing
that ID from the shared positional row. This fixes the selected late-child case
where the row omitted the child and moved a cursor one cell left. Container
remaps and retired/deleted IDs retain the existing fallback; general
trigger selection/cursor acceptance is still unfinished. Ordinary mode is
unchanged. This does not introduce retirement activity or new affinity policy.

`crates/loro/tests/identity_trigger_selection.rs` preserves complete main-first
cursor lists through a two-hunk trigger, an independent command and a late
two-hunk child. Text/plain List, ASCII/non-BMP/equal-spelling cells, both peer
orders, a real peer-prefix import and three full cycles are checked. The original
live IDs already resolve correctly; the test also requires the pop metadata to
retain those IDs, not just equal spelling or projected text. Intermediate redo
still uses the native last-popped selection; only the first undo in a loop
captures invocation selection. No new per-command invocation policy is adopted.
Evidence and the old stale-row control are in
`/root/notes/dark-derived-selection-2026-10-28/`.

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

### Retirement must cross the producer boundary

The gap is not confined to one manager's bookkeeping. `peer_retirement` uses
two actual peers/managers: A inserts a; B imports it and inserts B; A imports B
and replaces it with x inside A's derived scope; B imports that replacement.
B's undo consumes its invisible creation, returns false and adds no opposite.
A then imports B's exports and undoes A. Actual is B, desired empty. Without
B's retirement the same continuation must reveal B. This is a separate peer
activity boundary, not a new upstream bug count.

Before/after B's retirement, full updates and full snapshot bytes, incremental
updates from the pre-retirement VV, JSON history, VV and both frontiers are
identical. Snapshot reconstruction also has no retirement record to recover.
The observation is executed for Text/List, both peer orders and a non-BMP cell;
the subsequent A continuation covers binary, snapshot, JSON, batch, incremental
and cold-producer delivery. All 48 explicitly desired continuations fail.
The public Rust feature wrapper reproduces B/empty independently.

This rules out repairing this boundary with a manager-local inactive set or
another restoration-edge lookup alone. Current metadata is attached to actual
native changes; this retirement creates no change. An oplog-only receiver given
identical histories cannot recover which of these two intentions occurred.
It does not establish that a new binary codec layout is the only solution.

The next implementation edge is recoverable, causally ordered contribution
activity, including retirements with no visible text effect. It must come from
actual selected native operations, cross export/import and cold reconstruction,
and obey the writer/publication failure boundary. No caller-supplied inactive
registry, dummy text/user-root mutation or virtual opposite is adopted. A native
activity operation would change the experimental no-op's VV/export contract;
an independently replicated record would widen the current import protocol.
Neither change has been implemented or silently declared compatible. Ordinary
no-op behavior remains fixed. Default passing byte-equality checks document the
information loss, **not acceptance of resurrection**; the desired assertions
remain separately ignored and explicitly executed as failures.

The fresh-review **proposed full-visibility gate** now has a separate native
witness: A undoes its overwrite and actually restores B before learning B's
invisible retirement. Importing that retirement would then have to hide the
already-restored alias, not just affect future inverse planning. Both peers
exchange updates to equal VV/frontiers; a cold reader reconstructs the final
snapshot. Current results retain B on both peers and the reader, rather than the
proposed empty result. The active/no-retirement control must retain B.

`late_retirement_hides_already_restored_peer_contribution` explicitly executes
48 failing branches (Text/List, peer orders, ASCII/non-BMP, six delivery paths).
Delivery is repeated, and cold-producer delivery uses an actual post-retirement
snapshot. This fixture deliberately does not require unchanged retirement
VV/bytes, so it can test a future activity implementation. It does not recover
undo stacks. Full replicated visibility is a stronger contract than merely
consulting activity on a future undo; no new activity policy/protocol has been
adopted by adding this proposed oracle, and no new upstream defect is counted.

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
