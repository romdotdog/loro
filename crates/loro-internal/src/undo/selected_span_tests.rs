//! Explicit native range selection, before adopting a manager/trigger API.
use super::placement_probe::candidate_order as causal_gap_candidate_order;
use super::*;
use crate::{cursor::PosType, HandlerTrait};

fn checkpoint(doc: &LoroDoc, start: Counter) -> IdSpan {
    doc.commit_then_renew();
    IdSpan::new(doc.peer_id(), start, get_counter_end(doc, doc.peer_id()))
}

fn inverse(doc: &LoroDoc, spans: &[IdSpan]) -> IdSpan {
    inverse_with_receipts(doc, spans).0
}

fn inverse_with_receipts(
    doc: &LoroDoc,
    spans: &[IdSpan],
) -> (IdSpan, Vec<super::text::AppliedRestoration>) {
    let start = get_counter_end(doc, doc.peer_id());
    let commit = doc
        .undo_internal_ranges_with(
            spans,
            &mut Default::default(),
            None,
            &mut |_| {},
            &mut None,
            &mut Default::default(),
        )
        .unwrap();
    let receipts = commit.text_restorations.clone();
    drop(commit);
    (checkpoint(doc, start), receipts)
}

fn fixture() -> (LoroDoc, IdSpan, IdSpan, IdSpan) {
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(1).unwrap();
    let text = doc.get_text("text");
    text.insert(0, "(", PosType::Unicode).unwrap();
    let trigger = checkpoint(&doc, 0);
    let start = get_counter_end(&doc, 1);
    text.insert(1, "B", PosType::Unicode).unwrap();
    let intervening = checkpoint(&doc, start);
    let start = get_counter_end(&doc, 1);
    text.insert(2, ")", PosType::Unicode).unwrap();
    let child = checkpoint(&doc, start);
    (doc, trigger, intervening, child)
}

#[test]
fn selecting_trigger_and_child_leaves_the_intervening_command() {
    let (doc, trigger, _, child) = fixture();
    let mut chosen = vec![trigger, child];
    for _ in 0..3 {
        let opposite = inverse(&doc, &chosen);
        assert_eq!(doc.get_text("text").to_string(), "B");
        chosen = vec![inverse(&doc, &[opposite])];
        assert_eq!(doc.get_text("text").to_string(), "(B)");
    }
}

#[test]
#[ignore = "selected native inverses still lose the intervening command's interior redo placement"]
fn redo_intervening_command_after_selected_trigger_roundtrip() {
    let (doc, trigger, intervening, child) = fixture();
    let redo_b = inverse(&doc, &[intervening]);
    assert_eq!(doc.get_text("text").to_string(), "()");
    let redo_a = inverse(&doc, &[trigger, child]);
    assert_eq!(doc.get_text("text").to_string(), "");
    inverse(&doc, &[redo_a]);
    assert_eq!(doc.get_text("text").to_string(), "()");
    inverse(&doc, &[redo_b]);
    assert_eq!(doc.get_text("text").to_string(), "(B)");
}

/// Locate the missing information boundary without changing the desired
/// continuation above. Historical creation IDs survive; current cursor
/// resolution does not associate the restored delimiters with those IDs.
#[test]
fn historical_interior_ids_survive_import_but_not_restoration_cursor_placement() {
    use crate::{cursor::Side, encoding::ExportMode};

    for peer in [1, 2] {
        for middle in ["B", "🦀"] {
            let doc = LoroDoc::new_auto_commit();
            doc.set_peer_id(peer).unwrap();
            let text = doc.get_text("text");
            text.insert(0, "(", PosType::Unicode).unwrap();
            let trigger = checkpoint(&doc, 0);
            let start = get_counter_end(&doc, peer);
            text.insert(1, middle, PosType::Unicode).unwrap();
            let command = checkpoint(&doc, start);
            let start = get_counter_end(&doc, peer);
            text.insert(2, ")", PosType::Unicode).unwrap();
            let child = checkpoint(&doc, start);
            let historical_frontiers = doc.state_frontiers();
            let originals: Vec<_> = (0..3)
                .map(|i| text.get_cursor_internal(i, Side::Middle, false).unwrap())
                .collect();

            let redo_middle = inverse(&doc, &[command]);
            let redo_delimiters = inverse(&doc, &[trigger, child]);
            inverse(&doc, &[redo_delimiters]);
            assert_eq!(text.to_string(), "()");
            let restored: Vec<_> = (0..2)
                .map(|i| text.get_cursor_internal(i, Side::Middle, false).unwrap())
                .collect();
            assert_ne!(restored[0].id, originals[0].id);
            assert_ne!(restored[1].id, originals[2].id);

            // A fresh full import has the same native cursor result; this is
            // not just stale state in the process that performed the inverses.
            let imported = LoroDoc::new_auto_commit();
            imported
                .import(&doc.export(ExportMode::all_updates()).unwrap())
                .unwrap();
            for replica in [&doc, &imported] {
                let vv = replica.oplog_vv();
                let frontiers = replica.state_frontiers();
                let old_middle = replica.try_get_cursor_pos(&originals[1]).unwrap();
                let current_left = replica.try_get_cursor_pos(&restored[0]).unwrap();
                let current_right = replica.try_get_cursor_pos(&restored[1]).unwrap();
                eprintln!(
                    "peer={peer} middle={middle} old={:?} restored={:?} old_middle={old_middle:?} current_left={current_left:?} current_right={current_right:?}",
                    originals.iter().map(|c| c.id).collect::<Vec<_>>(),
                    restored.iter().map(|c| c.id).collect::<Vec<_>>(),
                );
                assert_eq!(old_middle.current.pos, 2);
                assert_ne!(old_middle.current.pos, current_right.current.pos);
                assert_eq!(current_left.current.pos, 0);
                assert_eq!(current_right.current.pos, 1);
                assert_eq!(replica.oplog_vv(), vv);
                assert_eq!(replica.state_frontiers(), frontiers);
            }

            // The original creation/order evidence is still queryable from
            // imported history. Do not confuse its availability with a
            // restoration-lineage algorithm: the test knows the chosen ranges.
            imported.checkout(&historical_frontiers).unwrap();
            let historical = imported.get_text("text");
            assert_eq!(historical.to_string(), format!("({middle})"));
            for (i, original) in originals.iter().enumerate() {
                assert_eq!(
                    historical
                        .get_cursor_internal(i, Side::Middle, false)
                        .unwrap()
                        .id,
                    original.id
                );
            }

            inverse(&doc, &[redo_middle]);
            assert_eq!(text.to_string(), format!("{middle}()"));
            // This is the observed failure, not the desired (middle) oracle.
        }
    }
}

