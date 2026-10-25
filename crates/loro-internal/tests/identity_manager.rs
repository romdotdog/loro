//! Release-compiled native identity candidate: no cfg(test) planner or TLS scope.
use loro_internal::{cursor::PosType, loro::ExportMode, LoroDoc, UndoManager};
fn sync(a: &LoroDoc, b: &LoroDoc) {
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
}
#[test]
fn mixed_replacements_and_manager_owned_links_cycle() {
    for peer in [1, 2] {
        for cell in ["a", "🦀"] {
            let a = LoroDoc::new_auto_commit();
            a.set_peer_id(peer).unwrap();
            let text = a.get_text("text");
            text.insert(0, &format!("{cell}B"), PosType::Unicode)
                .unwrap();
            a.commit_then_renew();
            let undo = UndoManager::new_identity(&a);
            undo.set_merge_interval(0);
            text.delete(1, 1, PosType::Unicode).unwrap();
            text.insert(1, "x", PosType::Unicode).unwrap();
            a.commit_then_renew();
            for _ in 0..4 {
                assert!(undo.undo().unwrap());
                assert_eq!(text.to_string(), format!("{cell}B"));
                assert!(undo.redo().unwrap());
                assert_eq!(text.to_string(), format!("{cell}x"));
            }
        }
    }
}
#[test]
fn mirrored_deleted_target_slots_are_distinct() {
    for peer in [1, 2] {
        for at in [0, 1] {
            let a = LoroDoc::new_auto_commit();
            a.set_peer_id(peer).unwrap();
            let b = LoroDoc::new_auto_commit();
            b.set_peer_id(3 - peer).unwrap();
            let text = a.get_text("text");
            text.insert(0, "ab", PosType::Unicode).unwrap();
            a.commit_then_renew();
            sync(&a, &b);
            let undo = UndoManager::new_identity(&a);
            text.delete(0, 1, PosType::Unicode).unwrap();
            a.commit_then_renew();
            b.get_text("text")
                .insert(at, "s", PosType::Unicode)
                .unwrap();
            b.commit_then_renew();
            sync(&b, &a);
            assert!(undo.undo().unwrap());
            assert_eq!(text.to_string(), if at == 0 { "sab" } else { "asb" });
            assert!(undo.redo().unwrap());
            assert_eq!(text.to_string(), "sb");
        }
    }
}
#[test]
fn own_overwrite_undo_does_not_restore_foreign_deleted_cell() {
    for peer in [1, 2] {
        let a = LoroDoc::new_auto_commit();
        a.set_peer_id(peer).unwrap();
        let b = LoroDoc::new_auto_commit();
        b.set_peer_id(3 - peer).unwrap();
        let t = a.get_text("text");
        t.insert(0, "aB", PosType::Unicode).unwrap();
        a.commit_then_renew();
        sync(&a, &b);
        let undo = UndoManager::new_identity(&a);
        t.delete(1, 1, PosType::Unicode).unwrap();
        t.insert(1, "x", PosType::Unicode).unwrap();
        a.commit_then_renew();
        b.get_text("text").delete(1, 1, PosType::Unicode).unwrap();
        b.commit_then_renew();
        sync(&b, &a);
        for _ in 0..3 {
            assert!(undo.undo().unwrap());
            assert_eq!(t.to_string(), "a");
            assert!(undo.redo().unwrap());
            assert_eq!(t.to_string(), "ax");
        }
    }
}

