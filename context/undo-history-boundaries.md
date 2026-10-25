# Undo/redo history boundaries

Verified against code 2026-10-25 (restoration recovery diagnostic; earlier scopes below).

`crates/loro-internal/src/undo.rs`: `UndoManager::perform` processes at most
one recorded `StackItem` per call to `undo` or `redo`. Grouping and the merge
interval can put several edits, including edits to different containers, into
that one item.

Posted upstream repros are indexed in
[scripts/undo-issue-repros/README.md](../scripts/undo-issue-repros/README.md), with
unaltered JS code blocks and native schedule translations. The benchmark replay
explicitly includes known failing desired outcomes; a default run that skips
them is not closure. This covers reports #1227–1229 and #1231–1234, including the
second #1234 round-trip witness. It supplements, rather than replaces, native
continuation, rollback and no-op contracts below.

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

A different native failure was reproduced: start with aB shared by A and B. A replaces
B with x while B concurrently deletes the original B. Importing B's deletion
leaves A visibly unchanged at ax, but undoing A restores aB rather than a.
`undo_text_import::concurrent_delete_of_locally_overwritten_text_is_not_restored`
preserves the desired assertion for both peer-ID orders and ASCII/non-BMP text.
The one-span and selected dependency-separated group cases now pass, including
repeated undo/redo; the original desired group oracle is no longer ignored.
This is not a new upstream issue count.

`src/undo/text_intent_tests.rs` locates the loss boundary without changing the
inverse: for both peer orders and ASCII, non-BMP and multi-scalar runs, the import
advances the native version vector but emits no root state-diff notification.
Scanning actual native operations finds both local and remote deletion ranges
naming the same original creation IDs. Import did not discard the remote delete;
the manager's state-diff transform input cannot express its intent. This audit
passes independently of the public undo oracle.

The scoped native repair in `src/undo/text.rs` uses original text-operation
identity/removal intent for non-shallow history. It gathers IDs deleted by all
local spans, scans other peers' operations since the first span's dependency
version, and intersects their deletion targets with those IDs. While checked out
at each raw inverse's output, native ID queries identify its restored targets.
Private `Annotated` batches carry creation IDs and suppression flags, with one
mark per event unit (Unicode natively, UTF16 with the wasm feature). Raw inverse
capture reads IDs from the actual checked-out output, including equal-spelling
characters. Native delta composition carries the marks across selected spans.
For transform, retain the ordered insertion-ID stream and assign its positions
from the actual transformed text delta. Native transform preserves that stream;
unit counts and scalar boundaries are checked. A separately rebuilt ArrayVec
bridge can normalize adjacent delete/insert chunks differently from StringSlice,
and give different OT placement despite equal final text. It is therefore not
a sound second source of transformed positions. Identity capture does not rebuild
otherwise untouched text deltas: only the existing suppression-eligible container
scope enters filtering. Public inverse application remains positional.
Final filtering asserts inserted-unit count, output position and scalar agreement
between text and provenance. Input positions are not the right invariant: delta
normalization may commute adjacent insertion/deletion chunks while preserving
their output positions. The repeated-cycle regressions exercise that distinction.
Filtering runs only after all spans have composed, before `on_last_event_a` and
the final remote transform. It changes no intermediate coordinate base. Retains,
deletion lengths and text attributes are preserved; precheck, application and
rollback still use the existing native path. History rebasing sees the filtered
inverse. Public `DiffBatch` and encoded operations are unchanged.
No caller hook, new CRDT operation kind or text-restoration registry is added.

Public regressions cover partial run deletion, remote suffix insertion, plain/
non-BMP text, peer-ID ordering, attributes and repeated cycles. A wholly redundant
deletion consumes its one no-op item without an opposite entry or extra CRDT op.
The internal application-failure regression checks both single-span and grouped
inverses: repeated failures leave value, versions, event silence and history
intact, then retry/redo succeed. The grouped matrix includes three dependency-
separated spans, partial deletions, peer ordering, Unicode, attributes and repeated
cycles. A long-run case crosses provenance chunk boundaries. A grouped net-zero
case preserves the one-item/no-op boundary without virtual opposite entries.

Further repairs must retain original native text-operation identity/removal intent,
including operations invisible in state diffs. Do not infer remote deletion
from before/after text alone. The existing MovableList capture/plan path in
`LoroDoc::undo_internal_with` is a native integration example, not a text proof.
Keep its held transaction, unknown-container precheck, rollback and one-item/no-op
contracts; the parked Dark sidecar inverse and virtual opposite entries are not
an adopted replacement for those contracts.