#[test]
fn calculated_restoration_origins_survive_selected_inverse_composition() {
    use super::text::with_calculated_restorations;
    use crate::cursor::Side;

    // Equal spelling makes matching strings insufficient to identify any cell.
    // Unicode checks that event units do not become distinct logical origins.
    for content in ["aaa", "🦀🦀🦀"] {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let text = doc.get_text("text");
        let mut spans = Vec::new();
        for ch in content.chars() {
            let start = get_counter_end(&doc, 1);
            text.insert(spans.len(), &ch.to_string(), PosType::Unicode)
                .unwrap();
            spans.push(checkpoint(&doc, start));
        }
        let ids: Vec<_> = (0..3)
            .map(|i| text.get_cursor_internal(i, Side::Middle, false).unwrap().id)
            .collect();
        let redo_middle = inverse(&doc, &[spans[1]]);
        let redo_outer = inverse(&doc, &[spans[0], spans[2]]);
        let (remove_outer, origins) = with_calculated_restorations(|| inverse(&doc, &[redo_outer]));
        let width = if cfg!(feature = "wasm") {
            content.chars().next().unwrap().len_utf16()
        } else {
            1
        };
        assert_eq!(
            origins,
            vec![(text.id(), 0, ids[0]), (text.id(), width, ids[2])]
        );
        let restored_ids: Vec<_> = (0..2)
            .map(|i| text.get_cursor_internal(i, Side::Middle, false).unwrap().id)
            .collect();
        assert_ne!(restored_ids[0], ids[0]);
        assert_ne!(restored_ids[1], ids[2]);
        let (_, origins) = with_calculated_restorations(|| inverse(&doc, &[redo_middle]));
        assert_eq!(origins, vec![(text.id(), 0, ids[1])]);
        // Its identity is now available to a future native placement plan;
        // the unchanged positional application still puts it before the pair.
        assert_eq!(text.to_string(), content);
        let restore_outer_next = inverse(&doc, &[remove_outer]);
        let (_, origins) = with_calculated_restorations(|| inverse(&doc, &[restore_outer_next]));
        assert_eq!(
            origins,
            vec![
                (text.id(), width, restored_ids[0]),
                (text.id(), 2 * width, restored_ids[1]),
            ]
        );
        // Origins are the physical IDs at this inverse's output frontier.
        // They must not be mislabeled as a transitive logical-ID map.
    }
}

#[test]
fn calculated_restoration_origins_cross_unselected_bridge_and_final_transform() {
    use super::text::with_calculated_restorations;
    use crate::cursor::Side;

    for cell in ["a", "🦀"] {
        for delete_middle_later in [false, true] {
            let doc = LoroDoc::new_auto_commit();
            doc.set_peer_id(1).unwrap();
            let text = doc.get_text("text");
            text.insert(0, &cell.repeat(3), PosType::Unicode).unwrap();
            checkpoint(&doc, 0);
            let ids: Vec<_> = (0..3)
                .map(|i| text.get_cursor_internal(i, Side::Middle, false).unwrap().id)
                .collect();
            let start = get_counter_end(&doc, 1);
            text.delete(0, 1, PosType::Unicode).unwrap();
            let left = checkpoint(&doc, start);
            let start = get_counter_end(&doc, 1);
            text.insert(1, "R", PosType::Unicode).unwrap();
            checkpoint(&doc, start);
            let start = get_counter_end(&doc, 1);
            text.delete(2, 1, PosType::Unicode).unwrap();
            let right = checkpoint(&doc, start);
            if delete_middle_later {
                text.delete(0, 1, PosType::Unicode).unwrap();
                doc.commit_then_renew();
            }
            let (_, origins) = with_calculated_restorations(|| inverse(&doc, &[left, right]));
            let width = if cfg!(feature = "wasm") {
                cell.chars().next().unwrap().len_utf16()
            } else {
                1
            };
            let middle = if delete_middle_later { "" } else { cell };
            assert_eq!(text.to_string(), format!("{cell}{middle}R{cell}"));
            let right_pos = width * if delete_middle_later { 1 } else { 2 } + 1;
            assert_eq!(
                origins,
                vec![(text.id(), 0, ids[0]), (text.id(), right_pos, ids[2])]
            );
        }
    }
}

#[test]
fn calculated_restoration_origins_remain_aligned_after_foreign_delete_filtering() {
    use super::text::with_calculated_restorations;
    use crate::{cursor::Side, encoding::ExportMode};

    for cell in ["a", "🦀"] {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let text = doc.get_text("text");
        text.insert(0, &cell.repeat(3), PosType::Unicode).unwrap();
        doc.commit_then_renew();
        let ids: Vec<_> = (0..3)
            .map(|i| text.get_cursor_internal(i, Side::Middle, false).unwrap().id)
            .collect();
        let remote = doc.fork();
        remote.set_peer_id(2).unwrap();
        let undo = UndoManager::new(&doc);
        undo.group_start().unwrap();
        text.delete(0, 3, PosType::Unicode).unwrap();
        text.insert(0, "x", PosType::Unicode).unwrap();
        doc.commit_then_renew();
        remote
            .get_text("text")
            .delete(1, 1, PosType::Unicode)
            .unwrap();
        remote.get_map("meta").insert("dependency", true).unwrap();
        remote.commit_then_renew();
        doc.import(&remote.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        text.delete(0, 1, PosType::Unicode).unwrap();
        text.insert(0, "y", PosType::Unicode).unwrap();
        doc.commit_then_renew();
        undo.group_end();
        let (applied, origins) = with_calculated_restorations(|| undo.undo().unwrap());
        assert!(applied);
        assert_eq!(text.to_string(), cell.repeat(2));
        let width = if cfg!(feature = "wasm") {
            cell.chars().next().unwrap().len_utf16()
        } else {
            1
        };
        assert_eq!(
            origins,
            vec![(text.id(), 0, ids[0]), (text.id(), width, ids[2])]
        );
    }
}

#[test]
fn applied_restoration_receipts_name_real_emitted_ids_across_cycles_and_import() {
    use crate::{cursor::Side, encoding::ExportMode};

    for cell in ["a", "🦀"] {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let text = doc.get_text("text");
        text.insert(0, &cell.repeat(3), PosType::Unicode).unwrap();
        doc.commit_then_renew();
        let original_left = text.get_cursor_internal(0, Side::Middle, false).unwrap().id;
        let original_right = text.get_cursor_internal(2, Side::Middle, false).unwrap().id;
        let start = get_counter_end(&doc, 1);
        text.delete(0, 1, PosType::Unicode).unwrap();
        let left = checkpoint(&doc, start);
        let start = get_counter_end(&doc, 1);
        text.insert(1, "R", PosType::Unicode).unwrap();
        checkpoint(&doc, start);
        let start = get_counter_end(&doc, 1);
        text.delete(2, 1, PosType::Unicode).unwrap();
        let right = checkpoint(&doc, start);
        let (remove_restored, first) = inverse_with_receipts(&doc, &[left, right]);
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].origin, original_left);
        assert_eq!(first[1].origin, original_right);
        for (receipt, index) in first.iter().zip([0, 3]) {
            assert_eq!(receipt.source_container, text.id());
            assert_eq!(receipt.target_container, text.id());
            assert_eq!(
                Some(receipt.restored),
                text.get_cursor_internal(index, Side::Middle, false)
                    .unwrap()
                    .id
            );
            assert_ne!(Some(receipt.restored), receipt.origin);
        }
        let restore_again = inverse(&doc, &[remove_restored]);
        let (_, second) = inverse_with_receipts(&doc, &[restore_again]);
        assert_eq!(second.len(), 2);
        assert_eq!(second[0].origin, Some(first[0].restored));
        assert_eq!(second[1].origin, Some(first[1].restored));
        let imported = LoroDoc::new_auto_commit();
        imported
            .import(&doc.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        for (receipt, index) in second.iter().zip([0, 3]) {
            assert_eq!(
                Some(receipt.restored),
                imported
                    .get_text("text")
                    .get_cursor_internal(index, Side::Middle, false)
                    .unwrap()
                    .id
            );
        }
        // The emitted IDs survive import. The receipt edges are private
        // application evidence and are NOT serialized by this change.
    }
}

