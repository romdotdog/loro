use super::*;
use crate::cursor::PosType;

#[test]
fn a_backward_recorded_clock_does_not_merge_separate_items() {
    for interval in [0, 500] {
        let doc = LoroDoc::new_auto_commit();
        let undo = UndoManager::new(&doc);
        undo.set_merge_interval(interval);
        let text = doc.get_text("text");
        text.insert(0, "a", PosType::Unicode).unwrap();
        doc.commit_then_renew();
        // Simulate a wall-clock retreat relative to the previous checkpoint.
        // No process/global clock replacement or sleeps are required.
        undo.inner.lock().borrow_mut().last_undo_time = get_sys_timestamp() as Timestamp + 60_000;
        text.insert(1, "b", PosType::Unicode).unwrap();
        doc.commit_then_renew();
        assert_eq!(undo.undo_count(), 2, "interval={interval}");
        assert!(undo.undo().unwrap());
        assert_eq!(text.to_string(), "a");
        assert!(undo.undo().unwrap());
        assert_eq!(text.to_string(), "");
    }
}

#[test]
fn explicit_grouping_still_coalesces_after_a_clock_retreat() {
    let doc = LoroDoc::new_auto_commit();
    let undo = UndoManager::new(&doc);
    undo.set_merge_interval(0);
    undo.group_start().unwrap();
    let text = doc.get_text("text");
    text.insert(0, "a", PosType::Unicode).unwrap();
    doc.commit_then_renew();
    undo.inner.lock().borrow_mut().last_undo_time = get_sys_timestamp() as Timestamp + 60_000;
    text.insert(1, "b", PosType::Unicode).unwrap();
    doc.commit_then_renew();
    undo.group_end();
    assert_eq!(undo.undo_count(), 1);
    assert!(undo.undo().unwrap());
    assert_eq!(text.to_string(), "");
}
