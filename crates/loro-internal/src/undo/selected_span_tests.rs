//! Explicit native range selection, before adopting a manager/trigger API.
use super::*;
use crate::{cursor::PosType, HandlerTrait};

fn checkpoint(doc: &LoroDoc, start: Counter) -> IdSpan {
    doc.commit_then_renew();
    IdSpan::new(doc.peer_id(), start, get_counter_end(doc, doc.peer_id()))
}

fn inverse(doc: &LoroDoc, spans: &[IdSpan]) -> IdSpan {
    let start = get_counter_end(doc, doc.peer_id());
    drop(
        doc.undo_internal_ranges_with(
            spans,
            &mut Default::default(),
            None,
            &mut |_| {},
            &mut None,
            &mut Default::default(),
        )
        .unwrap(),
    );
    checkpoint(doc, start)
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
