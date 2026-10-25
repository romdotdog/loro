# undo repro benchmarks

## primary: h's unfiled structural repro

`unfiled-delivery.ts` preserves the opening repro h sent in the Claude conversation,
not an issue-tracker report. This is the primary acceptance boundary: identical
causal operations and pre-undo source/VV must not produce different undo results
because imports were batched differently. The current native List witness still
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
cargo test --locked -p loro --test undo_delivery_review -- --include-ignored --nocapture
```

This currently exits 101: one diagnostic passes, both desired tests fail.
The TypeScript fixture is preserved source, not a fresh JS/Wasm execution claim.

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
The table tracks the checked fork: #1232 and #1233 are now repaired natively,
without changing their expected outcomes or the preserved JS fixtures.

| report | native benchmark | current result |
| --- | --- | --- |
| [#1227](https://github.com/loro-dev/loro/issues/1227) | internal `undo::clock_tests::a_backward_recorded_clock_does_not_merge_separate_items` | passes; simulates future recorded time rather than replacing Date.now |
| [#1228](https://github.com/loro-dev/loro/issues/1228) | `issue_1228_manual_group_after_dependent_map_import` | passes |
| [#1229](https://github.com/loro-dev/loro/issues/1229) | `issue_1229_undo_preserves_peer_color`, `issue_1229_redo_preserves_peer_unmark` | both fail: blue lost; red returns |
| [#1231](https://github.com/loro-dev/loro/issues/1231) | `issue_1231_peer_change_inside_manual_group` | fails: xa instead of x after peer change |
| [#1232](https://github.com/loro-dev/loro/issues/1232) | `issue_1232_redo_obeys_lowered_undo_limit` | passes; redo-created undo entries now obey the undo bound |
| [#1233](https://github.com/loro-dev/loro/issues/1233) | `issue_1233_post_import_item_resets_merge_timer` | passes; a new item forced by import starts a new clock |
| [#1234](https://github.com/loro-dev/loro/issues/1234) | `issue_1234_delete_revived_original` | fails: undo returns false |
| [#1234 comment](https://github.com/loro-dev/loro/issues/1234#issuecomment-6389281368) | `issue_1234_comment_roundtrip_uses_original_anchor` | fails: b,a,b instead of b,b,a |

Replay from repository root:

```sh
cargo test --locked -p loro-internal --lib undo::clock_tests
cargo test --locked -p loro --test undo_issue_benchmarks -- --include-ignored --nocapture
```

The second command currently exits 101: three pass, five desired-outcome failures.
Known failures have explicit `#[ignore]` labels to avoid making the ordinary
suite unusable; **a run without `--include-ignored` does not establish benchmark
closure**. Remove each ignore when its original expected outcome passes. Do not
replace assertions with the buggy result. Keep the selected `(B)` continuation,
remote-deletion, failure/rollback and one-item/no-op tests alongside this baseline;
these reports do not replace those contracts. Peer-change semantics in #1231
remain the reported expectation, not a newly adopted public API promise.

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