/// A receipt cannot be recovered merely by recognizing an insert's spelling,
/// dependencies or physical IDs. These are alternative histories of ONE peer,
/// deliberately never merged together, not permission to reuse peer IDs in a
/// distributed application. No persistent restoration encoding is added here.
#[test]
fn restoration_and_fresh_typing_can_export_identical_native_history() {
    use crate::{cursor::Side, encoding::ExportMode};

    for peer in [1, 2] {
        for cell in ["a", "🦀"] {
            let restored = LoroDoc::new_auto_commit();
            restored.set_peer_id(peer).unwrap();
            restored.set_record_timestamp(false);
            restored.set_change_merge_interval(0);
            let text = restored.get_text("text");
            text.insert(0, &cell.repeat(2), PosType::Unicode).unwrap();
            checkpoint(&restored, 0);
            let original = text.get_cursor_internal(0, Side::Middle, false).unwrap().id;
            let start = get_counter_end(&restored, peer);
            text.delete(0, 1, PosType::Unicode).unwrap();
            let deletion = checkpoint(&restored, start);

            // Start the alternative execution from exactly the same prefix.
            let typed = LoroDoc::new_auto_commit();
            typed.set_record_timestamp(false);
            typed.set_change_merge_interval(0);
            typed
                .import(&restored.export(ExportMode::all_updates()).unwrap())
                .unwrap();
            typed.set_peer_id(peer).unwrap();

            let (_, receipts) = inverse_with_receipts(&restored, &[deletion]);
            assert_eq!(receipts.len(), 1);
            assert_eq!(receipts[0].origin, original);
            let typed_text = typed.get_text("text");
            typed_text.insert(0, cell, PosType::Unicode).unwrap();
            typed.commit_then_renew();
            assert_eq!(text.to_string(), typed_text.to_string());
            assert_eq!(restored.oplog_vv(), typed.oplog_vv());
            assert_eq!(restored.state_frontiers(), typed.state_frontiers());
            assert_eq!(
                Some(receipts[0].restored),
                typed_text
                    .get_cursor_internal(0, Side::Middle, false)
                    .unwrap()
                    .id
            );

            // Compare the entire current native update encoding, not only
            // visible state or a projection stripped of metadata.
            let undo_bytes = restored.export(ExportMode::all_updates()).unwrap();
            let typed_bytes = typed.export(ExportMode::all_updates()).unwrap();
            assert_eq!(undo_bytes, typed_bytes);
            for bytes in [&undo_bytes, &typed_bytes] {
                let recovered = LoroDoc::new_auto_commit();
                recovered.import(bytes).unwrap();
                assert_eq!(recovered.get_text("text").to_string(), cell.repeat(2));
                assert_eq!(recovered.oplog_vv(), restored.oplog_vv());
            }
        }
    }
}

#[test]
fn applied_restoration_receipts_are_discarded_on_failure_then_retry() {
    use crate::{cursor::Side, loro::FAIL_APPLY_DIFF_AFTER_HANDLER};
    use std::sync::atomic::{AtomicUsize, Ordering};

    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(1).unwrap();
    let text = doc.get_text("text");
    text.insert(0, "aaa", PosType::Unicode).unwrap();
    doc.commit_then_renew();
    let start = get_counter_end(&doc, 1);
    text.delete(0, 3, PosType::Unicode).unwrap();
    let deletion = checkpoint(&doc, start);
    let before = doc.oplog_vv();
    let frontiers = doc.state_frontiers();
    let events = Arc::new(AtomicUsize::new(0));
    let seen = events.clone();
    let _sub = doc.subscribe_root(Arc::new(move |_| {
        seen.fetch_add(1, Ordering::Relaxed);
    }));
    for _ in 0..2 {
        FAIL_APPLY_DIFF_AFTER_HANDLER.with(|flag| flag.set(true));
        let result = doc.undo_internal_ranges_with(
            &[deletion],
            &mut Default::default(),
            None,
            &mut |_| {},
            &mut None,
            &mut Default::default(),
        );
        assert!(
            result.is_err(),
            "no successful guard/receipts may escape failure"
        );
        assert_eq!(text.to_string(), "");
        assert_eq!(doc.oplog_vv(), before);
        assert_eq!(doc.state_frontiers(), frontiers);
        assert_eq!(events.load(Ordering::Relaxed), 0);
    }
    let (created, receipts) = inverse_with_receipts(&doc, &[deletion]);
    assert_eq!(receipts.len(), 3);
    for (i, receipt) in receipts.iter().enumerate() {
        assert_eq!(receipt.origin, Some(loro_common::ID::new(1, i as i32)));
        assert_eq!(receipt.restored.counter, created.counter.min() + i as i32);
        assert_eq!(
            Some(receipt.restored),
            text.get_cursor_internal(i, Side::Middle, false).unwrap().id
        );
    }
}

#[test]
fn applied_restoration_receipts_exclude_filtered_foreign_deletions_and_noops() {
    use crate::encoding::ExportMode;

    for all in [false, true] {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let text = doc.get_text("text");
        text.insert(0, "aaa", PosType::Unicode).unwrap();
        doc.commit_then_renew();
        let remote = doc.fork();
        remote.set_peer_id(2).unwrap();
        let start = get_counter_end(&doc, 1);
        text.delete(0, 3, PosType::Unicode).unwrap();
        let deletion = checkpoint(&doc, start);
        remote
            .get_text("text")
            .delete(
                if all { 0 } else { 1 },
                if all { 3 } else { 1 },
                PosType::Unicode,
            )
            .unwrap();
        remote.commit_then_renew();
        doc.import(&remote.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        let (_, receipts) = inverse_with_receipts(&doc, &[deletion]);
        let origins: Vec<_> = receipts.iter().map(|r| r.origin.unwrap().counter).collect();
        assert_eq!(origins, if all { vec![] } else { vec![0, 2] });
        assert_eq!(text.to_string(), if all { "" } else { "aa" });
    }
}

#[test]
fn applied_restoration_receipts_follow_actual_ids_around_style_operations() {
    use crate::cursor::Side;

    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(1).unwrap();
    let text = doc.get_text("text");
    text.insert(0, "aaa", PosType::Unicode).unwrap();
    text.mark(0, 1, "bold", true.into(), PosType::Unicode)
        .unwrap();
    text.mark(2, 3, "italic", true.into(), PosType::Unicode)
        .unwrap();
    doc.commit_then_renew();
    let expected = text.get_richtext_value();
    let old_ids: Vec<_> = (0..3)
        .map(|i| text.get_cursor_internal(i, Side::Middle, false).unwrap().id)
        .collect();
    let start = get_counter_end(&doc, 1);
    text.delete(0, 3, PosType::Unicode).unwrap();
    let deletion = checkpoint(&doc, start);
    let (_, receipts) = inverse_with_receipts(&doc, &[deletion]);
    assert_eq!(text.get_richtext_value(), expected);
    assert_eq!(receipts.len(), 3);
    for (i, receipt) in receipts.iter().enumerate() {
        assert_eq!(receipt.origin, old_ids[i]);
        assert_eq!(
            Some(receipt.restored),
            text.get_cursor_internal(i, Side::Middle, false).unwrap().id
        );
    }
}

#[test]
fn applied_restoration_receipts_do_not_claim_skipped_unreachable_text() {
    use crate::handler::TextHandler;

    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(1).unwrap();
    let parent = doc.get_map("parent");
    let text = parent
        .insert_container("text", TextHandler::new_detached())
        .unwrap();
    text.insert(0, "aaa", PosType::Unicode).unwrap();
    doc.commit_then_renew();
    let start = get_counter_end(&doc, 1);
    text.delete(0, 3, PosType::Unicode).unwrap();
    let deletion = checkpoint(&doc, start);
    parent.delete("text").unwrap();
    doc.commit_then_renew();
    let before = doc.oplog_vv();
    let (_, receipts) = inverse_with_receipts(&doc, &[deletion]);
    assert!(receipts.is_empty());
    assert_eq!(doc.oplog_vv(), before);
    assert!(parent.get("text").is_none());
}

#[test]
fn applied_restoration_receipts_reject_existing_id_and_roll_back_real_insertion() {
    use super::text::POINT_RECEIPT_AT_RETAINED_TEXT;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(1).unwrap();
    let text = doc.get_text("text");
    text.insert(0, "aaa", PosType::Unicode).unwrap();
    doc.commit_then_renew();
    let start = get_counter_end(&doc, 1);
    text.delete(1, 1, PosType::Unicode).unwrap();
    let deletion = checkpoint(&doc, start);
    let before = doc.oplog_vv();
    let frontiers = doc.state_frontiers();
    let events = Arc::new(AtomicUsize::new(0));
    let seen = events.clone();
    let _sub = doc.subscribe_root(Arc::new(move |_| {
        seen.fetch_add(1, Ordering::Relaxed);
    }));
    for _ in 0..2 {
        // All cells spell "a". Deliberately point the receipt at an older
        // retained cell instead of the fresh insertion, after real application.
        POINT_RECEIPT_AT_RETAINED_TEXT.with(|flag| flag.set(true));
        let error = doc
            .undo_internal_ranges_with(
                &[deletion],
                &mut Default::default(),
                None,
                &mut |_| {},
                &mut None,
                &mut Default::default(),
            )
            .unwrap_err();
        assert!(matches!(error, LoroError::TransactionError(ref message)
            if message.as_ref() == "restoration receipt names an existing element"));
        assert_eq!(text.to_string(), "aa");
        assert_eq!(doc.oplog_vv(), before);
        assert_eq!(doc.state_frontiers(), frontiers);
        assert_eq!(events.load(Ordering::Relaxed), 0);
    }
    let (_, receipts) = inverse_with_receipts(&doc, &[deletion]);
    assert_eq!(text.to_string(), "aaa");
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].origin, Some(loro_common::ID::new(1, 1)));
}