enum Sequence {
    Text(loro_internal::handler::TextHandler),
    List(loro_internal::handler::ListHandler),
}
impl Sequence {
    fn new(doc: &LoroDoc, list: bool) -> Self {
        if list {
            Self::List(doc.get_list("sequence"))
        } else {
            Self::Text(doc.get_text("sequence"))
        }
    }
    fn insert(&self, at: usize, value: &str) {
        match self {
            Self::Text(t) => t.insert(at, value, PosType::Unicode).unwrap(),
            Self::List(l) => l.insert(at, value).unwrap(),
        }
    }
    fn delete(&self, at: usize) {
        match self {
            Self::Text(t) => t.delete(at, 1, PosType::Unicode).unwrap(),
            Self::List(l) => l.delete(at, 1).unwrap(),
        }
    }
    fn source(&self) -> String {
        match self {
            Self::Text(t) => t.to_string(),
            Self::List(l) => (0..l.len())
                .map(|i| match l.get(i).unwrap() {
                    loro_common::LoroValue::String(s) => s.to_string(),
                    _ => panic!("string fixture"),
                })
                .collect(),
        }
    }
}
fn delivery(list: bool, incremental: bool) -> String {
    let a = LoroDoc::new_auto_commit();
    a.set_peer_id(1).unwrap();
    let b = LoroDoc::new_auto_commit();
    b.set_peer_id(2).unwrap();
    let local = Sequence::new(&a, list);
    let other = Sequence::new(&b, list);
    local.insert(0, "a");
    local.insert(1, "b");
    a.commit_then_renew();
    sync(&a, &b);
    local.insert(1, "x");
    a.commit_then_renew();
    other.insert(0, "r");
    b.commit_then_renew();
    sync(&b, &a);
    let undo = UndoManager::new_identity(&a);
    undo.set_merge_interval(0);
    local.delete(1);
    a.commit_then_renew();
    other.delete(0);
    b.commit_then_renew();
    if incremental {
        sync(&b, &a);
    }
    other.delete(1);
    b.commit_then_renew();
    if incremental {
        sync(&b, &a);
    }
    other.insert(0, "s");
    b.commit_then_renew();
    sync(&b, &a);
    assert_eq!(local.source(), "sx");
    assert!(undo.undo().unwrap());
    let after = local.source();
    sync(&a, &b);
    assert_eq!(other.source(), after);
    let recovered = LoroDoc::new_auto_commit();
    sync(&a, &recovered);
    assert_eq!(Sequence::new(&recovered, list).source(), after);
    assert!(undo.redo().unwrap());
    assert_eq!(local.source(), "sx");
    after
}
#[test]
fn primary_unfiled_delivery_schedule_in_both_sequence_types() {
    for list in [false, true] {
        assert_eq!(delivery(list, false), "sax");
        assert_eq!(delivery(list, true), "sax");
    }
}
#[test]
fn list_mirrored_placement_and_replacement_cycles() {
    for peer in [1, 2] {
        for at in [0, 1] {
            let a = LoroDoc::new_auto_commit();
            a.set_peer_id(peer).unwrap();
            let b = LoroDoc::new_auto_commit();
            b.set_peer_id(3 - peer).unwrap();
            let l = Sequence::new(&a, true);
            let r = Sequence::new(&b, true);
            l.insert(0, "a");
            l.insert(1, "b");
            a.commit_then_renew();
            sync(&a, &b);
            let undo = UndoManager::new_identity(&a);
            l.delete(0);
            a.commit_then_renew();
            r.insert(at, "s");
            b.commit_then_renew();
            sync(&b, &a);
            for _ in 0..3 {
                assert!(undo.undo().unwrap());
                assert_eq!(l.source(), if at == 0 { "sab" } else { "asb" });
                assert!(undo.redo().unwrap());
                assert_eq!(l.source(), "sb");
            }
        }
    }
}

#[test]
fn manager_bootstraps_and_cycles_without_fixture_relations_or_test_scopes() {
    for list in [false, true] {
        for peer in [1, 2] {
            let a = LoroDoc::new_auto_commit();
            a.set_peer_id(peer).unwrap();
            let b = LoroDoc::new_auto_commit();
            b.set_peer_id(3 - peer).unwrap();
            let l = Sequence::new(&a, list);
            let r = Sequence::new(&b, list);
            for (at, s) in ["(", "X", "B", ")"].iter().enumerate() {
                l.insert(at, s);
            }
            a.commit_then_renew();
            sync(&a, &b);
            r.insert(2, "R");
            b.commit_then_renew();
            let undo = UndoManager::new_identity(&a);
            undo.set_merge_interval(0);
            l.delete(1);
            a.commit_then_renew();
            l.delete(1);
            a.commit_then_renew();
            l.delete(1);
            l.delete(0);
            a.commit_then_renew();
            sync(&b, &a);
            assert_eq!(undo.undo_count(), 3);
            for _ in 0..4 {
                for expected in ["(R)", "(RB)", "(XRB)"] {
                    assert!(undo.undo().unwrap());
                    assert_eq!(l.source(), expected);
                }
                for expected in ["(RB)", "(R)", "R"] {
                    assert!(undo.redo().unwrap());
                    assert_eq!(l.source(), expected);
                }
            }
            sync(&a, &b);
            assert_eq!(r.source(), "R");
            assert_eq!(a.oplog_vv(), b.oplog_vv());
        }
    }
}
#[test]
fn list_mixed_replacement_preserves_foreign_deletion() {
    for peer in [1, 2] {
        let a = LoroDoc::new_auto_commit();
        a.set_peer_id(peer).unwrap();
        let b = LoroDoc::new_auto_commit();
        b.set_peer_id(3 - peer).unwrap();
        let l = Sequence::new(&a, true);
        let r = Sequence::new(&b, true);
        l.insert(0, "a");
        l.insert(1, "B");
        a.commit_then_renew();
        sync(&a, &b);
        let undo = UndoManager::new_identity(&a);
        l.delete(1);
        l.insert(1, "x");
        a.commit_then_renew();
        r.delete(1);
        b.commit_then_renew();
        sync(&b, &a);
        for _ in 0..3 {
            assert!(undo.undo().unwrap());
            assert_eq!(l.source(), "a");
            assert!(undo.redo().unwrap());
            assert_eq!(l.source(), "ax");
        }
    }
}

