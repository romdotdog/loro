# undo repro benchmarks

## primary: h's unfiled structural repro

`unfiled-delivery.ts` preserves the opening repro h sent in the Claude conversation,
not an issue-tracker report. This is the primary acceptance boundary: identical
causal operations and pre-undo source/VV must not produce different undo results
because imports were batched differently. The ordinary native List witness still
gives sax/asx. Each result converges after its own undo operations are synced;
this is delivery-dependent undo, not demonstrated CRDT divergence.

`crates/loro/tests/undo_delivery_review.rs` owns the native translation and
mirrored placement witnesses. Its two desired tests require List delivery
invariance and Text restoration to distinguish s-before-deleted-a from
s-after-deleted-a (sab/asb), despite identical projected import diffs. Both still
fail. The observation test alone is not acceptance. No-delete baselines suggest
a placement policy; delivery invariance itself does not choose which common
result is correct. Passing the filed reports below cannot replace this boundary,
the selected (B) continuation or preservation of independent peer operations.

```sh
cargo test --locked -p loro --test undo_delivery_review --test identity_duplicate -- --include-ignored --nocapture
```

This currently exits 101: one diagnostic passes, both desired tests fail.
The TypeScript fixture is preserved source, not a fresh JS/Wasm execution claim.

### Opt-in native identity candidate

The Rust feature below selects `UndoManager::new_identity` for the Text/List
fixtures while leaving MovableList as an ordinary reference. The schedules and
desired assertions are unchanged. All three tests pass in this candidate mode:

```sh
cargo test --locked -p loro --features experimental-identity-undo \
  --test undo_delivery_review --test identity_duplicate -- --include-ignored
cargo test --locked -p loro-internal --test identity_manager
```

This is a normal-built mixed-inverse planner with actual committed receipt
metadata, not a cfg(test) substitution. Every candidate peer enables
`doc.enable_identity_history()?` **before initial writes**; then use
`UndoManager::new_identity(&doc)?`. Both are fallible. Existing untagged history
and untagged imports reject, never get retrospectively labeled fresh. Native
editing schedules/assertions above are unchanged; candidate peers now explicitly
produce the metadata.

The actual two-manager continuation now passes and is not ignored:

```sh
cargo test --locked -p loro-internal --test identity_manager \
  peer_restoration_relation_crosses_the_actual_import_boundary
cargo test --locked -p loro-internal --lib undo::identity_tests
```

This covers binary updates/snapshots, JSON and batch imports, sliced changes,
Text/List, both peer orders, non-BMP cells, repeated cycles and cold document
reconstruction. Import checks reject missing/malformed/noncausal metadata in
a staging document before target publication. A valid prefix plus invalid
metadata in one batch leaves the target unchanged. Removing only foreign
receipt recovery reproduces `()B` versus `(B)`; removing staged validation makes
the admission assertion fail. Earlier controls remain evidence.