#[test]
fn selected_range_validation_leaves_document_and_events_unchanged() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let (doc, trigger, _, child) = fixture();
    let vv = doc.oplog_vv();
    let frontiers = doc.state_frontiers();
    let seen = Arc::new(AtomicUsize::new(0));
    let events = seen.clone();
    let _subscription = doc.subscribe_root(Arc::new(move |_| {
        events.fetch_add(1, Ordering::Relaxed);
    }));
    for spans in [
        vec![],
        vec![trigger, trigger],
        vec![child, trigger],
        vec![IdSpan::new(1, 1, 1)],
        vec![trigger, IdSpan::new(2, 0, 1)],
        vec![IdSpan::new(1, 0, 100)],
    ] {
        let result = doc.undo_internal_ranges_with(
            &spans,
            &mut Default::default(),
            None,
            &mut |_| {},
            &mut None,
            &mut Default::default(),
        );
        assert!(result.is_err(), "invalid selection {spans:?}");
        assert_eq!(doc.get_text("text").to_string(), "(B)");
        assert_eq!(doc.oplog_vv(), vv);
        assert_eq!(doc.state_frontiers(), frontiers);
        assert_eq!(seen.load(Ordering::Relaxed), 0);
    }
    // The original single-span entry must also return an error without
    // trying to renew auto-commit while its transaction guard is held.
    assert!(doc
        .undo_internal(
            IdSpan::new(1, 0, 100),
            &mut Default::default(),
            None,
            &mut |_| {},
        )
        .is_err());
    assert_eq!(doc.get_text("text").to_string(), "(B)");
    doc.get_text("text")
        .insert(3, "!", PosType::Unicode)
        .unwrap();
    doc.commit_then_renew();
    assert_eq!(doc.get_text("text").to_string(), "(B)!");
}

#[test]
fn selected_inverse_application_failure_is_atomic_and_retryable() {
    use crate::loro::FAIL_APPLY_DIFF_AFTER_HANDLER;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let (doc, trigger, _, child) = fixture();
    let vv = doc.oplog_vv();
    let frontiers = doc.state_frontiers();
    let rebased = AtomicUsize::new(0);
    let seen = Arc::new(AtomicUsize::new(0));
    let events = seen.clone();
    let _subscription = doc.subscribe_root(Arc::new(move |_| {
        events.fetch_add(1, Ordering::Relaxed);
    }));
    for _ in 0..2 {
        FAIL_APPLY_DIFF_AFTER_HANDLER.with(|flag| flag.set(true));
        let result = doc.undo_internal_ranges_with(
            &[trigger, child],
            &mut Default::default(),
            None,
            &mut |_| {
                rebased.fetch_add(1, Ordering::Relaxed);
            },
            &mut None,
            &mut Default::default(),
        );
        assert!(result.is_err());
        assert_eq!(doc.get_text("text").to_string(), "(B)");
        assert_eq!(doc.oplog_vv(), vv);
        assert_eq!(doc.state_frontiers(), frontiers);
        assert_eq!(seen.load(Ordering::Relaxed), 0);
        assert_eq!(rebased.load(Ordering::Relaxed), 0);
    }
    drop(
        doc.undo_internal_ranges_with(
            &[trigger, child],
            &mut Default::default(),
            None,
            &mut |_| {
                rebased.fetch_add(1, Ordering::Relaxed);
            },
            &mut None,
            &mut Default::default(),
        )
        .unwrap(),
    );
    assert_eq!(doc.get_text("text").to_string(), "B");
    assert_eq!(rebased.load(Ordering::Relaxed), 1);
}

