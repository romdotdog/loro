//! Candidate atomicity includes its manager-owned, release-compiled relation.
use super::*;
use crate::{cursor::PosType, loro::FAIL_APPLY_DIFF_AFTER_HANDLER};
use std::sync::atomic::{AtomicUsize, Ordering};
#[test]
fn candidate_errors_preserve_manager_relation_and_both_stacks() {
    for list in [false, true] {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let text = doc.get_text("text");
        let seq = doc.get_list("sequence");
        if list {
            for (i, s) in ["a", "B"].iter().enumerate() {
                seq.insert(i, *s).unwrap();
            }
        } else {
            text.insert(0, "aB", PosType::Unicode).unwrap();
        }
        doc.commit_then_renew();
        let manager = UndoManager::new_identity(&doc);
        if list {
            seq.delete(1, 1).unwrap();
            seq.insert(1, "x").unwrap();
        } else {
            text.delete(1, 1, PosType::Unicode).unwrap();
            text.insert(1, "x", PosType::Unicode).unwrap();
        }
        doc.commit_then_renew();
        let events = Arc::new(AtomicUsize::new(0));
        let observed = events.clone();
        let _sub = doc.subscribe_root(Arc::new(move |_| {
            observed.fetch_add(1, Ordering::Relaxed);
        }));
        for _ in 0..3 {
            for redo in [false, true] {
                let before = doc.get_deep_value();
                let vv = doc.oplog_vv();
                let frontiers = doc.state_frontiers();
                let edges = manager.identity_receipts.lock().clone();
                let counts = (manager.undo_count(), manager.redo_count());
                let n = events.load(Ordering::Relaxed);
                for _ in 0..2 {
                    FAIL_APPLY_DIFF_AFTER_HANDLER.with(|flag| flag.set(true));
                    let result = if redo { manager.redo() } else { manager.undo() };
                    assert!(result.is_err());
                    assert_eq!(doc.get_deep_value(), before);
                    assert_eq!(doc.oplog_vv(), vv);
                    assert_eq!(doc.state_frontiers(), frontiers);
                    assert_eq!((manager.undo_count(), manager.redo_count()), counts);
                    assert_eq!(*manager.identity_receipts.lock(), edges);
                    assert_eq!(events.load(Ordering::Relaxed), n);
                }
                assert!(if redo {
                    manager.redo().unwrap()
                } else {
                    manager.undo().unwrap()
                });
                assert_eq!(
                    manager.identity_receipts.lock().as_ref().unwrap().len(),
                    edges.as_ref().unwrap().len() + 1
                );
            }
        }
    }
}
