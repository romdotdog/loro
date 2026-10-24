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