/// A placement experiment, NOT an adopted inverse path. It consumes actual
/// applied receipts and native full-history order. No fixture-authored alias,
/// guessed creation ID, original character matching or retained-neighbor order.
#[test]
fn native_historical_order_and_applied_lineage_locate_restored_interior() {
    use crate::{cursor::Side, encoding::ExportMode};
    for peer in [1, 2] {
        for middle in ["B", "🦀"] {
            let doc = LoroDoc::new_auto_commit();
            doc.set_peer_id(peer).unwrap();
            let text = doc.get_text("text");
            text.insert(0, "(", PosType::Unicode).unwrap();
            let trigger = checkpoint(&doc, 0);
            let start = get_counter_end(&doc, peer);
            text.insert(1, middle, PosType::Unicode).unwrap();
            let command = checkpoint(&doc, start);
            let start = get_counter_end(&doc, peer);
            text.insert(2, ")", PosType::Unicode).unwrap();
            let child = checkpoint(&doc, start);
            let (redo_middle, removed_middle) = inverse_with_receipts(&doc, &[command]);
            assert!(removed_middle.is_empty());
            let redo_pair = inverse(&doc, &[trigger, child]);
            let (mut remove_pair, mut restored_pair) = inverse_with_receipts(&doc, &[redo_pair]);
            assert_eq!(text.to_string(), "()");
            assert_eq!(restored_pair.len(), 2);
            for _ in 0..2 {
                let restore_pair = inverse(&doc, &[remove_pair]);
                let (next_remove, next_edges) = inverse_with_receipts(&doc, &[restore_pair]);
                assert_eq!(text.to_string(), "()");
                assert_eq!(next_edges.len(), 2);
                remove_pair = next_remove;
                restored_pair.extend(next_edges);
            }

            // Capture the actual origin of the next native inverse before it
            // is applied. This does not supply its identity or repair its slot.
            let prefix = doc.export(ExportMode::all_updates()).unwrap();
            let live: Vec<_> = (0..2)
                .map(|i| {
                    text.get_cursor_internal(i, Side::Middle, false)
                        .unwrap()
                        .id
                        .unwrap()
                })
                .collect();
            let order = crate::diff_calc::historical_text_id_order(&doc.oplog().lock(), &text.id());
            let insertions =
                crate::diff_calc::historical_text_insertions(&doc.oplog().lock(), &text.id());
            let candidate_order =
                causal_gap_candidate_order(&order, &insertions, &restored_pair).unwrap();
            let (_, origins) = super::text::with_calculated_restorations(|| {
                inverse(&doc, &[redo_middle]);
            });
            assert_eq!(text.to_string(), format!("{middle}()"));
            // Calculated restoration positions enumerate scalar boundaries,
            // even when event offsets use UTF16 units.
            assert_eq!(origins.len(), 1);
            let target = origins[0].2.unwrap();
            assert!(origins.iter().all(|r| r.2 == Some(target)));
            let rank = |id| order.iter().position(|x| *x == id).unwrap();
            let target_rank = rank(target);
            let candidate_rank = |id| candidate_order.iter().position(|x| *x == id).unwrap();
            let mut candidate_before = 0;
            for live_id in &live {
                let mut root = *live_id;
                for _ in 0..restored_pair.len() {
                    match restored_pair
                        .iter()
                        .find(|edge| edge.restored == root)
                        .and_then(|edge| edge.origin)
                    {
                        Some(parent) => root = parent,
                        None => break,
                    }
                }
                candidate_before += usize::from(candidate_rank(root) < candidate_rank(target));
            }
            assert_eq!(candidate_before, 1);
            let position = |use_lineage: bool| {
                live.iter()
                    .filter(|id| {
                        let mut original = **id;
                        if use_lineage {
                            let mut traversed = 0;
                            while let Some(parent) = restored_pair
                                .iter()
                                .find(|edge| edge.restored == original)
                                .and_then(|edge| edge.origin)
                            {
                                traversed += 1;
                                assert!(traversed <= restored_pair.len(), "lineage cycle");
                                original = parent;
                            }
                        }
                        rank(original) < target_rank
                    })
                    .count()
            };
            assert_eq!(position(true), 1);
            assert_ne!(
                position(false),
                1,
                "physical IDs alone must not discharge lineage"
            );

            let planned = LoroDoc::new_auto_commit();
            planned.import(&prefix).unwrap();
            assert_eq!(
                crate::diff_calc::historical_text_id_order(&planned.oplog().lock(), &text.id()),
                order
            );
            planned.set_peer_id(99).unwrap();
            let before = planned.oplog_vv();
            let planned_text = planned.get_text("text");
            planned_text
                .insert(position(true), middle, PosType::Unicode)
                .unwrap();
            planned.commit_then_renew();
            assert_eq!(planned_text.to_string(), format!("({middle})"));
            assert_ne!(planned.oplog_vv(), before);
            let imported = LoroDoc::new_auto_commit();
            imported
                .import(&planned.export(ExportMode::all_updates()).unwrap())
                .unwrap();
            assert_eq!(imported.get_text("text").to_string(), format!("({middle})"));
        }
    }
}

/// Primary unfiled List schedule: native full-history order keeps the missing
/// landmark even when the manager's composed projection loses it. This only
/// probes a placement ingredient, not a repaired native undo implementation.
#[test]
fn native_historical_list_order_retains_delivery_partition_landmark() {
    use crate::{cursor::Side, encoding::ExportMode};
    for incremental in [false, true] {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let remote = LoroDoc::new_auto_commit();
        remote.set_peer_id(2).unwrap();
        let list = doc.get_list("sequence");
        let other = remote.get_list("sequence");
        let deliver = |from: &LoroDoc, to: &LoroDoc| {
            to.import(&from.export(ExportMode::Snapshot).unwrap())
                .unwrap();
        };
        list.insert(0, "a").unwrap();
        list.insert(1, "b").unwrap();
        doc.commit_then_renew();
        deliver(&doc, &remote);
        list.insert(1, "x").unwrap();
        doc.commit_then_renew();
        other.insert(0, "r").unwrap();
        remote.commit_then_renew();
        deliver(&remote, &doc);
        let original = list.get_cursor(1, Side::Middle).unwrap().id.unwrap();
        let start = get_counter_end(&doc, 1);
        list.delete(1, 1).unwrap();
        let deletion = checkpoint(&doc, start);
        other.delete(0, 1).unwrap();
        remote.commit_then_renew();
        if incremental {
            deliver(&remote, &doc);
        }
        other.delete(1, 1).unwrap();
        remote.commit_then_renew();
        if incremental {
            deliver(&remote, &doc);
        }
        other.insert(0, "s").unwrap();
        remote.commit_then_renew();
        deliver(&remote, &doc);
        let live: Vec<_> = (0..2)
            .map(|i| list.get_cursor(i, Side::Middle).unwrap().id.unwrap())
            .collect();
        let vv = doc.oplog_vv();
        let frontiers = doc.state_frontiers();
        let order = crate::diff_calc::historical_list_id_order(&doc.oplog().lock(), &list.id());
        let rank = |id| order.iter().position(|x| *x == id).unwrap();
        let slot = live.iter().filter(|id| rank(**id) < rank(original)).count();
        assert_eq!(slot, 1, "incremental={incremental}");
        assert_eq!(doc.oplog_vv(), vv);
        assert_eq!(doc.state_frontiers(), frontiers);
        let imported = LoroDoc::new_auto_commit();
        deliver(&doc, &imported);
        assert_eq!(
            crate::diff_calc::historical_list_id_order(&imported.oplog().lock(), &list.id()),
            order
        );
        // Native application is deliberately unchanged. The two native results
        // still differ; having the landmark is not yet consuming it correctly.
        inverse(&doc, &[deletion]);
    }
}

#[test]
fn native_historical_text_order_separates_mirrored_deleted_target_slots() {
    use crate::encoding::ExportMode;
    for peer in [1, 2] {
        for at in [0, 1] {
            let doc = LoroDoc::new_auto_commit();
            doc.set_peer_id(peer).unwrap();
            let remote = LoroDoc::new_auto_commit();
            remote.set_peer_id(3 - peer).unwrap();
            let text = doc.get_text("text");
            text.insert(0, "ab", PosType::Unicode).unwrap();
            doc.commit_then_renew();
            remote
                .import(&doc.export(ExportMode::all_updates()).unwrap())
                .unwrap();
            let start = get_counter_end(&doc, peer);
            text.delete(0, 1, PosType::Unicode).unwrap();
            let deletion = checkpoint(&doc, start);
            remote
                .get_text("text")
                .insert(at, "s", PosType::Unicode)
                .unwrap();
            remote.commit_then_renew();
            doc.import(&remote.export(ExportMode::all_updates()).unwrap())
                .unwrap();
            assert_eq!(text.to_string(), "sb");
            let prefix = doc.export(ExportMode::all_updates()).unwrap();
            let live: Vec<_> = (0..2)
                .map(|i| {
                    text.get_cursor(i, crate::cursor::Side::Middle)
                        .unwrap()
                        .id
                        .unwrap()
                })
                .collect();
            let order = crate::diff_calc::historical_text_id_order(&doc.oplog().lock(), &text.id());
            let insertions =
                crate::diff_calc::historical_text_insertions(&doc.oplog().lock(), &text.id());
            let candidate_order = causal_gap_candidate_order(&order, &insertions, &[]).unwrap();
            let (_, origins) =
                super::text::with_calculated_restorations(|| inverse(&doc, &[deletion]));
            assert_eq!(origins.len(), 1);
            let target = origins[0].2.unwrap();
            let rank = |id| order.iter().position(|x| *x == id).unwrap();
            let slot = live.iter().filter(|id| rank(**id) < rank(target)).count();
            assert_eq!(slot, if at == 0 { 1 } else { 0 });
            let candidate_rank = |id| candidate_order.iter().position(|x| *x == id).unwrap();
            let candidate_slot = live
                .iter()
                .filter(|id| candidate_rank(**id) < candidate_rank(target))
                .count();
            assert_eq!(candidate_slot, slot);
            let planned = LoroDoc::new_auto_commit();
            planned.import(&prefix).unwrap();
            planned.set_peer_id(99).unwrap();
            planned
                .get_text("text")
                .insert(slot, "a", PosType::Unicode)
                .unwrap();
            planned.commit_then_renew();
            assert_eq!(
                planned.get_text("text").to_string(),
                if at == 0 { "sab" } else { "asb" }
            );
        }
    }
}