**Not safe to deploy:** this is a closed trusted-producer protocol, not
authentication or proof of a foreign writer's intent. GC/retention, selection,
general topology, trigger lifecycle and full-history staging costs remain open.
Unsupported styles, nested containers, multi-container items and shallow history
are not silently accepted. There is no undo-stack recovery or retrofit of
ordinary histories. Ordinary defaults, the Dark pin and deployment remain unchanged. See
[native scope](../../context/undo-history-boundaries.md#experimental-native-identity-manager-candidate).

### Native trigger and late-child candidate

With the Rust feature, `capture_trigger()` names the current native item and
`with_derived(&token, || ...)` annotates synchronous work before its first write.
Native commit/query barriers inside the work keep its ownership. This groups
effects; it is not rollback of the closure's partial edits. Tokens are local to
the manager and follow an item through redo, not across stack persistence.

```sh
cargo test --locked -p loro --features experimental-identity-undo --test identity_trigger
cargo test --locked -p loro-internal --test trigger_history
cargo test --locked -p loro-internal --lib derived_membership_tokens
```

The original delimiter/interior continuation and active-token scopes pass, with
Text/List, peer imports, Unicode, repeated cycles, native commit barriers and
preserved primary grouping. Default internal trigger replay **skips five tests**:

```sh
cargo test --locked -p loro-internal --test trigger_history hidden_creator -- --ignored
cargo test --locked -p loro-internal --test trigger_history proposed_wrapping -- --ignored
cargo test --locked -p loro-internal --test trigger_history invisible_peer_retirement_prevents -- --ignored
cargo test --locked -p loro-internal --test trigger_history late_retirement -- --include-ignored
cargo test --locked -p loro --features experimental-identity-undo --test identity_trigger public_retired -- --ignored
```

The first explicitly fails twice at B/empty after retiring an invisible B undo.
This is a real missing inactive-creation-intent boundary, not a membership fix or
new upstream issue count. The second fails at ()!B/(B)! and is a **policy-extension
proposal**: the closing child is created while B is already inactive, unlike the
original accepted schedule. It does not settle wrapping/tie policy. Native no-op
retirement is unchanged; no virtual opposite is silently added. First-span-only
selection and disabled derived-event routing controls fail their source/count
assertions. [Native contract](../../context/undo-trigger-lifecycle.md).

The peer-retirement desired test explicitly fails all 48 continuations: two real
peers, Text/List, both peer orders, ASCII/non-BMP and six delivery/reconstruction
paths. The public wrapper has one corresponding ignored desired failure. Default
byte-equality controls pass because B's invisible retirement changes neither
full update/snapshot bytes nor JSON, VV/frontiers or incremental updates. Those
controls document the missing information; they do not close the desired oracle.
Creation activity must cross the producer boundary, not live only in a manager
cache. Current change-message metadata has no new change to carry this event.

The added late-retirement replay is a **proposed full-visibility** gate, not an
adopted policy or completed repair. A first restores B, then receives B's hidden
retirement. All 48 desired branches still retain B on both peers and a cold
reader; their active/no-retirement controls pass. Six delivery modes include
duplicate delivery and a cold producer rebuilt from an actual post-retirement
snapshot. The fixture does not require unchanged VV/bytes, so a future activity
implementation can use it. Future-inverse planning alone cannot satisfy that
stronger gate; undo-stack persistence is not tested.

## filed upstream reports

These `.cjs` files preserve the JavaScript code blocks posted by romdotdog in
loro-dev/loro issues, retrieved 2026-10-25. `1234-comment.cjs` is the additional
round-trip repro in the issue comment. No linking rewrites or weakened expected
results. The published programs often log the wrong result rather than asserting
it: **exit zero is not a passing correctness benchmark**. This directory does not
install npm packages or claim these JS programs were executed here.

The linked Rust tests translate the schedules into the actual current native
fork. They are not a test of the published JS/Wasm package or a performance suite.
`crates/loro/tests/undo_issue_benchmarks.rs` uses the posted snapshot/incremental
import modes, peer IDs, commit and manager boundaries. JS console observations
become desired-outcome assertions. #1229 case 2 translates its prose schedule;
there is no second JS fixture for it. #1233 retains the two 300ms waits and checks
the timing window; a missed window is inconclusive and must be retried.

Initial baseline: native `fix/undo-intent`, fd8f82d7 plus the fixture addition.
The table tracks the checked fork: #1231, #1232, #1233 and both #1234 cases are repaired natively,
without changing their expected outcomes or the preserved JS fixtures.

| report | native benchmark | current result |
| --- | --- | --- |
| [#1227](https://github.com/loro-dev/loro/issues/1227) | internal `undo::clock_tests::a_backward_recorded_clock_does_not_merge_separate_items` | passes; simulates future recorded time rather than replacing Date.now |
| [#1228](https://github.com/loro-dev/loro/issues/1228) | `issue_1228_manual_group_after_dependent_map_import` | passes |
| [#1229](https://github.com/loro-dev/loro/issues/1229) | `issue_1229_undo_preserves_peer_color`, `issue_1229_redo_preserves_peer_unmark` | both fail: blue lost; red returns |
| [#1231](https://github.com/loro-dev/loro/issues/1231) | `issue_1231_peer_change_inside_manual_group` | passes; the cleared manual group restarts at the new peer's counter |
| [#1232](https://github.com/loro-dev/loro/issues/1232) | `issue_1232_redo_obeys_lowered_undo_limit` | passes; redo-created undo entries now obey the undo bound |
| [#1233](https://github.com/loro-dev/loro/issues/1233) | `issue_1233_post_import_item_resets_merge_timer` | passes; a new item forced by import starts a new clock |
| [#1234](https://github.com/loro-dev/loro/issues/1234) | `issue_1234_delete_revived_original` | passes; new references distinguish a revived original from its older alias |
| [#1234 comment](https://github.com/loro-dev/loro/issues/1234#issuecomment-6389281368) | `issue_1234_comment_roundtrip_uses_original_anchor` | passes; old/new anchors use their observed restoration branch |

Replay from repository root:

```sh
cargo test --locked -p loro-internal --lib undo::clock_tests
cargo test --locked -p loro --test undo_peer_group
cargo test --locked -p loro --test movable_list_undo_aliases
cargo test --locked -p loro --test undo_issue_benchmarks -- --include-ignored --nocapture
```

The full benchmark command still exits 101: six pass, two #1229 desired-outcome failures.
Known failures have explicit `#[ignore]` labels to avoid making the ordinary
suite unusable; **a run without `--include-ignored` does not establish benchmark
closure**. Remove each ignore when its original expected outcome passes. Do not
replace assertions with the buggy result. Keep the selected `(B)` continuation,
remote-deletion, failure/rollback and one-item/no-op tests alongside this baseline;
these reports do not replace those contracts. The peer-change repair resets an
already-open group after clearing old items; it does not group across peer IDs.
The MovableList repair keeps causal restoration branches with real native birth
IDs; it does not infer identity from equal spelling or prefer whichever copy is
live. Removing observation gating restores both original #1234 failures;
overwriting older branches and globally re-resolving planned physical IDs fail
separate public controls. Native context owns scope and retention limits. No
experimental activity/VV change or Dark adoption is supplied by this repair.

Also retain the passing native `undo_restoration_topology` suite. After undoing
Ba to empty, redoing a, and receiving a remote R after that restored a, redo of B
currently gives BaR. The rank-counting placement experiment instead gives aBR;
this is a candidate-algorithm failure, not a new upstream defect. Its explicit
desired replay is:

```sh
cargo test --locked -p loro --test undo_restoration_topology
cargo test --locked -p loro-internal --lib rank_only_plan_preserves_restored_parent_order_with_fresh_child -- --ignored
```

The first passes; the second fails. Do not promote the earlier bounded historical
order/lineage successes into an applied plan without preserving this behavior.

## Metadata callbacks that unwind

```sh
cargo test --locked -p loro --test undo_callback_panic
cargo test --locked -p loro --test undo_callback_panic --features experimental-identity-undo
```

Actual committed local/inverse work keeps its history when `on_push` or `on_pop`
panics: bookkeeping finishes, then the first panic is resumed. Failed push uses
default metadata; invisible no-op retirement still creates no opposite or ops.
Inverse pre-commit, root/event and local-update subscriber panics now finish
publication, actual receipt retention and the opposite item before unwinding.
The actual emitted update is admitted twice by another document; all five
publication/metadata sites may panic without replacing the first payload.
The feature command executes both ordinary and candidate managers. This is Rust
unwind handling, not general callback reentrancy, internal engine panic recovery
or Wasm/browser acceptance. See [the contract](../../context/undo-atomicity.md#metadata-callbacks-after-committed-application).