Filtering a span's restored text before composing several span inverses is not
automatically sound: the following inverse still expects that span's original
output coordinates. A repair must carry target identity through the whole
composition or prepare a native identity-based text plan; passing only the
single replacement witness would not justify changing grouped calculation.
`foreign_delete_intent_survives_dependency_separated_group_spans` retains the
original desired native oracle: overwrite B→x, import B's invisible delete plus
a disjoint map update, then x→y. One undo now yields a. Dropping marks during
composition recreates the exact aB failure; passing a single-span test alone
does not validate this boundary. Shallow histories and same-peer excluded edits
remain outside the foreign-delete repair. Late causal-trigger membership is not
implemented by this carrier: a manager still records its existing local spans.
Scanning history and per-ID positional queries also have unbounded long-history
costs; no indexing/performance bound or WASM/browser acceptance is established.

### Restoration origins available to native calculation

Verified against code 2026-10-24.

The private inverse calculation returns `Annotated` through the last transform,
instead of discarding provenance before returning to native orchestration.
`loro.rs` still applies its ordinary `DiffBatch`; private receipts now connect
origins to actual emitted IDs, but no placement plan, persistent physical-ID remap
or trigger API uses those edges yet. An origin is the physical creation
ID present at that raw inverse's output frontier, **not** a transitive logical
identity. On a subsequent restoration cycle it can name a previous restored copy.
Missing IDs remain explicit `None`; a future plan must not silently infer them
from spelling or assume shallow-history support.

The three `calculated_restoration_origins_*` native tests observe this actual
calculation, not an independently rewritten inverse. They distinguish identical
ASCII/non-BMP cells, discontiguous selected deletion spans around an unselected
insertion, a later unselected deletion changing the final transform, grouped
invisible foreign deletion filtering, and a repeated physical-restoration cycle.
The same focused tests run on a native host with the `wasm` UTF16-index feature;
that is indexing evidence, not a Wasm binary/browser acceptance run. Observers
only collect the computed origins; they neither supply IDs nor change placement.
The complete (B) continuation still fails separately. Capturing the right origin
at the wrong positional output does not satisfy the identity/placement spec.

### Native applied-restoration receipts

Verified against code 2026-10-24.

`text::PendingRestorations` keeps the calculated origins provisional. Immediately
after each successful native text-handler application, while undo owns the writer
gate and rollback scope, it reads actual creation IDs at the insertion positions.
Receipts record source/target container IDs, the raw physical origin, the emitted
ID and the event position at that handler boundary. An emitted ID must belong to
the current peer and not precede the inverse's starting counter; missing or older
IDs return an application error through the existing whole-inverse rollback.
This does not guess counters from string lengths or match equal spelling.

Only a successful inverse returns these private receipts on `CommitWhenDrop`.
Dropping the existing guard commits as before; receipt collection does not add a
separate commit or notification. Failed applications/receipt validation return
no guard and discard provisional edges. Containers skipped by existing native
reachability rules produce no receipts. The manager does not yet consume or
persist them; receipt edges are not serialized in operations or snapshots.

The six `applied_restoration_receipts_*` tests check emitted IDs against real
state through discontiguous selection, equal-spelling ASCII/non-BMP cells,
repeated physical restoration and fresh import, style operations, filtered
foreign deletion/no-ops, unreachable text and repeated failure/retry. A named
test-only control points a receipt at an older retained equal-spelling cell:
validation rejects it after the real insertion, then value/VV/frontiers/event
silence are restored and a correct retry succeeds. Import preserves the emitted
IDs, not the private receipt edges. No general reconstruction, transitive
logical-identity map, historical placement or selection/group API is accepted.

## Private selected-range kernel; causal API remains open

Verified against code 2026-10-24.

`LoroDoc::undo_internal_ranges_with` accepts explicit ordered, nonoverlapping
operation ranges from one peer, splitting each on native dependencies before
using the existing whole inverse, precheck and rollback path. The original
single-span entry delegates to this kernel. Empty, overlapping, unordered,
mixed-peer and nonexistent selections reject without changing the document;
the invalid-endpoint path drops its transaction guard before auto-commit renewal.
This is private native plumbing, not a trigger-token or manager-membership API.
The existing manager's recording, metadata, retirement and stack order are
unchanged. Selective unknown-container rejection/rebasing and general selected-
range integration remain unaccepted boundaries before exposing an interface.

`src/undo/selected_span_tests.rs` explicitly selects the `(` trigger and its late
`)` child around an intervening local `B`. Their native inverse leaves B, and
reversing that inverse restores (B), across three runs. The two selected ranges
are not widened over B, and no paused-manager or caller-side inverse is used.
Application failure/retry and selection-validation tests pass separately.

The decisive continuation still fails: undo B to (), undo both selected trigger
segments to empty, redo them to (), then redo B. Native selected-range inversion
produces B() instead of (B). The desired assertion is retained as an explicitly
ignored test. This command fails:

```sh
cargo test --locked -p loro-internal --lib \
  undo::selected_span_tests::redo_intervening_command_after_selected_trigger_roundtrip \
  -- --ignored --exact
```