/// Check the topology assumption before promoting the previous count-by-rank
/// experiment. A fresh remote insertion after a restored identity stays after it
/// without invalidating redo as a new normal local edit would.
/// Original B precedes a, so B must be restored before that still-visible a.
fn fresh_child_rank_probe(assert_desired: bool) {
    use crate::{cursor::Side, encoding::ExportMode};
    for peer in [1, 2] {
        for restored_cell in ["a", "🦀"] {
            let doc = LoroDoc::new_auto_commit();
            doc.set_peer_id(peer).unwrap();
            let text = doc.get_text("text");
            text.insert(0, restored_cell, PosType::Unicode).unwrap();
            let command_a = checkpoint(&doc, 0);
            let start = get_counter_end(&doc, peer);
            text.insert(0, "B", PosType::Unicode).unwrap();
            let command_b = checkpoint(&doc, start);
            let redo_b = inverse(&doc, &[command_b]);
            let redo_a = inverse(&doc, &[command_a]);
            let (_, restored) = inverse_with_receipts(&doc, &[redo_a]);
            assert_eq!(restored.len(), 1);
            assert_eq!(text.to_string(), restored_cell);
            let remote = LoroDoc::new_auto_commit();
            remote.set_peer_id(3 - peer).unwrap();
            remote
                .import(&doc.export(ExportMode::all_updates()).unwrap())
                .unwrap();
            remote
                .get_text("text")
                .insert(1, "R", PosType::Unicode)
                .unwrap();
            remote.commit_then_renew();
            doc.import(&remote.export(ExportMode::all_updates()).unwrap())
                .unwrap();
            assert_eq!(text.to_string(), format!("{restored_cell}R"));
            let prefix = doc.export(ExportMode::all_updates()).unwrap();
            let live = [0, 1].map(|i| {
                text.get_cursor_internal(i, Side::Middle, false)
                    .unwrap()
                    .id
                    .unwrap()
            });
            let order = crate::diff_calc::historical_text_id_order(&doc.oplog().lock(), &text.id());
            let insertions =
                crate::diff_calc::historical_text_insertions(&doc.oplog().lock(), &text.id());
            let remote_gap = insertions
                .iter()
                .find(|insert| insert.id == live[1])
                .unwrap();
            assert_eq!(remote_gap.left, Some(live[0]));
            assert_eq!(remote_gap.right, None);
            let candidate_order =
                causal_gap_candidate_order(&order, &insertions, &restored).unwrap();
            let (_, origins) =
                super::text::with_calculated_restorations(|| inverse(&doc, &[redo_b]));
            assert_eq!(origins.len(), 1);
            let target = origins[0].2.unwrap();
            // The unmodified native inverse gets this continuation right.
            assert_eq!(text.to_string(), format!("B{restored_cell}R"));
            let rank = |id| order.iter().position(|x| *x == id).unwrap();
            let canonical = |id| {
                restored
                    .iter()
                    .find(|edge| edge.restored == id)
                    .and_then(|edge| edge.origin)
                    .unwrap_or(id)
            };
            let precedes: Vec<_> = live
                .iter()
                .map(|id| rank(canonical(*id)) < rank(target))
                .collect();
            let slot = precedes.iter().filter(|before| **before).count();
            let candidate_rank = |id| candidate_order.iter().position(|x| *x == id).unwrap();
            let candidate_slot = live
                .iter()
                .filter(|id| candidate_rank(canonical(**id)) < candidate_rank(target))
                .count();
            assert_eq!(candidate_slot, 0);
            eprintln!("peer={peer} cell={restored_cell} native={} order={order:?} live={live:?} precedes={precedes:?} rank_slot={slot}", text.to_string());
            let planned = LoroDoc::new_auto_commit();
            planned.import(&prefix).unwrap();
            planned.set_peer_id(99).unwrap();
            planned
                .get_text("text")
                .insert(slot, "B", PosType::Unicode)
                .unwrap();
            planned.commit_then_renew();
            eprintln!("rank-only source={}", planned.get_text("text").to_string());
            // Counting these constraints cannot choose a valid gap: false
            // followed by true is not a prefix. A plan must not publish it.
            assert_eq!(precedes, vec![false, true]);
            let topology_plan = LoroDoc::new_auto_commit();
            topology_plan.import(&prefix).unwrap();
            topology_plan.set_peer_id(98).unwrap();
            let vv_before_query = topology_plan.oplog_vv();
            let mut recovered_gaps = crate::diff_calc::historical_text_insertions(
                &topology_plan.oplog().lock(),
                &text.id(),
            );
            let mut original_gaps = insertions.clone();
            recovered_gaps.sort_by_key(|gap| (gap.id.peer, gap.id.counter));
            original_gaps.sort_by_key(|gap| (gap.id.peer, gap.id.counter));
            assert_eq!(recovered_gaps, original_gaps);
            assert_eq!(topology_plan.oplog_vv(), vv_before_query);
            topology_plan
                .get_text("text")
                .insert(candidate_slot, "B", PosType::Unicode)
                .unwrap();
            topology_plan.commit_then_renew();
            assert_eq!(topology_plan.get_text("text").to_string(), text.to_string());
            if assert_desired {
                assert_eq!(
                    planned.get_text("text").to_string(),
                    format!("B{restored_cell}R")
                );
            } else {
                assert_eq!(
                    planned.get_text("text").to_string(),
                    format!("{restored_cell}BR")
                );
            }
        }
    }
}

#[test]
fn fresh_child_of_restored_identity_exposes_rank_only_placement_limit() {
    fresh_child_rank_probe(false);
}

#[test]
#[ignore = "candidate rank-only placement loses a fresh child's relation to its restored parent; native inverse passes"]
fn rank_only_plan_preserves_restored_parent_order_with_fresh_child() {
    fresh_child_rank_probe(true);
}

/// Cross the real inverse handler boundary, but only with the explicitly armed
/// single-scalar test probe. Ordinary manager/selected behavior is unchanged.
#[test]
fn placement_probe_applies_interior_inverse_and_real_receipts() {
    use crate::{cursor::Side, encoding::ExportMode};
    for peer in [1, 2] {
        for (left, middle) in [("(", "B"), ("(", "🦀"), ("🦀", "B"), ("🦀", "🦀")] {
            let doc = LoroDoc::new_auto_commit();
            doc.set_peer_id(peer).unwrap();
            let text = doc.get_text("text");
            text.insert(0, left, PosType::Unicode).unwrap();
            let trigger = checkpoint(&doc, 0);
            let start = get_counter_end(&doc, peer);
            text.insert(1, middle, PosType::Unicode).unwrap();
            let command = checkpoint(&doc, start);
            let original = text.get_cursor_internal(1, Side::Middle, false).unwrap().id;
            let start = get_counter_end(&doc, peer);
            text.insert(2, ")", PosType::Unicode).unwrap();
            let child = checkpoint(&doc, start);
            let redo_b = inverse(&doc, &[command]);
            let mut redo_a = inverse(&doc, &[trigger, child]);
            let mut edges = Vec::new();
            for cycle in 0..3 {
                let (undo_a, restored) = inverse_with_receipts(&doc, &[redo_a]);
                edges.extend(restored);
                assert_eq!(text.to_string(), format!("{left})"));
                if cycle != 2 {
                    redo_a = inverse(&doc, &[undo_a]);
                }
            }
            let (_, receipts) = super::placement_probe::with_receipts(&edges, || {
                inverse_with_receipts(&doc, &[redo_b])
            });
            assert_eq!(text.to_string(), format!("{left}{middle})"));
            assert_eq!(receipts.len(), 1);
            assert_eq!(receipts[0].origin, original);
            assert_eq!(
                receipts[0].output,
                if cfg!(feature = "wasm") {
                    left.encode_utf16().count()
                } else {
                    1
                }
            );
            assert_eq!(
                Some(receipts[0].restored),
                text.get_cursor_internal(1, Side::Middle, false).unwrap().id
            );
            let recovered = LoroDoc::new_auto_commit();
            recovered
                .import(&doc.export(ExportMode::all_updates()).unwrap())
                .unwrap();
            assert_eq!(recovered.get_text("text").to_string(), text.to_string());
            assert_eq!(recovered.oplog_vv(), doc.oplog_vv());
            assert_eq!(
                recovered
                    .get_text("text")
                    .get_cursor_internal(1, Side::Middle, false)
                    .unwrap()
                    .id,
                Some(receipts[0].restored)
            );
        }
    }
}

