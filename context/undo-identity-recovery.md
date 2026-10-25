# Experimental identity-history recovery and admission

Verified against code 2026-10-25. Rust-only candidate, not Dark adoption or a
whole-history proof. Ordinary documents and `UndoManager::new` are unchanged.

## The object that crosses import

An applied inverse creates fresh physical IDs. The native candidate's actual
handler receipts say which older creation each fresh cell restores. Previously
those edges lived only in one manager: a second manager restored parentheses,
but its peer could not place an older B inside them after importing operations.
The desired sequence was `(B)` then `(XB)`; the first inverse gave `()B`.

`undo/recovery.rs` now carries those **actual source-local receipts** in the
existing persisted change message. It does not infer an edge from equal text,
put a registry in user data, or change Loro's binary/JSON codec schema. Metadata
changes complete update bytes; the earlier observation that ordinary native
undo and fresh typing could export identical bytes remains valid for the
ordinary path. A new wire-format layout was not necessary for this experiment.

This is a closed, trusted-producer protocol. Before initial edits, every editing
peer calls `LoroDoc::enable_identity_history()?` (public Rust wrapper gated by
`experimental-identity-undo`). `UndoManager::new_identity(&doc)?` also enables
it. Both are fallible: existing untagged history, shallow/pending history and
detached startup reject. There is no “all pre-existing insertions are fresh”
bootstrap allowance and no retrofit of missing intent. A fully tagged imported
history can be reopened; recovery of the undo **stack** is not implemented.

The mode lasts for the document, including after manager drop and attached
`fork()`. Each subsequent local commit is stamped after user pre-commit message
modification. An ordinary commit declares its creations fresh; a candidate
inverse declares all its actual restoration edges. Ordinary UndoManager inverse
generation on a tagged producer rejects before application, rather than labeling
an unrecorded ordinary restoration as fresh. `apply_diff`/manual replacement is
ordinary authored data, not an identity-restoration declaration. Historical
checkout/fork-at lifecycle acceptance is not claimed.

## Record and trust contract

The reserved message prefix is `loro:identity-undo:1\n`. Its strict JSON record
contains the producer's peer/counter interval, inverse flag, restored→origin ID
pairs and the original user message nested as `message`. Metadata can therefore
appear in existing change-message APIs. Per-change ranges prevent native merging
of differently stamped commits; they do not set application undo grouping.

The producer supplies edges only from `CommitWhenDrop`'s successful native
handler receipts, not a public relation setter or the user message. Failure
before the rollback scope succeeds publishes neither operations nor metadata.
The ordinary transaction commit stamps fresh commits automatically. Restored
payloads/containers are recovered from actual history on the receiver.

For every received available change/slice, recovery checks:

- exact metadata version/schema and compatible producer counter interval;
- no missing tags, duplicate/invalid edge IDs or edges on a fresh declaration;
- each consumed edge names this change's insertion and a causally prior creation;
- same plain Text/List container and exact scalar/List value at both IDs;
- every insertion in an inverse slice has an edge; nested-container edges reject;
- full parsable history without pending dependencies or shallow roots.

Native export may slice a change; the record covers its original interval and
only edges in the available slice are consumed. Future slices are checked when
available. Causal precedence prevents restoration cycles. Payload equality is a
consistency check, **not evidence of intent**: a dishonest producer could label
fresh typing as restoration of a causally prior equal cell, or falsely declare
an inverse fresh. No signature, authentication or independent proof of a remote
writer's intent is supplied. The caller must trust admitted producers to execute
this protocol and preserve metadata; application membership/authentication and
protocol negotiation remain unimplemented. Arbitrary JSON is not certified safe
by these structural checks.

## Import and failure boundary

Tagged documents preflight binary imports, JSON imports and whole binary batches
in an isolated full-history snapshot, under the target's transaction writer
barrier. They run ordinary native decode/apply there, then validate the resulting
history/metadata before importing anything into the target. Missing dependencies
reject with a resync request; no pending queue is installed. Untagged peers,
missing/malformed records, unsupported versions and invalid edges return `Err`.
A valid prefix followed by bad metadata in one batch does not publish the prefix.
The normal pending-user-edit commit barrier still happens before admission;
rejection does not undo already pending user edits.

The existing real import rollback remains in charge if actual application fails.
Tagged batch reattachment emits **Import**, not user Checkout: clearing the local
undo stacks at that internal checkout lost the original items in the batch
continuation. Ordinary batch behavior is unchanged. User checkout still has its
existing stack-clearing semantics.

Before selecting an inverse, the manager reconstructs edges from committed
history under the writer gate. Its in-memory relation is a derived cache, not
an authoritative fixture. It replaces that cache and appends the successful
step's actual receipts only after the real commit. Failed applications leave the
previous cache and both stacks unchanged; retries reconstruct imported edges
again. Erasing the cache cannot erase committed lineage.

## Executed boundary and remaining work

`peer_restoration_relation_crosses_the_actual_import_boundary` now runs as a
normal desired assertion, not ignored. Two real candidate managers restore
parentheses repeatedly; the first manager then restores `(B)` and `(XB)` and
redos repeatedly. It covers Text/plain List, both peer orders, a non-BMP cell,
full/incremental binary updates, full snapshots, JSON and batch import. Duplicate
delivery and a fresh snapshot-imported document/manager are included. Restored
edges come from the exported changes; no receipt fixture or test TLS is supplied.
The original public delivery/mirrored assertions still use their original
editing schedules and expected values; candidate peers now explicitly enable
this metadata protocol before producing edits.

`undo::identity_tests` checks malformed/missing/duplicate/noncausal records and
all three import paths, preserving exact values, VV/frontiers, both stacks,
relation and events. It also injects actual post-handler failures after foreign
lineage is imported, compares exported committed bytes across retries, and checks
successful continuation. Separate native controls reject untagged startup,
untagged peers, ordinary inverse generation and missing dependencies. The latter
can recover by full version resync. Host UTF16 checks are not Wasm execution.

Controls omitting only foreign receipt recovery reproduce `()B` versus `(B)`.
Omitting only staged metadata validation makes the malformed-import test fail
its admission assertion. These are software executions, not proofs or an
independent review. Earlier rank-only/output-correction failures remain evidence.

Do not deploy for user data. The experiment closes this selected recovery/import
boundary under the stated producer contract, not general identity semantics.
GC/retention, full-history metadata/replay/staging costs, general topology/ties,
selection/callback reentrancy, complete trigger/group lifecycle and application proof/
browser acceptance remain open. Styles, nested containers, multi-container items
and shallow history are unsupported. No undo-stack persistence, Wasm bindings,
Dark pin, public runtime or deployment is changed. Commands are in the
[benchmark index](../scripts/undo-issue-repros/README.md#opt-in-native-identity-candidate).

The [native trigger scope](undo-trigger-lifecycle.md) now captures actual child
membership and passes the original full delimiter/interior continuation, but
still fails the consumed hidden-creator oracle. Restoration edges do not by
themselves recover original contribution activity after an invisible undo.