This is a limit of the candidate representation,
not an additional upstream bug count. Choosing spans addresses membership but
does not preserve interior placement through delete/reinsert identities.

Next work must establish native restoration lineage/placement against this
unchanged continuation before adopting a manager API or a Dark trigger registry.
Do not reset positional diffs because the source string returned to the same
spelling, and do not revive the rejected per-trigger paused-manager plan.

## Delivery partition and tombstone-affinity review

Verified against code 2026-10-24.

`crates/loro/tests/undo_delivery_review.rs` ports h's List delivery counterexample
and mirrored before/after insertion witnesses to the current native fork. Both
delivery schedules have source sx and VV {1:4, 2:4} before undo. List restores
sax with batched imports and asx with incremental imports. Each resulting history
still converges with its peer and agrees with a fresh full import. This is undo
delivery-partition dependence, not demonstrated CRDT convergence failure.

For ab, delete a locally while another peer inserts s before or after the
original a. Both projected import diffs are identical at the receiving replica.
Text and List restore asb in both cases; the fork's native MovableList plan
restores sab in both cases. Thus ID-aware surviving-neighbor placement alone also
does not establish the desired pair sab/asb. The desired Text affinity pair and
List delivery invariance are retained as explicitly ignored failing assertions;
the passing diagnostic verifies the observations/replica convergence, not those
unmet properties. Released-version sweeps from h's transcript were not rerun.

`Stack::compose_remote_event` accumulates import deltas; inversion transforms
against the accumulated delta later. It does not move a saved index after each
individual delivery. The identical-projection witness shows the information
missing from that input; it is not a general impossibility theorem about OT or
about the whole UndoManager, which still has access to the richer native history.

A rewrite should preserve original creation/deletion intent, restoration lineage
and historical gap/ordering information, retaining the checked writer gate,
atomic application/retry and history boundaries. Delivery partition invariance,
both tombstone-affinity directions and the selected B()/(B) continuation belong
in its acceptance suite. The no-local-delete baseline motivates an affinity
policy; it is not a universal specification of concurrent formatting/removal
undo. An ID-based cursor or the word "tombstone" does not by itself repair the
placement/lineage boundary. No manager rewrite has been adopted by this review.

### Historical cursor is not a restoration-lineage substitute

Verified against code 2026-10-24.

`selected_span_tests::historical_interior_ids_survive_import_but_not_restoration_cursor_placement`
locates this boundary on real native history, for peer IDs 1/2 and ASCII/non-BMP
interior text. After undoing B, undoing the selected delimiters, and restoring
the delimiters, the current source is (). Its delimiters have fresh IDs, at
positions 0 and 1. Resolving the original B cursor instead returns position 2,
outside the restored pair, both locally and after a fresh full import. Cursor
queries leave VV/frontiers unchanged. Checking out the imported history at the
original frontier recovers (B), including all three original creation IDs.
Thus original order evidence survives export/import, but the existing historical
cursor alone does not recover the desired interior slot. The inverse's actual
continuation still gives B(); cursor-only insertion would choose the other
outside slot, ()B. This diagnostic does not infer general restoration lineage
from history: the fixture explicitly knows the selected ranges and frontier.
The desired (B) continuation remains separately ignored and explicitly failing.

## Restoration recovery information boundary

`undo::selected_span_tests::restoration_and_fresh_typing_can_export_identical_native_history`
constructs two alternative executions from one identical non-shallow prefix:
insert aa, delete its first a, then either apply the actual native inverse or
type a fresh a into the same slot. The inverse returns an applied receipt naming
the deleted original ID; fresh typing does not perform an inverse. With timestamps
disabled and change merging disabled, both executions export **byte-identical
complete native updates**, including physical IDs and dependencies. Fresh imports
recover the same source and VV. Both peer IDs and ASCII/non-BMP cells are covered.
These are alternative executions of a single peer, never merged together; the
test does not authorize peer-ID reuse between live writers.

Thus the current exported updates alone cannot distinguish these two execution
intents. Matching spelling, creation IDs or history dependencies cannot recover
the missing receipt edge in this witness. `CommitOptions::origin` is transient;
the native inverse's undo origin does not supply a persisted discriminator.
This does not prove the candidate laws inconsistent: they explicitly allow
history metadata as semantic input. Retaining receipts with recoverable history
metadata, or defining an explicit persistent restoration representation, remains
a design choice. It does not follow that a wire-format change is necessary for
a source-local history manager, nor that arbitrary remote lineage is already
recoverable. No encoding or manager behavior is changed by this diagnostic.

## Native historical-order ingredient (test-only)

`diff_calc::historical_{text,list}_id_order` rebuild native trackers through the
existing causal replay and expose their insertion order, including tombstones.
They do not replay stored indices into a history with operations removed. The
test-only tracker traversal excludes unknown-history placeholders; the wrappers
reject shallow history. Both materialize entire histories without a cost bound
and are not production queries or an adopted placement API.

