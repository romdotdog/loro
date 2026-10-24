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
            for _ in 0..3 {
                assert!(undo.redo().unwrap());
                assert_eq!(t.to_string(), "ax");
                assert!(undo.undo().unwrap());
                assert_eq!(t.to_string(), "a");
            }
        }
    }
    assert_eq!(
        actual, expected,
        "undo must preserve the concurrent remote deletion"
    );
}

#[test]
fn partial_foreign_delete_of_an_overwritten_run_preserves_other_characters() {
    for peers in [(1, 2), (2, 1)] {
        for original in ["BCD", "B🦀C"] {
            for (start, len) in [(0, 1), (1, 1), (1, 2)] {
                for replacement in ["x", ""] {
                    for append in [false, true] {
                        let a = LoroDoc::new();
                        let b = LoroDoc::new();
                        a.set_peer_id(peers.0).unwrap();
                        b.set_peer_id(peers.1).unwrap();
                        let text = a.get_text("text");
                        text.insert(0, "a|").unwrap();
                        a.commit();
                        sync(&a, &b);
                        b.get_text("text").insert(1, original).unwrap();
                        b.commit();
                        sync(&b, &a);
                        let mut undo = UndoManager::new(&a);
                        undo.set_merge_interval(0);
                        text.delete(1, 3).unwrap();
                        text.insert(1, replacement).unwrap();
                        a.commit();
                        let remote = b.get_text("text");
                        remote.delete(1 + start, len).unwrap();
                        if append {
                            remote.insert(remote.len_unicode() - 1, "R").unwrap();
                        }
                        b.commit();
                        let expected = remote.to_string();
                        sync(&b, &a);
                        let merged = text.to_string();
                        for cycle in 0..3 {
                            assert!(undo.undo().unwrap());
                            assert_eq!(text.to_string(), expected,
                                "{peers:?}/{original}/{start}/{len}/{replacement}/{append}/undo{cycle}");
                            assert!(!undo.can_undo());
                            assert!(undo.redo().unwrap());
                            assert_eq!(text.to_string(), merged,
                                "{peers:?}/{original}/{start}/{len}/{replacement}/{append}/redo{cycle}");
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn fully_duplicated_delete_consumes_only_its_noop_item() {
    let a = LoroDoc::new();
    let b = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    b.set_peer_id(2).unwrap();
    a.get_text("text").insert(0, "aB").unwrap();
    a.commit();
    sync(&a, &b);
    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(0);
    a.get_map("meta").insert("keep", true).unwrap();
    a.commit();
    a.get_text("text").delete(1, 1).unwrap();
    a.commit();
    b.get_text("text").delete(1, 1).unwrap();
    b.commit();
    sync(&b, &a);
    let before = a.oplog_vv();
    assert!(!undo.undo().unwrap());
    assert_eq!(a.oplog_vv(), before, "no compensating CRDT operation");
    assert_eq!(a.get_text("text").to_string(), "a");
    assert_eq!(a.get_map("meta").len(), 1, "older item survives");
    assert!(undo.can_undo());
    assert!(!undo.can_redo(), "native no-op has no opposite entry");
    assert!(undo.undo().unwrap());
    assert_eq!(a.get_map("meta").len(), 0);
}

#[test]
fn restored_survivors_retain_attributes_around_invisible_foreign_delete() {
    let a = LoroDoc::new();
    let b = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    b.set_peer_id(2).unwrap();
    let text = a.get_text("text");
    text.insert(0, "aB🦀C|").unwrap();
    text.mark(1..4, "bold", true).unwrap();
    a.commit();
    sync(&a, &b);
    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(0);
    text.delete(1, 3).unwrap();
    text.insert(1, "x").unwrap();
    a.commit();
    b.get_text("text").delete(2, 1).unwrap();
    b.commit();
    let expected = b.get_text("text").to_delta();
    sync(&b, &a);
    let merged = text.to_delta();
    for _ in 0..3 {
        assert!(undo.undo().unwrap());
        assert_eq!(text.to_delta(), expected);
        assert!(undo.redo().unwrap());
        assert_eq!(text.to_delta(), merged);
    }
}

#[test]
#[ignore = "text intent filtering does not yet compose across dependency-separated spans"]
fn foreign_delete_intent_survives_dependency_separated_group_spans() {
    let a = LoroDoc::new();
    let b = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    b.set_peer_id(2).unwrap();
    let text = a.get_text("text");
    text.insert(0, "aB").unwrap();
    a.commit();
    sync(&a, &b);
    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(0);
    undo.group_start().unwrap();
    text.delete(1, 1).unwrap();
    text.insert(1, "x").unwrap();
    a.commit();
    b.get_text("text").delete(1, 1).unwrap();
    b.get_map("meta").insert("flag", true).unwrap();
    b.commit();
    sync(&b, &a);
    text.delete(1, 1).unwrap();
    text.insert(1, "y").unwrap();
    a.commit();
    undo.group_end();
    assert_eq!(undo.undo_count(), 1, "the visible map import is disjoint");
    assert!(undo.undo().unwrap());
    assert_eq!(text.to_string(), "a", "preserve B's original deletion");
}
