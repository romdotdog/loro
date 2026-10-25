//! Release-compiled native identity candidate: no cfg(test) planner or TLS scope.
use loro_internal::{cursor::PosType, loro::ExportMode, LoroDoc, UndoManager};
fn tracked_doc() -> LoroDoc {
    let doc = LoroDoc::new_auto_commit();
    doc.enable_identity_history().unwrap();
    doc
}
fn sync(a: &LoroDoc, b: &LoroDoc) {
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
}
#[test]
fn mixed_replacements_and_manager_owned_links_cycle() {
    for peer in [1, 2] {
        for cell in ["a", "🦀"] {
            let a = tracked_doc();
            a.set_peer_id(peer).unwrap();
            let text = a.get_text("text");
            text.insert(0, &format!("{cell}B"), PosType::Unicode)
                .unwrap();
            a.commit_then_renew();
            let undo = UndoManager::new_identity(&a).unwrap();
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
            let a = tracked_doc();
            a.set_peer_id(peer).unwrap();
            let b = tracked_doc();
            b.set_peer_id(3 - peer).unwrap();
            let text = a.get_text("text");
            text.insert(0, "ab", PosType::Unicode).unwrap();
            a.commit_then_renew();
            sync(&a, &b);
            let undo = UndoManager::new_identity(&a).unwrap();
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
        let a = tracked_doc();
        a.set_peer_id(peer).unwrap();
        let b = tracked_doc();
        b.set_peer_id(3 - peer).unwrap();
        let t = a.get_text("text");
        t.insert(0, "aB", PosType::Unicode).unwrap();
        a.commit_then_renew();
        sync(&a, &b);
        let undo = UndoManager::new_identity(&a).unwrap();
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
    let a = tracked_doc();
    a.set_peer_id(1).unwrap();
    let b = tracked_doc();
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
    let undo = UndoManager::new_identity(&a).unwrap();
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
    let recovered = tracked_doc();
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
            let a = tracked_doc();
            a.set_peer_id(peer).unwrap();
            let b = tracked_doc();
            b.set_peer_id(3 - peer).unwrap();
            let l = Sequence::new(&a, true);
            let r = Sequence::new(&b, true);
            l.insert(0, "a");
            l.insert(1, "b");
            a.commit_then_renew();
            sync(&a, &b);
            let undo = UndoManager::new_identity(&a).unwrap();
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
            let a = tracked_doc();
            a.set_peer_id(peer).unwrap();
            let b = tracked_doc();
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
            let undo = UndoManager::new_identity(&a).unwrap();
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
        let a = tracked_doc();
        a.set_peer_id(peer).unwrap();
        let b = tracked_doc();
        b.set_peer_id(3 - peer).unwrap();
        let l = Sequence::new(&a, true);
        let r = Sequence::new(&b, true);
        l.insert(0, "a");
        l.insert(1, "B");
        a.commit_then_renew();
        sync(&a, &b);
        let undo = UndoManager::new_identity(&a).unwrap();
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
        let a = tracked_doc();
        a.set_peer_id(1).unwrap();
        let b = tracked_doc();
        b.set_peer_id(2).unwrap();
        let l = Sequence::new(&a, list);
        let r = Sequence::new(&b, list);
        let undo = UndoManager::new_identity(&a).unwrap();
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
    let doc = tracked_doc();
    doc.set_peer_id(1).unwrap();
    let text = doc.get_text("text");
    text.insert(0, &"a".repeat(4096), PosType::Unicode).unwrap();
    doc.commit_then_renew();
    let undo = UndoManager::new_identity(&doc).unwrap();
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
            let a = tracked_doc();
            a.set_peer_id(peer).unwrap();
            let b = tracked_doc();
            b.set_peer_id(3 - peer).unwrap();
            let l = Sequence::new(&a, list);
            let r = Sequence::new(&b, list);
            let undo = UndoManager::new_identity(&a).unwrap();
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
            let a = tracked_doc();
            a.set_peer_id(peer).unwrap();
            let b = tracked_doc();
            b.set_peer_id(3 - peer).unwrap();
            let l = Sequence::new(&a, list);
            let r = Sequence::new(&b, list);
            l.insert(0, "a");
            l.insert(1, "B");
            a.commit_then_renew();
            let undo = UndoManager::new_identity(&a).unwrap();
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
            let a = tracked_doc();
            a.set_peer_id(peer).unwrap();
            let b = tracked_doc();
            b.set_peer_id(3 - peer).unwrap();
            let l = Sequence::new(&a, list);
            let r = Sequence::new(&b, list);
            for (i, s) in ["a", "b", "c", "d", "e", "f"].iter().enumerate() {
                l.insert(i, s);
            }
            a.commit_then_renew();
            sync(&a, &b);
            let undo = UndoManager::new_identity(&a).unwrap();
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
fn peer_restoration_relation_crosses_the_actual_import_boundary() {
    for list in [false, true] {
        for peer in [1, 2] {
            for cell in ["X", "🦀"] {
                for mode in 0..6 {
                    recovered_boundary(list, peer, cell, mode);
                }
            }
        }
    }
}
fn recovered_boundary(list: bool, peer: u64, cell: &str, mode: usize) {
    let a = tracked_doc();
    a.set_peer_id(peer).unwrap();
    let b = tracked_doc();
    b.set_peer_id(3 - peer).unwrap();
    let l = Sequence::new(&a, list);
    let r = Sequence::new(&b, list);
    for (i, s) in ["(", cell, "B", ")"].iter().enumerate() {
        l.insert(i, s);
    }
    a.commit_then_renew();
    let local_undo = UndoManager::new_identity(&a).unwrap();
    l.delete(1);
    a.commit_then_renew();
    l.delete(1);
    a.commit_then_renew();
    sync(&a, &b);
    let peer_undo = UndoManager::new_identity(&b).unwrap();
    peer_undo.set_merge_interval(0);
    let mut since = b.oplog_vv();
    let mut packets = Vec::new();
    r.delete(1);
    r.delete(0);
    b.commit_then_renew();
    for cycle in 0..3 {
        if cycle != 0 {
            assert!(peer_undo.redo().unwrap());
        }
        assert!(peer_undo.undo().unwrap());
        assert_eq!(r.source(), "()");
        let packet = b.export(ExportMode::updates(&since)).unwrap();
        since = b.oplog_vv();
        if mode == 1 {
            a.import(&packet).unwrap();
        }
        packets.push(packet);
    }
    match mode {
        0 => sync(&b, &a),
        1 => {}
        2 => {
            a.import(&b.export(ExportMode::Snapshot).unwrap()).unwrap();
        }
        3 => {
            a.import_json_updates(
                b.export_json_updates(&Default::default(), &b.oplog_vv(), false)
                    .unwrap(),
            )
            .unwrap();
        }
        4 => {
            a.import_batch(&packets).unwrap();
        }
        5 => {
            // Cut within the final two-cell restoration change, not merely at
            // a commit boundary. Its message still covers the original range.
            let mut prefix = b.oplog_vv();
            let end = prefix.get(&(3 - peer)).copied().unwrap();
            prefix.set_end(loro_common::ID::new(3 - peer, end - 1));
            a.import(&b.export(ExportMode::updates_till(&prefix)).unwrap())
                .unwrap();
            assert_eq!(l.source(), "(");
            a.import(&b.export(ExportMode::updates(&prefix)).unwrap())
                .unwrap();
        }
        _ => unreachable!(),
    }
    // Duplicate delivery is idempotent. Metadata is in actual exported changes.
    sync(&b, &a);
    for cycle in 0..3 {
        assert!(
            local_undo.undo().unwrap(),
            "list={list} peer={peer} cell={cell} mode={mode} cycle={cycle} source={}",
            l.source()
        );
        assert_eq!(l.source(), "(B)");
        assert!(local_undo.undo().unwrap());
        assert_eq!(l.source(), format!("({cell}B)"));
        assert!(local_undo.redo().unwrap());
        assert_eq!(l.source(), "(B)");
        assert!(local_undo.redo().unwrap());
        assert_eq!(l.source(), "()");
    }
    // Recover a fresh document and manager, not the old manager's memory.
    let cold = if mode % 2 == 0 {
        tracked_doc()
    } else {
        LoroDoc::new_auto_commit()
    };
    cold.set_peer_id(99).unwrap();
    cold.import(&a.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    let c = Sequence::new(&cold, list);
    let fresh = UndoManager::new_identity(&cold).unwrap();
    c.delete(0);
    cold.commit_then_renew();
    assert!(fresh.undo().unwrap());
    assert_eq!(c.source(), "()");
    assert!(fresh.redo().unwrap());
    assert_eq!(c.source(), ")");
}

#[test]
fn list_provenance_chunks_preserve_partial_foreign_deletion() {
    let a = tracked_doc();
    a.set_peer_id(1).unwrap();
    let b = tracked_doc();
    b.set_peer_id(2).unwrap();
    let l = Sequence::new(&a, true);
    let r = Sequence::new(&b, true);
    for i in 0..1000 {
        l.insert(i, &format!("{i},"));
    }
    a.commit_then_renew();
    sync(&a, &b);
    let undo = UndoManager::new_identity(&a).unwrap();
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

#[test]
fn missing_dependencies_reject_before_import_then_full_resync_succeeds() {
    let a = tracked_doc();
    a.set_peer_id(1).unwrap();
    let t = a.get_text("text");
    t.insert(0, "a", PosType::Unicode).unwrap();
    a.commit_then_renew();
    let first = a.oplog_vv();
    t.insert(1, "b", PosType::Unicode).unwrap();
    a.commit_then_renew();
    let suffix = a.export(ExportMode::updates(&first)).unwrap();
    let b = tracked_doc();
    let undo = UndoManager::new_identity(&b).unwrap();
    let before = b.oplog_vv();
    assert!(b
        .import(&suffix)
        .unwrap_err()
        .to_string()
        .contains("missing dependencies"));
    assert_eq!(b.oplog_vv(), before);
    assert!(b.get_text("text").to_string().is_empty());
    assert_eq!((undo.undo_count(), undo.redo_count()), (0, 0));
    sync(&a, &b);
    assert_eq!(b.get_text("text").to_string(), "ab");
}

#[test]
fn untagged_fresh_peer_is_rejected_instead_of_guessed_and_cannot_be_retagged_afterwards() {
    let a = tracked_doc();
    let b = LoroDoc::new_auto_commit();
    b.set_peer_id(2).unwrap();
    b.get_text("text")
        .insert(0, "fresh", PosType::Unicode)
        .unwrap();
    b.commit_then_renew();
    let unknown = b.export(ExportMode::all_updates()).unwrap();
    let before = a.oplog_vv();
    assert!(a
        .import(&unknown)
        .unwrap_err()
        .to_string()
        .contains("untagged"));
    assert_eq!(a.oplog_vv(), before);
    // Enabling production cannot retroactively certify already-created history.
    assert!(b.enable_identity_history().is_err());
    assert!(a
        .import(&b.export(ExportMode::all_updates()).unwrap())
        .is_err());
}

#[test]
fn strict_mode_survives_manager_drop_and_refuses_ordinary_inverse_generation() {
    let doc = tracked_doc();
    let text = doc.get_text("text");
    text.insert(0, "ab", PosType::Unicode).unwrap();
    doc.commit_then_renew();
    let identity = UndoManager::new_identity(&doc).unwrap();
    drop(identity);
    let ordinary = UndoManager::new(&doc);
    text.delete(1, 1, PosType::Unicode).unwrap();
    doc.commit_then_renew();
    let vv = doc.oplog_vv();
    assert!(ordinary
        .undo()
        .unwrap_err()
        .to_string()
        .contains("ordinary undo"));
    assert_eq!(doc.oplog_vv(), vv);
    assert_eq!(text.to_string(), "a");
    assert_eq!((ordinary.undo_count(), ordinary.redo_count()), (1, 0));
    let fork = doc.fork();
    fork.get_text("text")
        .insert(1, "c", PosType::Unicode)
        .unwrap();
    fork.commit_then_renew();
    doc.import(&fork.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(text.to_string(), "ac");
}

#[test]
fn constructor_rejects_untagged_history_without_enabling_strict_mode() {
    let doc = LoroDoc::new_auto_commit();
    let text = doc.get_text("text");
    text.insert(0, "ab", PosType::Unicode).unwrap();
    doc.commit_then_renew();
    let ordinary = UndoManager::new(&doc);
    assert!(UndoManager::new_identity(&doc).is_err());
    text.delete(1, 1, PosType::Unicode).unwrap();
    doc.commit_then_renew();
    assert!(ordinary.undo().unwrap());
    assert_eq!(text.to_string(), "ab");
}
