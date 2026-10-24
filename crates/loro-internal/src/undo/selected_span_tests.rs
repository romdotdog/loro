//! Explicit native range selection, before adopting a manager/trigger API.
use super::*;
use crate::cursor::PosType;

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
