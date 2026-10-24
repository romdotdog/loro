use loro::{ExportMode, LoroDoc, UndoManager};
fn sync(a: &LoroDoc, b: &LoroDoc) {
    b.import(&a.export(ExportMode::updates(&b.oplog_vv())).unwrap())
        .unwrap();
}
#[test]
fn overlapping_text_import_ends_the_active_group() {
    let a = LoroDoc::new();
    let b = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    b.set_peer_id(2).unwrap();
    a.get_text("text").insert(0, "y|").unwrap();
    a.commit();
    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(0);
    undo.group_start().unwrap();
    a.get_text("text").insert(1, "p").unwrap();
    a.commit();
    sync(&a, &b);
    b.get_text("text").insert(2, "S").unwrap();
    b.commit();
    sync(&b, &a);
    a.get_text("text").delete(1, 1).unwrap();
    a.get_text("text").insert(1, "q").unwrap();
    a.commit();
    undo.group_end();
    assert_eq!(a.get_text("text").to_string(), "yqS|");
    // Native policy splits on an overlapping import, even inside an explicit
    // group. Restoring p here undoes only the later replacement, not the group.
    assert_eq!(undo.undo_count(), 2);
    assert!(undo.undo().unwrap());
    assert_eq!(a.get_text("text").to_string(), "ypS|");
    assert!(undo.undo().unwrap());
    assert_eq!(a.get_text("text").to_string(), "yS|");
    assert!(!undo.can_undo());
    assert!(undo.redo().unwrap());
    assert_eq!(a.get_text("text").to_string(), "ypS|");
    assert!(undo.redo().unwrap());
    assert_eq!(a.get_text("text").to_string(), "yqS|");
}

#[test]
fn split_before_import_preserves_two_actual_groups() {
    let a = LoroDoc::new();
    let b = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    b.set_peer_id(2).unwrap();
    a.get_text("text").insert(0, "y|").unwrap();
    a.commit();
    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(0);
    undo.group_start().unwrap();
    a.get_text("text").insert(1, "p").unwrap();
    a.commit();
    sync(&a, &b);
    b.get_text("text").insert(2, "S").unwrap();
    b.commit();
    // Dark closes an ordinary history group before an admitted remote import.
    undo.group_end();
    sync(&b, &a);
    undo.group_start().unwrap();
    a.get_text("text").delete(1, 1).unwrap();
    a.get_text("text").insert(1, "q").unwrap();
    a.commit();
    undo.group_end();
    assert_eq!(a.get_text("text").to_string(), "yqS|");
    assert!(undo.undo().unwrap());
    assert_eq!(a.get_text("text").to_string(), "ypS|");
    assert!(undo.undo().unwrap());
    assert_eq!(a.get_text("text").to_string(), "yS|");
    assert!(!undo.can_undo());
    assert!(undo.redo().unwrap());
    assert_eq!(a.get_text("text").to_string(), "ypS|");
    assert!(undo.redo().unwrap());
    assert_eq!(a.get_text("text").to_string(), "yqS|");
    assert!(!undo.can_redo());
}

#[test]
#[ignore = "known native text intent failure: undo restores a concurrently deleted original ID"]
fn concurrent_delete_of_locally_overwritten_text_is_not_restored() {
    let mut actual = Vec::new();
    let mut expected = Vec::new();
    for peers in [(1, 2), (2, 1)] {
        for remote in ["B", "🦀"] {
            let a = LoroDoc::new();
            let b = LoroDoc::new();
            a.set_peer_id(peers.0).unwrap();
            b.set_peer_id(peers.1).unwrap();
            let t = a.get_text("text");
            t.insert(0, "a").unwrap();
            a.commit();
            sync(&a, &b);
            b.get_text("text").insert(1, remote).unwrap();
            b.commit();
            sync(&b, &a);
            let mut undo = UndoManager::new(&a);
            undo.set_merge_interval(0);
            // Concurrent deletes of the same original ID: A also creates x.
            t.delete(1, 1).unwrap();
            t.insert(1, "x").unwrap();
            a.commit();
            b.get_text("text").delete(1, 1).unwrap();
            b.commit();
            assert_eq!(t.to_string(), "ax");
            sync(&b, &a);
            assert_eq!(t.to_string(), "ax", "remote delete is invisible locally");
            assert!(undo.undo().unwrap());
            actual.push((peers, remote, t.to_string()));
            expected.push((peers, remote, "a".to_string()));
        }
    }
    assert_eq!(
        actual, expected,
        "undo must preserve the concurrent remote deletion"
    );
}