#[test]
fn placement_probe_handler_failure_rolls_back_then_retries() {
    use crate::loro::FAIL_APPLY_DIFF_AFTER_HANDLER;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let (doc, trigger, command, child) = fixture();
    let redo_b = inverse(&doc, &[command]);
    let redo_a = inverse(&doc, &[trigger, child]);
    let (restored_pair, edges) = inverse_with_receipts(&doc, &[redo_a]);
    let vv = doc.oplog_vv();
    let frontiers = doc.state_frontiers();
    let events = Arc::new(AtomicUsize::new(0));
    let seen = events.clone();
    let _sub = doc.subscribe_root(Arc::new(move |_| {
        seen.fetch_add(1, Ordering::Relaxed);
    }));
    for _ in 0..2 {
        FAIL_APPLY_DIFF_AFTER_HANDLER.with(|flag| flag.set(true));
        let failed = super::placement_probe::with_receipts(&edges, || {
            doc.undo_internal_ranges_with(
                &[redo_b],
                &mut Default::default(),
                None,
                &mut |_| {},
                &mut None,
                &mut Default::default(),
            )
        });
        assert!(failed.is_err());
        assert_eq!(doc.get_text("text").to_string(), "()");
        assert_eq!(doc.oplog_vv(), vv);
        assert_eq!(doc.state_frontiers(), frontiers);
        assert_eq!(events.load(Ordering::Relaxed), 0);
    }
    let (_, receipts) =
        super::placement_probe::with_receipts(&edges, || inverse_with_receipts(&doc, &[redo_b]));
    assert_eq!(doc.get_text("text").to_string(), "(B)");
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].output, 1);
    // The probe refuses deletion plans without mutating the real source.
    let before = doc.oplog_vv();
    let rejected = super::placement_probe::with_receipts(&edges, || {
        doc.undo_internal_ranges_with(
            &[restored_pair],
            &mut Default::default(),
            None,
            &mut |_| {},
            &mut None,
            &mut Default::default(),
        )
    });
    assert!(rejected.is_err());
    assert_eq!(doc.get_text("text").to_string(), "(B)");
    assert_eq!(doc.oplog_vv(), before);
    // Scope cleanup matters: the same ordinary deletion remains admitted.
    inverse(&doc, &[restored_pair]);
    assert_eq!(doc.get_text("text").to_string(), "B");
}

#[test]
fn placement_probe_applies_before_restored_parent_and_remote_child() {
    use crate::{cursor::Side, encoding::ExportMode};
    for peer in [1, 2] {
        for cell in ["a", "🦀"] {
            let doc = LoroDoc::new_auto_commit();
            doc.set_peer_id(peer).unwrap();
            let text = doc.get_text("text");
            text.insert(0, cell, PosType::Unicode).unwrap();
            let a = checkpoint(&doc, 0);
            let start = get_counter_end(&doc, peer);
            text.insert(0, "B", PosType::Unicode).unwrap();
            let b = checkpoint(&doc, start);
            let redo_b = inverse(&doc, &[b]);
            let redo_a = inverse(&doc, &[a]);
            let (_, edges) = inverse_with_receipts(&doc, &[redo_a]);
            let remote = LoroDoc::new_auto_commit();
            remote.set_peer_id(3 - peer).unwrap();
            remote
                .import(&doc.export(ExportMode::all_updates()).unwrap())
                .unwrap();
            remote
                .get_text("text")
                .insert(1, "R", PosType::Unicode)
                .unwrap();
            remote.commit_then_renew();
            doc.import(&remote.export(ExportMode::all_updates()).unwrap())
                .unwrap();
            let (_, receipts) = super::placement_probe::with_receipts(&edges, || {
                inverse_with_receipts(&doc, &[redo_b])
            });
            assert_eq!(text.to_string(), format!("B{cell}R"));
            assert_eq!(receipts.len(), 1);
            assert_eq!(receipts[0].output, 0);
            assert_eq!(
                Some(receipts[0].restored),
                text.get_cursor_internal(0, Side::Middle, false).unwrap().id
            );
            remote
                .import(&doc.export(ExportMode::all_updates()).unwrap())
                .unwrap();
            assert_eq!(remote.get_text("text").to_string(), text.to_string());
            assert_eq!(remote.oplog_vv(), doc.oplog_vv());
        }
    }
}

#[test]
fn placement_probe_refuses_manager_transform_without_rebase_callback() {
    let (doc, trigger, command, child) = fixture();
    let redo_b = inverse(&doc, &[command]);
    let redo_a = inverse(&doc, &[trigger, child]);
    let (_, edges) = inverse_with_receipts(&doc, &[redo_a]);
    let vv = doc.oplog_vv();
    let frontiers = doc.state_frontiers();
    let mut rebased = false;
    let rejected = super::placement_probe::with_receipts(&edges, || {
        doc.undo_internal_ranges_with(
            &[redo_b],
            &mut Default::default(),
            Some(&DiffBatch::default()),
            &mut |_| {
                rebased = true;
            },
            &mut None,
            &mut Default::default(),
        )
        .is_err()
    });
    assert!(rejected);
    assert!(!rebased);
    assert_eq!(doc.get_text("text").to_string(), "()");
    assert_eq!(doc.oplog_vv(), vv);
    assert_eq!(doc.state_frontiers(), frontiers);
}

