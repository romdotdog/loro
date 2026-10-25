//! Peer changes clear old items, but an open manual group starts afresh at the
//! new peer's counter rather than comparing counters from different peers.
use loro::{LoroDoc, UndoManager};

#[test]
fn manual_group_survives_peer_change_as_a_fresh_new_peer_group() {
    for seed_len in [0, 1, 4] {
        for old_group_len in [0, 2] {
            let doc = LoroDoc::new();
            let text = doc.get_text("text");
            doc.set_peer_id(2).unwrap();
            let seed = "q".repeat(seed_len);
            text.insert(0, &seed).unwrap();
            doc.commit();
            doc.set_peer_id(1).unwrap();
            text.insert(seed_len, "x").unwrap();
            doc.commit();
            let mut undo = UndoManager::new(&doc);
            undo.set_merge_interval(0);
            undo.group_start().unwrap();
            let old = "g".repeat(old_group_len);
            text.insert(seed_len + 1, &old).unwrap();
            doc.commit();
            doc.set_peer_id(2).unwrap();
            assert_eq!((undo.undo_count(), undo.redo_count()), (0, 0));
            let base = format!("{seed}x{old}");
            text.insert(base.len(), "a").unwrap();
            doc.commit();
            text.insert(base.len() + 1, "b").unwrap();
            doc.commit();
            undo.group_end();
            assert_eq!(undo.undo_count(), 1, "seed={seed_len}, old={old_group_len}");
            for _ in 0..3 {
                assert!(undo.undo().unwrap());
                assert_eq!(text.to_string(), base);
                assert_eq!((undo.undo_count(), undo.redo_count()), (0, 1));
                assert!(undo.redo().unwrap());
                assert_eq!(text.to_string(), format!("{base}ab"));
                assert_eq!((undo.undo_count(), undo.redo_count()), (1, 0));
            }
        }
    }
}

#[test]
fn repeated_peer_changes_do_not_join_cleared_items_or_create_a_group() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    doc.set_peer_id(1).unwrap();
    let mut undo = UndoManager::new(&doc);
    undo.set_merge_interval(0);
    for grouped in [false, true] {
        if grouped {
            undo.group_start().unwrap();
        }
        for peer in [2, 3, 1] {
            doc.set_peer_id(peer).unwrap();
            assert!(!undo.can_undo());
            text.insert(text.len_unicode(), "a").unwrap();
            doc.commit();
            text.insert(text.len_unicode(), "b").unwrap();
            doc.commit();
            assert_eq!(undo.undo_count(), if grouped { 1 } else { 2 });
        }
        undo.group_end();
    }
}

#[test]
fn peer_change_discards_the_cleared_groups_affected_containers() {
    use loro::ExportMode;
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let old_text = doc.get_text("old");
    let new_text = doc.get_text("new");
    let mut undo = UndoManager::new(&doc);
    undo.set_merge_interval(0);
    undo.group_start().unwrap();
    old_text.insert(0, "g").unwrap();
    doc.commit();
    let other = LoroDoc::new();
    other.set_peer_id(2).unwrap();
    other
        .import(&doc.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    doc.set_peer_id(3).unwrap();
    other.get_text("old").insert(1, "R").unwrap();
    other.commit();
    // This import touches only the old peer's cleared group. It must not end
    // the still-empty new-peer group on the strength of that stale membership.
    doc.import(&other.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    new_text.insert(0, "a").unwrap();
    doc.commit();
    new_text.insert(1, "b").unwrap();
    doc.commit();
    undo.group_end();
    assert_eq!(undo.undo_count(), 1);
    assert!(undo.undo().unwrap());
    assert_eq!(new_text.to_string(), "");
    assert_eq!(old_text.to_string(), "gR");
    assert!(!undo.can_undo());
    assert!(undo.redo().unwrap());
    assert_eq!(new_text.to_string(), "ab");
    assert_eq!(old_text.to_string(), "gR");
}
