//! Revived originals and undo-created aliases are distinct live elements.
use loro::{ExportMode, LoroDoc, ToJson, UndoManager};
use serde_json::json;

fn send(from: &LoroDoc, to: &LoroDoc, snapshot: bool) {
    let known = to.oplog_vv();
    let mode = if snapshot {
        ExportMode::Snapshot
    } else {
        ExportMode::updates(&known)
    };
    to.import(&from.export(mode).unwrap()).unwrap();
}

fn revived(peers: [u64; 2], value: &str, snapshot: bool) -> (LoroDoc, LoroDoc, UndoManager) {
    let a = LoroDoc::new();
    let b = LoroDoc::new();
    a.set_peer_id(peers[0]).unwrap();
    b.set_peer_id(peers[1]).unwrap();
    let list = a.get_movable_list("s");
    list.insert(0, value).unwrap();
    list.insert(1, "b").unwrap();
    a.commit();
    send(&a, &b, snapshot);
    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(0);
    list.delete(0, 1).unwrap();
    a.commit();
    b.get_movable_list("s").mov(0, 1).unwrap();
    b.commit();
    assert!(undo.undo().unwrap());
    send(&b, &a, snapshot);
    assert_eq!(list.get_value().to_json_value(), json!([value, "b", value]));
    (a, b, undo)
}

#[test]
fn new_edits_address_revived_original_without_touching_prior_alias() {
    for peers in [[1, 2], [2, 1]] {
        for value in ["a", "🦀"] {
            for snapshot in [false, true] {
                for change_peer in [false, true] {
                    let (a, b, mut undo) = revived(peers, value, snapshot);
                    let list = a.get_movable_list("s");
                    let alias = list.get_cursor(0, Default::default()).unwrap().id;
                    if change_peer {
                        a.set_peer_id(3).unwrap();
                    }
                    list.set(2, "changed").unwrap();
                    a.commit();
                    for _ in 0..3 {
                        assert!(undo.undo().unwrap());
                        assert_eq!(list.get_value().to_json_value(), json!([value, "b", value]));
                        assert_eq!(list.get_cursor(0, Default::default()).unwrap().id, alias);
                        assert!(undo.redo().unwrap());
                        assert_eq!(
                            list.get_value().to_json_value(),
                            json!([value, "b", "changed"])
                        );
                    }
                    // The original's later removal/restoration adds a second
                    // edge at the same root; it must not redirect the older alias.
                    list.delete(2, 1).unwrap();
                    a.commit();
                    for _ in 0..4 {
                        assert!(undo.undo().unwrap());
                        assert_eq!(
                            list.get_value().to_json_value(),
                            json!([value, "b", "changed"])
                        );
                        assert_eq!(list.get_cursor(0, Default::default()).unwrap().id, alias);
                        assert!(undo.redo().unwrap());
                        assert_eq!(list.get_value().to_json_value(), json!([value, "b"]));
                        assert_eq!(list.get_cursor(0, Default::default()).unwrap().id, alias);
                    }
                    assert!(undo.undo().unwrap());
                    // Older Set must now follow the new original's own branch.
                    assert!(undo.undo().unwrap());
                    assert_eq!(list.get_value().to_json_value(), json!([value, "b", value]));
                    assert_eq!(list.get_cursor(0, Default::default()).unwrap().id, alias);
                    send(&a, &b, snapshot);
                    send(&b, &a, snapshot);
                    assert_eq!(a.get_deep_value(), b.get_deep_value());
                    let cold = LoroDoc::new();
                    cold.import(&a.export(ExportMode::Snapshot).unwrap())
                        .unwrap();
                    assert_eq!(cold.get_deep_value(), a.get_deep_value());
                }
            }
        }
    }
}

#[test]
fn older_anchor_and_new_inverse_anchor_keep_their_own_identity_contexts() {
    for peers in [[1, 2], [2, 1]] {
        for values in [["a", "b"], ["a", "a"], ["🦀", "β"]] {
            for snapshot in [false, true] {
                for forked in [false, true] {
                    let a = LoroDoc::new();
                    let b = LoroDoc::new();
                    a.set_peer_id(peers[0]).unwrap();
                    b.set_peer_id(peers[1]).unwrap();
                    let list = a.get_movable_list("s");
                    list.insert(0, values[0]).unwrap();
                    list.insert(1, values[1]).unwrap();
                    a.commit();
                    send(&a, &b, snapshot);
                    let mut undo = UndoManager::new(&a);
                    undo.set_merge_interval(0);
                    list.mov(0, 1).unwrap();
                    a.commit();
                    list.delete(0, 1).unwrap();
                    a.commit();
                    assert!(undo.undo().unwrap());
                    let restored_anchor = list.get_cursor(0, Default::default()).unwrap().id;
                    b.get_movable_list("s").mov(1, 0).unwrap();
                    b.commit();
                    send(&b, &a, snapshot);
                    let expected = json!([values[1], values[1], values[0]]);
                    assert_eq!(list.get_value().to_json_value(), expected);
                    if forked {
                        let original = (0..2)
                            .find(|&i| {
                                list.get_cursor(i, Default::default()).unwrap().id
                                    != restored_anchor
                            })
                            .unwrap();
                        list.delete(original, 1).unwrap();
                        a.commit();
                        assert!(undo.undo().unwrap());
                        assert_eq!(list.get_value().to_json_value(), expected);
                    }
                    let first_anchor = list.get_cursor(0, Default::default()).unwrap().id;
                    let second_anchor = list.get_cursor(1, Default::default()).unwrap().id;
                    assert_ne!(first_anchor, second_anchor);
                    // Native tie order can put either equal-valued B first. The
                    // older move names the restoration of its historical B, not
                    // whichever B happens to occupy index one after import.
                    let anchor_position = (0..2)
                        .find(|&i| {
                            list.get_cursor(i, Default::default()).unwrap().id == restored_anchor
                        })
                        .unwrap();
                    let mut before_move = vec![values[1], values[1]];
                    before_move.insert(anchor_position, values[0]);
                    for _ in 0..4 {
                        assert!(undo.undo().unwrap());
                        assert_eq!(list.get_value().to_json_value(), json!(before_move));
                        assert!(undo.redo().unwrap());
                        assert_eq!(list.get_value().to_json_value(), expected);
                        assert_eq!(
                            list.get_cursor(0, Default::default()).unwrap().id,
                            first_anchor
                        );
                        assert_eq!(
                            list.get_cursor(1, Default::default()).unwrap().id,
                            second_anchor
                        );
                    }
                    send(&a, &b, snapshot);
                    assert_eq!(a.get_deep_value(), b.get_deep_value());
                }
            }
        }
    }
}