fn manager_rebase_after_relocated_inverse_probe(require_desired: bool) {
    use crate::encoding::ExportMode;
    use crate::loro::FAIL_APPLY_DIFF_AFTER_HANDLER;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(1).unwrap();
    let text = doc.get_text("text");
    text.insert(0, "(XB)", PosType::Unicode).unwrap();
    doc.commit_then_renew();
    let manager = UndoManager::new(&doc);
    manager.set_merge_interval(0);
    text.delete(1, 1, PosType::Unicode).unwrap();
    doc.commit_then_renew();
    text.delete(1, 1, PosType::Unicode).unwrap();
    doc.commit_then_renew();
    assert_eq!(manager.undo_count(), 2);
    let remote = LoroDoc::new_auto_commit();
    remote.set_peer_id(2).unwrap();
    remote
        .import(&doc.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let other = remote.get_text("text");
    let start = get_counter_end(&remote, 2);
    other.delete(1, 1, PosType::Unicode).unwrap();
    other.delete(0, 1, PosType::Unicode).unwrap();
    let removed = checkpoint(&remote, start);
    let (_, edges) = inverse_with_receipts(&remote, &[removed]);
    assert_eq!(other.to_string(), "()");
    doc.import(&remote.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let before = doc.oplog_vv();
    let frontiers = doc.state_frontiers();
    let seen = Arc::new(AtomicUsize::new(0));
    let events = seen.clone();
    let _sub = doc.subscribe_root(Arc::new(move |_| {
        events.fetch_add(1, Ordering::Relaxed);
    }));
    super::placement_probe::with_manager_probe(&edges, || {
        for _ in 0..2 {
            FAIL_APPLY_DIFF_AFTER_HANDLER.with(|flag| flag.set(true));
            assert!(manager.undo().is_err());
            assert_eq!(text.to_string(), "()");
            assert_eq!(manager.undo_count(), 2);
            assert_eq!(manager.redo_count(), 0);
            assert_eq!(doc.oplog_vv(), before);
            assert_eq!(doc.state_frontiers(), frontiers);
            assert_eq!(seen.load(Ordering::Relaxed), 0);
        }
        assert!(manager.undo().unwrap());
    });
    assert_eq!(text.to_string(), "(B)");
    assert_eq!(manager.undo_count(), 1);
    assert_eq!(manager.redo_count(), 1);
    assert!(manager.undo().unwrap());
    assert_eq!(manager.undo_count(), 0);
    assert_eq!(manager.redo_count(), 2);
    assert_eq!(
        text.to_string(),
        if require_desired { "(XB)" } else { "X(B)" }
    );
}

#[test]
fn relocating_one_manager_inverse_leaves_older_restoration_misplaced() {
    manager_rebase_after_relocated_inverse_probe(false);
}

#[test]
#[ignore = "relocating the applied inverse alone leaves older manager restoration at X(B), not (XB)"]
fn relocated_manager_inverse_preserves_older_restoration_slot() {
    manager_rebase_after_relocated_inverse_probe(true);
}

/// Unlike the negative control above, replan each successive restoration and
/// retain its actual fresh-ID edge for the next step. Still cfg(test), with
/// full-history replay and no persistent/serialized manager relation.
#[test]
fn manager_probe_replans_successive_restorations_with_actual_receipts() {
    use crate::{cursor::Side, encoding::ExportMode, loro::FAIL_APPLY_DIFF_AFTER_HANDLER};
    for peer in [1, 2] {
        for (left, first, second) in [("(", "X", "B"), ("🦀", "🦀", "🦀")] {
            let doc = LoroDoc::new_auto_commit();
            doc.set_peer_id(peer).unwrap();
            let text = doc.get_text("text");
            text.insert(0, &format!("{left}{first}{second})"), PosType::Unicode)
                .unwrap();
            doc.commit_then_renew();
            let originals: Vec<_> = (0..4)
                .map(|i| {
                    text.get_cursor_internal(i, Side::Middle, false)
                        .unwrap()
                        .id
                        .unwrap()
                })
                .collect();
            let manager = UndoManager::new(&doc);
            manager.set_merge_interval(0);
            text.delete(1, 1, PosType::Unicode).unwrap();
            doc.commit_then_renew();
            text.delete(1, 1, PosType::Unicode).unwrap();
            doc.commit_then_renew();
            let remote = LoroDoc::new_auto_commit();
            remote.set_peer_id(3 - peer).unwrap();
            remote
                .import(&doc.export(ExportMode::all_updates()).unwrap())
                .unwrap();
            let other = remote.get_text("text");
            let start = get_counter_end(&remote, 3 - peer);
            other.delete(1, 1, PosType::Unicode).unwrap();
            other.delete(0, 1, PosType::Unicode).unwrap();
            let deletion = checkpoint(&remote, start);
            let (_, mut edges) = inverse_with_receipts(&remote, &[deletion]);
            doc.import(&remote.export(ExportMode::all_updates()).unwrap())
                .unwrap();
            for _ in 0..3 {
                edges = super::placement_probe::with_manager_probe(&edges, || {
                    assert!(manager.undo().unwrap());
                    assert_eq!(text.to_string(), format!("{left}{second})"));
                    let after_first = super::placement_probe::manager_receipts();
                    assert_eq!(after_first.len(), edges.len() + 1);
                    let last = after_first.last().unwrap();
                    assert_eq!(
                        Some(last.restored),
                        text.get_cursor_internal(1, Side::Middle, false).unwrap().id
                    );
                    let before = doc.oplog_vv();
                    let frontiers = doc.state_frontiers();
                    for _ in 0..2 {
                        FAIL_APPLY_DIFF_AFTER_HANDLER.with(|flag| flag.set(true));
                        assert!(manager.undo().is_err());
                        assert_eq!(text.to_string(), format!("{left}{second})"));
                        assert_eq!(doc.oplog_vv(), before);
                        assert_eq!(doc.state_frontiers(), frontiers);
                        assert_eq!(manager.undo_count(), 1);
                        assert_eq!(manager.redo_count(), 1);
                        assert_eq!(super::placement_probe::manager_receipts(), after_first);
                    }
                    assert!(manager.undo().unwrap());
                    assert_eq!(text.to_string(), format!("{left}{first}{second})"));
                    let after_second = super::placement_probe::manager_receipts();
                    assert_eq!(after_second.len(), after_first.len() + 1);
                    assert_eq!(
                        Some(after_second.last().unwrap().restored),
                        text.get_cursor_internal(1, Side::Middle, false).unwrap().id
                    );
                    assert_eq!(manager.undo_count(), 0);
                    assert_eq!(manager.redo_count(), 2);
                    for (receipt, original) in [
                        (after_first.last().unwrap(), originals[2]),
                        (after_second.last().unwrap(), originals[1]),
                    ] {
                        let mut origin = receipt.origin.unwrap();
                        for _ in 0..after_second.len() {
                            match after_second
                                .iter()
                                .find(|e| e.restored == origin)
                                .and_then(|e| e.origin)
                            {
                                Some(parent) => origin = parent,
                                None => break,
                            }
                        }
                        assert_eq!(
                            origin, original,
                            "equal spelling must not erase logical origin"
                        );
                    }
                    let recovered = LoroDoc::new_auto_commit();
                    recovered
                        .import(&doc.export(ExportMode::all_updates()).unwrap())
                        .unwrap();
                    let restored_text = recovered.get_text("text");
                    assert_eq!(restored_text.to_string(), text.to_string());
                    assert_eq!(recovered.oplog_vv(), doc.oplog_vv());
                    assert_eq!(
                        restored_text
                            .get_cursor_internal(1, Side::Middle, false)
                            .unwrap()
                            .id,
                        Some(after_second.last().unwrap().restored)
                    );
                    assert_eq!(
                        restored_text
                            .get_cursor_internal(2, Side::Middle, false)
                            .unwrap()
                            .id,
                        Some(after_first.last().unwrap().restored)
                    );
                    after_second
                });
                // Ordinary deletion inverses stay outside the single-insert
                // probe's admission scope; no relaxed deletion policy is used.
                assert!(manager.redo().unwrap());
                assert_eq!(text.to_string(), format!("{left}{second})"));
                assert!(manager.redo().unwrap());
                assert_eq!(text.to_string(), format!("{left})"));
                assert_eq!(manager.undo_count(), 2);
                assert_eq!(manager.redo_count(), 0);
            }
            // Actual exported state still converges. Lineage remains separate.
            remote
                .import(&doc.export(ExportMode::all_updates()).unwrap())
                .unwrap();
            assert_eq!(other.to_string(), text.to_string());
            assert_eq!(remote.oplog_vv(), doc.oplog_vv());
            assert!(edges.iter().any(|edge| edge.origin == Some(originals[1])));
            assert!(edges.iter().any(|edge| edge.origin == Some(originals[2])));
        }
    }
}
