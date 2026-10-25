//! Direct Rust translations of h's posted JS repros. Assertions are desired
//! outcomes, never assertions that the currently observed bug is acceptable.
use loro::{ExportMode, LoroDoc, ToJson, UndoManager};
use serde_json::json;

fn sync(from: &LoroDoc, to: &LoroDoc) {
    to.import(&from.export(ExportMode::Snapshot).unwrap())
        .unwrap();
}

#[test]
fn issue_1231_peer_change_inside_manual_group() {
    for change_peer in [false, true] {
        let doc = LoroDoc::new();
        doc.set_peer_id(1).unwrap();
        let text = doc.get_text("text");
        text.insert(0, "x").unwrap();
        doc.commit();
        let mut undo = UndoManager::new(&doc);
        undo.set_merge_interval(0);
        undo.group_start().unwrap();
        if change_peer {
            doc.set_peer_id(2).unwrap();
        }
        text.insert(1, "a").unwrap();
        doc.commit();
        text.insert(2, "b").unwrap();
        doc.commit();
        undo.group_end();
        undo.undo().unwrap();
        assert_eq!(text.to_string(), "x", "change_peer={change_peer}");
        assert!(!undo.can_undo());
    }
}

#[test]
fn issue_1232_redo_obeys_lowered_undo_limit() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    let mut undo = UndoManager::new(&doc);
    undo.set_merge_interval(0);
    text.insert(0, "a").unwrap();
    doc.commit();
    text.insert(1, "b").unwrap();
    doc.commit();
    undo.undo().unwrap();
    undo.set_max_undo_steps(1);
    undo.redo().unwrap();
    undo.undo().unwrap();
    assert_eq!(text.to_string(), "a");
    assert!(!undo.can_undo());
}

#[test]
fn issue_1233_post_import_item_resets_merge_timer() {
    use std::time::{Duration, Instant};
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let remote = LoroDoc::new();
    remote.set_peer_id(2).unwrap();
    let text = doc.get_text("text");
    text.insert(0, "ab").unwrap();
    doc.commit();
    let mut undo = UndoManager::new(&doc);
    undo.set_merge_interval(500);
    text.insert(1, "X").unwrap();
    doc.commit();
    let first = Instant::now();
    sync(&doc, &remote);
    remote.get_text("text").delete(1, 1).unwrap();
    remote.commit();
    sync(&remote, &doc);
    std::thread::sleep(Duration::from_millis(300));
    text.delete(0, 1).unwrap();
    doc.commit();
    let second = Instant::now();
    std::thread::sleep(Duration::from_millis(300));
    let since_first = first.elapsed();
    let since_second = second.elapsed();
    text.delete(0, 1).unwrap();
    doc.commit();
    assert!(
        second.duration_since(first).as_millis() < 500
            && since_first.as_millis() > 500
            && since_second.as_millis() < 500,
        "timing window missed: inconclusive, retry; first={since_first:?} second={since_second:?}"
    );
    assert_eq!(text.to_string(), "");
    assert!(undo.undo().unwrap());
    assert_eq!(text.to_string(), "ab");
}

fn movable_pair() -> (LoroDoc, LoroDoc, UndoManager) {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let remote = LoroDoc::new();
    remote.set_peer_id(2).unwrap();
    let list = doc.get_movable_list("sequence");
    list.insert(0, "a").unwrap();
    list.insert(1, "b").unwrap();
    doc.commit();
    sync(&doc, &remote);
    let mut undo = UndoManager::new(&doc);
    undo.set_merge_interval(0);
    (doc, remote, undo)
}

#[test]
#[ignore = "upstream #1234 desired outcome still fails; run with --include-ignored"]
fn issue_1234_delete_revived_original() {
    let (doc, remote, mut undo) = movable_pair();
    let list = doc.get_movable_list("sequence");
    list.delete(0, 1).unwrap();
    doc.commit();
    remote.get_movable_list("sequence").mov(0, 1).unwrap();
    remote.commit();
    assert!(undo.undo().unwrap());
    assert_eq!(list.get_value().to_json_value(), json!(["a", "b"]));
    sync(&remote, &doc);
    assert_eq!(list.get_value().to_json_value(), json!(["a", "b", "a"]));
    list.delete(2, 1).unwrap();
    doc.commit();
    assert_eq!(list.get_value().to_json_value(), json!(["a", "b"]));
    assert!(undo.can_undo());
    assert!(undo.undo().unwrap());
    assert_eq!(list.get_value().to_json_value(), json!(["a", "b", "a"]));
    assert!(!undo.can_undo());
}