#[test]
fn noops_still_consume_one_item_and_never_create_virtual_opposites() {
    for list in [false, true] {
        let a = LoroDoc::new_auto_commit();
        a.set_peer_id(1).unwrap();
        let b = LoroDoc::new_auto_commit();
        b.set_peer_id(2).unwrap();
        let l = Sequence::new(&a, list);
        let r = Sequence::new(&b, list);
        let undo = UndoManager::new_identity(&a);
        l.insert(0, "a");
        a.commit_then_renew();
        l.insert(1, "b");
        a.commit_then_renew();
        sync(&a, &b);
        r.delete(1);
        r.delete(0);
        b.commit_then_renew();
        sync(&b, &a);
        assert!(!undo.undo().unwrap());
        assert_eq!(undo.undo_count(), 1);
        assert_eq!(undo.redo_count(), 0);
        assert!(!undo.undo().unwrap());
        assert_eq!(undo.undo_count(), 0);
        assert_eq!(undo.redo_count(), 0);
    }
}

#[test]
fn quota_sized_mixed_text_roundtrip() {
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(1).unwrap();
    let text = doc.get_text("text");
    text.insert(0, &"a".repeat(4096), PosType::Unicode).unwrap();
    doc.commit_then_renew();
    let undo = UndoManager::new_identity(&doc);
    text.delete(1000, 1000, PosType::Unicode).unwrap();
    text.insert(1000, &"b".repeat(1000), PosType::Unicode)
        .unwrap();
    doc.commit_then_renew();
    let start = std::time::Instant::now();
    assert!(undo.undo().unwrap());
    assert_eq!(text.to_string(), "a".repeat(4096));
    assert!(undo.redo().unwrap());
    assert_eq!(
        text.to_string(),
        format!(
            "{}{}{}",
            "a".repeat(1000),
            "b".repeat(1000),
            "a".repeat(2096)
        )
    );
    eprintln!("4096-scalar mixed undo+redo {:?}", start.elapsed());
}

