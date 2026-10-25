# upstream undo repro benchmarks

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

Baseline: native `fix/undo-intent`, fd8f82d7 plus this test-only addition.

| report | native benchmark | current result |
| --- | --- | --- |
| [#1227](https://github.com/loro-dev/loro/issues/1227) | internal `undo::clock_tests::a_backward_recorded_clock_does_not_merge_separate_items` | passes; simulates future recorded time rather than replacing Date.now |
| [#1228](https://github.com/loro-dev/loro/issues/1228) | `issue_1228_manual_group_after_dependent_map_import` | passes |
| [#1229](https://github.com/loro-dev/loro/issues/1229) | `issue_1229_undo_preserves_peer_color`, `issue_1229_redo_preserves_peer_unmark` | both fail: blue lost; red returns |
| [#1231](https://github.com/loro-dev/loro/issues/1231) | `issue_1231_peer_change_inside_manual_group` | fails: xa instead of x after peer change |
| [#1232](https://github.com/loro-dev/loro/issues/1232) | `issue_1232_redo_obeys_lowered_undo_limit` | fails: canUndo still true |
| [#1233](https://github.com/loro-dev/loro/issues/1233) | `issue_1233_post_import_item_resets_merge_timer` | fails: b instead of ab, timing window satisfied |
| [#1234](https://github.com/loro-dev/loro/issues/1234) | `issue_1234_delete_revived_original` | fails: undo returns false |
| [#1234 comment](https://github.com/loro-dev/loro/issues/1234#issuecomment-6389281368) | `issue_1234_comment_roundtrip_uses_original_anchor` | fails: b,a,b instead of b,b,a |

Replay from repository root:

```sh
cargo test --locked -p loro-internal --lib undo::clock_tests
cargo test --locked -p loro --test undo_issue_benchmarks -- --include-ignored --nocapture
```

The second command currently exits 101: one pass, seven desired-outcome failures.
Known failures have explicit `#[ignore]` labels to avoid making the ordinary
suite unusable; **a run without `--include-ignored` does not establish benchmark
closure**. Remove each ignore when its original expected outcome passes. Do not
replace assertions with the buggy result. Keep the selected `(B)` continuation,
remote-deletion, failure/rollback and one-item/no-op tests alongside this baseline;
these reports do not replace those contracts. Peer-change semantics in #1231
remain the reported expectation, not a newly adopted public API promise.
