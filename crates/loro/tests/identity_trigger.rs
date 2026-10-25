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