#[test]
#[ignore = "upstream #1234 desired outcome still fails; run with --include-ignored"]
fn issue_1234_comment_roundtrip_uses_original_anchor() {
    let (doc, remote, mut undo) = movable_pair();
    let list = doc.get_movable_list("sequence");
    list.mov(0, 1).unwrap();
    doc.commit();
    list.delete(0, 1).unwrap();
    doc.commit();
    assert!(undo.undo().unwrap());
    remote.get_movable_list("sequence").mov(1, 0).unwrap();
    remote.commit();
    sync(&remote, &doc);
    let before = list.get_value().to_json_value();
    assert_eq!(before, json!(["b", "b", "a"]));
    assert!(undo.undo().unwrap());
    assert!(undo.redo().unwrap());
    assert_eq!(list.get_value().to_json_value(), before);
}

fn sync_updates(from: &LoroDoc, to: &LoroDoc) {
    to.import(&from.export(ExportMode::updates(&to.oplog_vv())).unwrap())
        .unwrap();
}

fn formatting_pair() -> (LoroDoc, LoroDoc) {
    use loro::{ExpandType, StyleConfig, StyleConfigMap};
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    for doc in [&a, &b] {
        let mut styles = StyleConfigMap::default_rich_text_config();
        styles.insert("color".into(), StyleConfig::new().expand(ExpandType::After));
        doc.config_text_style(styles);
    }
    a.get_text("text").insert(0, "x").unwrap();
    a.commit();
    sync_updates(&a, &b);
    (a, b)
}

#[test]
#[ignore = "upstream #1229 desired outcome still fails; run with --include-ignored"]
fn issue_1229_undo_preserves_peer_color() {
    let (a, b) = formatting_pair();
    let text = a.get_text("text");
    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(0);
    text.mark(0..1, "color", "red").unwrap();
    a.commit();
    sync_updates(&a, &b);
    b.get_text("text").mark(0..1, "color", "blue").unwrap();
    b.commit();
    sync_updates(&b, &a);
    let expected = json!([{"insert":"x", "attributes":{"color":"blue"}}]);
    assert_eq!(text.get_richtext_value().to_json_value(), expected);
    undo.undo().unwrap();
    assert_eq!(text.get_richtext_value().to_json_value(), expected);
}

/// Issue 1229 case 2 is a prose schedule, not a posted second JS program.
#[test]
#[ignore = "upstream #1229 desired outcome still fails; run with --include-ignored"]
fn issue_1229_redo_preserves_peer_unmark() {
    let (a, b) = formatting_pair();
    let text = a.get_text("text");
    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(0);
    text.mark(0..1, "color", "red").unwrap();
    a.commit();
    sync_updates(&a, &b);
    b.get_text("text").unmark(0..1, "color").unwrap();
    b.commit();
    undo.undo().unwrap();
    let expected = json!([{"insert":"x"}]);
    assert_eq!(text.get_richtext_value().to_json_value(), expected);
    sync_updates(&b, &a);
    undo.redo().unwrap();
    assert_eq!(text.get_richtext_value().to_json_value(), expected);
}

#[test]
fn issue_1228_manual_group_after_dependent_map_import() {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    let text = a.get_text("text");
    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(0);
    undo.group_start().unwrap();
    text.insert(0, "x").unwrap();
    a.commit();
    // The posted repro uses incremental updates here and below.
    b.import(&a.export(ExportMode::updates(&b.oplog_vv())).unwrap())
        .unwrap();
    b.get_map("metadata").insert("flag", true).unwrap();
    b.commit();
    text.delete(0, 1).unwrap();
    text.insert(0, "y").unwrap();
    a.commit();
    undo.group_end();
    a.import(&b.export(ExportMode::updates(&a.oplog_vv())).unwrap())
        .unwrap();
    assert_eq!(text.to_string(), "y");
    undo.undo().unwrap();
    assert_eq!(text.to_string(), "");
    assert!(!undo.can_undo());
    assert_eq!(
        a.get_map("metadata").get_value().to_json_value()["flag"],
        true
    );
    undo.redo().unwrap();
    assert_eq!(text.to_string(), "y");
}
