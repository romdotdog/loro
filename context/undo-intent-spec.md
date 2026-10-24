# Candidate native undo semantics

Reviewed against code and sources 2026-10-24.

This is a proposed acceptance contract for native repair, not a claim that the
current implementation meets it. It makes h's candidate laws distinguish actual
outcomes. The current regressions and implementation limits remain in
[undo-history-boundaries.md](undo-history-boundaries.md).

## Decisions for the next repair

Keep the original element as the logical target of restoration, including its
historical ordering relations. A physical insert produced by an inverse may have
a fresh operation ID, but equal spelling does not establish logical identity.
Nor does choosing the nearest currently visible neighbor establish the original
position. Hidden elements must remain usable as ordering evidence.

Undo selects an explicit local command's operation ranges, possibly discontiguous.
It changes that command's contribution, not every later operation that observed
it. A derived edit belongs through its explicit trigger identity; happens-before,
adjacency, or insertion inside the same string is not enough. Preserve the settled
500 ms/split-only primary grouping rules independently of this membership rule.

For text, the intended visibility condition is:

```
visible(original_element) =
    creation_contribution_active(original_element)
    and no active deletion contribution targets original_element
```

This describes a reference interpretation, not a new replicated operation format.
It must include deletions with no projected state diff. Undo removes only selected
contributions; redo restores those same contributions, rather than claiming a new
user insertion or a newer user overwrite. Keep all original causal relationships
and insertion-placement evidence when interpreting inactive contributions.
Do not obtain the reference result by replaying old view indices after deleting
operations from the history.

For register-like attributes, prefer preserving another peer's active overwrite:
black -> A:red -> B:green -> undo(A) leaves green. The consumed superseded write
must not make the manager undo an older item. Attribute-range membership, boundary
expansion, and concurrent mark priority still need their own exact contract;
this scalar example does not settle the entire rich-text problem.

## Laws and separating examples

1. **Delivery invariance.** Fix the causal operations, author, selected command
   ranges, history metadata and configuration. Different legal import partitions
   must give equivalent undo results. Derived caches are not extra semantic input.
   Acceptance includes the existing batched/incremental List witness, not just
   convergence after each separately generated inverse.
2. **Independent peer contributions survive.** Undoing A's insertion does not
   delete B's interior insertion. Undoing A's deletion cannot cancel B's deletion
   of the same original element. Hidden metadata may persist even when A's own
   characters disappear. This is narrower than promising every remote operation
   always has a visible effect, including when its parent container is absent.
3. **Historical placement survives restoration.** Both before/after-deleted-a
   witnesses must produce the desired sab/asb pair. The trigger `(`, intervening
   B, late child `)` continuation must finish at (B), not B(), including after
   undoing and redoing both commands. Identity labels and surviving-neighbor
   placement alone do not discharge this law.
4. **Matched round-trip neutrality.** With no intervening normal edit/import,
   undo followed by redo of that same item restores the specified observable
   state. Compare text, attributes and identity-sensitive ordering, not the VV or
   literal operation log. A consumed no-op has no opposite entry under the native
   contract: an arbitrary subsequent redo need not select that item.
5. **Failure neutrality.** Failed application changes neither document nor stack
   membership or published events; retry applies the same selected command.
   Preserve the existing writer gate, rollback and one-item boundaries.

These are required outcomes for the candidate, not passed checkboxes. Scalar
formatting policy is a proposed choice; no new formatting implementation is
accepted here. Shallow history/GC, general containers, performance and a public
trigger API remain outside this repair until separately specified and validated.

## What the further reading changes

- Weiss, Urso and Molli, *Logoot-Undo* (2010), §§4–6
  ([author-hosted paper](https://www.researchgate.net/profile/Pascal_Urso/publication/233882440_Logoot-Undo_Distributed_Collaborative_Editing_System/links/0fcfd50c84f5194937000000.pdf)):
  immutable ordered line IDs separate placement from visibility. Patch activation
  and per-line visibility degrees distinguish overlapping deletions; restoring
  one patch need not restore a line another patch still deletes. This supports
  separating identity/order and active contributions. It does not supply Loro's
  restoration bridge, local stack boundaries or late-child API. Its global undo
  counters and exactly-once dissemination assumptions are not adopted wholesale.
- Yu, Andre and Ignat, *A CRDT Supporting Selective Undo for Collaborative Text
  Editing* (2015), §§3–6
  ([full paper](https://members.loria.fr/CIgnat/files/pdf/YuDAIS15.pdf)):
  explicit operation descriptors survive splitting of string runs. However, its
  default dependency policy hides insertions built on an undone insertion,
  including other users' contributions. That conflicts with our independent-B
  witness. Borrow the need for persistent operation relationships, not automatic
  transitive undo membership. The paper explicitly excludes a formal proof.
- Weidner, *CRDT Survey, Part 2*, Undo/Redo, and *Part 4*, Undo
  ([part 2](https://mattweidner.com/2023/09/26/crdt-survey-2.html#undoredo),
  [part 4](https://mattweidner.com/2023/09/26/crdt-survey-4.html)):
  filtering undone operations requires semantics that tolerate missing causal
  predecessors. Keeping historical placement scaffolding is a concrete remaining
  obligation; “replay only active operations” alone is not an algorithm.
- Yjs v13.6.27, implementation comparison, not a paper or acceptance run:
  [`Item.js`](https://github.com/yjs/yjs/blob/v13.6.27/src/structs/Item.js),
  `followRedone`/`redoItem`, follows restoration chains and allocates fresh physical
  IDs. [`UndoManager.js`](https://github.com/yjs/yjs/blob/v13.6.27/src/utils/UndoManager.js),
  `popStackItem`, can skip no-effect items in a loop. The first is useful native
  precedent to inspect; the second conflicts with our one-item contract. No Yjs
  runtime behavior was tested in this source review.

## Next implementation gate

Before another manager abstraction, expose or reconstruct sufficient native
historical ordering/restoration evidence for the two affinity directions and the
complete B continuation. Validate against real native operations and retained
original IDs, including invisible remote deletion, repeated restoration and fresh
imports. A same-process table that cannot be reconstructed or reconciled from the
agreed history does not satisfy delivery invariance. If existing encoded
operations cannot communicate the needed relation, identify that information gap
explicitly before proposing a format change. No format change is authorized by
this document, and no new application-side intent registry is adopted.