`native_historical_list_order_retains_delivery_partition_landmark` runs h's
unfiled List schedule under both delivery partitions. Counting the actual live
IDs before the original deleted ID in that native order locates slot 1 in both
cases. A fresh full import reconstructs the same order; the query leaves VV and
frontiers unchanged. This identifies available placement evidence, not a fixed
UndoManager: the separate desired delivery-invariance benchmark still fails.
`native_historical_text_order_separates_mirrored_deleted_target_slots` also
uses the actual calculated inverse origin in both mirrored Text cases and peer
orders. Its native order yields different slots and separate manual applications
produce sab/asb. This is the same test-only placement experiment, not evidence
the ordinary inverse already respects those slots.

`native_historical_order_and_applied_lineage_locate_restored_interior` combines
that native Text order with actual applied receipts from three delimiter
restoration cycles. It follows receipt chains to original IDs and weights those
positions by current live IDs. The B/non-BMP interior target then locates slot 1;
using physical IDs without lineage does not. Both peer IDs are tested. The normal
inverse is executed separately and still gives B(). A separate replica imported
from the pre-redo prefix performs a real text insertion at the experimental slot
and exports/imports (B). This manual test-only application is **not** a repaired
native inverse, retained manager metadata, or a recovery implementation. The
receipt edges are carried separately by the experiment; import reconstructs
order, not those edges. No fixture-authored aliases or character matching are
used, but general insertion topology after restoration, multiple target hunks,
attributes, admission/rollback and integration remain unestablished. This is a
bounded ingredient for a native placement plan, not acceptance closure.

## Prior work and candidate specification laws

Source review 2026-10-24; all three papers read in full. These are specification
inputs, not adopted semantics or implementation verification.

The further-search recommendation and separating acceptance examples are in
[undo-intent-spec.md](undo-intent-spec.md); its proposed laws are not all met.

- Stephen Dolan, [The Only Undoable CRDTs are Counters](https://arxiv.org/abs/2006.10494)
  (2020), §§2–4 and appendix: under its operation-based model, commuting partial
  primitive operations with finite inverse sequences returning to the same
  abstract state give an abelian group of actions. With finitely many primitives,
  the resulting structure is a tuple of integer/modular counters. This does not
  prohibit useful editor undo that grows internal history while restoring an
  observable value; nor does it establish that the candidate laws below conflict.
- Leo Stewen and Martin Kleppmann,
  [Undo and Redo Support for Replicated Registers](https://arxiv.org/abs/2404.11308)
  (2024), §2 Fig. 1(d), §§3–4, §7: their chosen local register undo restores the
  state preceding the selected write, even when that hides another user's later
  write. Starting black, A writes red, B observes it and writes green, A undoes:
  black; A redoes: green. Thus this paper does **not** establish the candidate
  law that undo preserves another peer's visible overwrite. Under that candidate,
  A's undo would instead leave green. Their restore-operation DAG and recursive
  predecessor traversal are concrete prior work, not a Loro range-formatting proof.
- Weihai Yu, Victorien Elvinger and Claudia-Lavinia Ignat,
  [A Generic Undo Support for State-Based CRDTs](https://drops.dagstuhl.de/entities/document/10.4230/LIPIcs.OPODIS.2019.14)
  (OPODIS 2019), §§5–7: retain monotone state and attach undo lengths to
  join-irreducibles; queries filter inactive contributions and their dependants.
  The text example tracks insertion and each site's deletion separately: undoing
  A's deletion does not reveal a character still deleted by B. This is relevant
  to the invisible-deletion witness. §6.1 restricts normal operations to one
  join-irreducible delta and excludes composite operations from its treatment;
  native compound edits, grouping and local stack policy need additional work.

h proposed four laws. Before treating them as proof obligations:

1. History determinism must fix the causal oplog, selected stack membership,
   replica identity and policy/metadata inputs. Delivery partition and derived
   caches must not choose the result. Grouping clocks can form membership but
   cannot justify a different inverse once membership is fixed.
2. Peer noninterference needs observable conflict rules. Keeping remote operations
   in the immutable log alone is insufficient. Register overwrite and concurrent
   deletion examples distinguish substantive policies.
3. Restoration identity must distinguish an original logical element and its
   historical order from a physical creation operation ID. Current text inverse
   application creates new IDs. A lineage relation would be a proposed repair,
   not evidence that original identities are already restored.
4. An uninterrupted, matched undo/redo pair should restore the specified observable
   state, not literally rewind the oplog/version vector. Specify text, attributes,
   ordering and logical identity observations. A consumed native no-op has no
   opposite entry, so calling redo afterward need not select the same item;
   the existing no-op boundary must remain explicit in this law.
