//! Public Rust feature boundary, not a test-only manager substitute.
#[cfg(feature = "experimental-identity-undo")]
#[test]
fn public_trigger_api_preserves_the_intervening_command_through_redo() {
    use loro::{LoroDoc, UndoManager};
    let doc = LoroDoc::new();
    doc.enable_identity_history().unwrap();
    let text = doc.get_text("text");
    let mut undo = UndoManager::new_identity(&doc).unwrap();
    undo.set_merge_interval(0);
    text.insert(0, "(").unwrap();
    doc.commit();
    let trigger = undo.capture_trigger().unwrap();
    text.insert(1, "B").unwrap();
    doc.commit();
    undo.with_derived(&trigger, || {
        text.insert(2, ")").unwrap();
        doc.commit();
    })
    .unwrap();
    for _ in 0..3 {
        assert!(undo.undo().unwrap());
        assert_eq!(text.to_string(), "()");
        assert!(undo.undo().unwrap());
        assert_eq!(text.to_string(), "");
        assert!(undo.redo().unwrap());
        assert_eq!(text.to_string(), "()");
        assert!(undo.redo().unwrap());
        assert_eq!(text.to_string(), "(B)");
        undo.check_trigger(&trigger).unwrap();
    }
}

#[cfg(feature = "experimental-identity-undo")]
fn public_peer_retirement() -> String {
    use loro::{ExportMode, LoroDoc, UndoManager};
    let a = LoroDoc::new();
    let b = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    b.set_peer_id(2).unwrap();
    a.enable_identity_history().unwrap();
    b.enable_identity_history().unwrap();
    let text = a.get_text("text");
    let mut undo_a = UndoManager::new_identity(&a).unwrap();
    text.insert(0, "a").unwrap();
    a.commit();
    let trigger = undo_a.capture_trigger().unwrap();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let mut undo_b = UndoManager::new_identity(&b).unwrap();
    b.get_text("text").insert(1, "B").unwrap();
    b.commit();
    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    undo_a
        .with_derived(&trigger, || {
            text.delete(1, 1).unwrap();
            text.insert(1, "x").unwrap();
        })
        .unwrap();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let before = b.export(ExportMode::Snapshot).unwrap();
    let vv = b.oplog_vv();
    assert!(!undo_b.undo().unwrap());
    assert_eq!((undo_b.undo_count(), undo_b.redo_count()), (0, 0));
    assert_eq!(b.oplog_vv(), vv);
    assert_eq!(b.export(ExportMode::Snapshot).unwrap(), before);
    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert!(undo_a.undo().unwrap());
    text.to_string()
}

#[cfg(feature = "experimental-identity-undo")]
#[test]
fn public_noop_retirement_has_no_recoverable_activity_record() {
    // Observed boundary, not acceptance of the resurrection semantics.
    assert_eq!(public_peer_retirement(), "B");
}

#[cfg(feature = "experimental-identity-undo")]
#[test]
#[ignore = "desired inactive peer creation is absent from exported native history"]
fn public_retired_peer_creation_is_not_resurrected() {
    assert_eq!(public_peer_retirement(), "");
}