#[test]
fn restored_parent_remote_child_and_original_predecessor_order() {
    for list in [false, true] {
        for peer in [1, 2] {
            let a = LoroDoc::new_auto_commit();
            a.set_peer_id(peer).unwrap();
            let b = LoroDoc::new_auto_commit();
            b.set_peer_id(3 - peer).unwrap();
            let l = Sequence::new(&a, list);
            let r = Sequence::new(&b, list);
            let undo = UndoManager::new_identity(&a);
            l.insert(0, "a");
            a.commit_then_renew();
            l.insert(0, "B");
            a.commit_then_renew();
            assert!(undo.undo().unwrap());
            assert!(undo.undo().unwrap());
            assert_eq!(l.source(), "");
            assert!(undo.redo().unwrap());
            sync(&a, &b);
            r.insert(1, "R");
            b.commit_then_renew();
            sync(&b, &a);
            assert!(undo.redo().unwrap());
            assert_eq!(l.source(), "BaR");
            sync(&a, &b);
            assert_eq!(r.source(), "BaR");
        }
    }
}
#[test]
fn foreign_delete_of_a_restored_id_is_not_undone() {
    for list in [false, true] {
        for peer in [1, 2] {
            let a = LoroDoc::new_auto_commit();
            a.set_peer_id(peer).unwrap();
            let b = LoroDoc::new_auto_commit();
            b.set_peer_id(3 - peer).unwrap();
            let l = Sequence::new(&a, list);
            let r = Sequence::new(&b, list);
            l.insert(0, "a");
            l.insert(1, "B");
            a.commit_then_renew();
            let undo = UndoManager::new_identity(&a);
            l.delete(1);
            a.commit_then_renew();
            assert!(undo.undo().unwrap());
            sync(&a, &b);
            assert!(undo.redo().unwrap());
            r.delete(1);
            b.commit_then_renew();
            sync(&b, &a);
            assert!(!undo.undo().unwrap());
            assert_eq!(l.source(), "a");
            assert_eq!(undo.undo_count(), 0);
            assert_eq!(undo.redo_count(), 0);
        }
    }
}
#[test]
fn multi_hunk_mixed_inverse_preserves_remote_insertion_and_anchor_removal() {
    for list in [false, true] {
        for peer in [1, 2] {
            let a = LoroDoc::new_auto_commit();
            a.set_peer_id(peer).unwrap();
            let b = LoroDoc::new_auto_commit();
            b.set_peer_id(3 - peer).unwrap();
            let l = Sequence::new(&a, list);
            let r = Sequence::new(&b, list);
            for (i, s) in ["a", "b", "c", "d", "e", "f"].iter().enumerate() {
                l.insert(i, s);
            }
            a.commit_then_renew();
            sync(&a, &b);
            let undo = UndoManager::new_identity(&a);
            l.delete(4);
            l.insert(4, "Y");
            l.delete(1);
            l.insert(1, "X");
            a.commit_then_renew();
            r.insert(3, "R");
            r.delete(0);
            b.commit_then_renew();
            sync(&b, &a);
            assert_eq!(l.source(), "XcRdYf");
            for _ in 0..3 {
                assert!(undo.undo().unwrap());
                assert_eq!(l.source(), "bcRdef");
                assert!(undo.redo().unwrap());
                assert_eq!(l.source(), "XcRdYf");
            }
            sync(&a, &b);
            assert_eq!(r.source(), "XcRdYf");
        }
    }
}

#[test]
#[ignore = "candidate does not import another manager's restoration relation with ordinary update bytes"]
fn peer_restoration_relation_crosses_the_actual_import_boundary() {
    let a = LoroDoc::new_auto_commit();
    a.set_peer_id(1).unwrap();
    let b = LoroDoc::new_auto_commit();
    b.set_peer_id(2).unwrap();
    let l = Sequence::new(&a, false);
    let r = Sequence::new(&b, false);
    for (i, s) in ["(", "X", "B", ")"].iter().enumerate() {
        l.insert(i, s);
    }
    a.commit_then_renew();
    let local_undo = UndoManager::new_identity(&a);
    l.delete(1);
    a.commit_then_renew();
    l.delete(1);
    a.commit_then_renew();
    sync(&a, &b);
    let peer_undo = UndoManager::new_identity(&b);
    r.delete(1);
    r.delete(0);
    b.commit_then_renew();
    assert!(peer_undo.undo().unwrap());
    assert_eq!(r.source(), "()");
    sync(&b, &a);
    assert!(local_undo.undo().unwrap());
    assert_eq!(l.source(), "(B)");
    assert!(local_undo.undo().unwrap());
    assert_eq!(l.source(), "(XB)");
}

#[test]
fn list_provenance_chunks_preserve_partial_foreign_deletion() {
    let a = LoroDoc::new_auto_commit();
    a.set_peer_id(1).unwrap();
    let b = LoroDoc::new_auto_commit();
    b.set_peer_id(2).unwrap();
    let l = Sequence::new(&a, true);
    let r = Sequence::new(&b, true);
    for i in 0..1000 {
        l.insert(i, &format!("{i},"));
    }
    a.commit_then_renew();
    sync(&a, &b);
    let undo = UndoManager::new_identity(&a);
    for _ in 0..200 {
        l.delete(400);
    }
    for i in 400..600 {
        l.insert(i, "x,");
    }
    a.commit_then_renew();
    for _ in 0..100 {
        r.delete(450);
    }
    b.commit_then_renew();
    sync(&b, &a);
    let original: String = (0..1000)
        .filter(|i| !(450..550).contains(i))
        .map(|i| format!("{i},"))
        .collect();
    let replacement: String = (0..1000)
        .map(|i| {
            if (400..600).contains(&i) {
                "x,".to_owned()
            } else {
                format!("{i},")
            }
        })
        .collect();
    for _ in 0..2 {
        assert!(undo.undo().unwrap());
        assert_eq!(l.source(), original);
        assert!(undo.redo().unwrap());
        assert_eq!(l.source(), replacement);
    }
}
