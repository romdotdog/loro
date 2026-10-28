//! Positive controls: native inverse writes must not hide earlier style history.
use loro::{ExpandType, ExportMode, LoroDoc, StyleConfig, StyleConfigMap, ToJson, UndoManager};
use serde_json::{json, Value};

fn pair(peers: [u64; 2]) -> (LoroDoc, LoroDoc) {
    let a = LoroDoc::new();
    let b = LoroDoc::new();
    for (doc, peer) in [(&a, peers[0]), (&b, peers[1])] {
        doc.set_peer_id(peer).unwrap();
        let mut config = StyleConfigMap::default_rich_text_config();
        config.insert("color".into(), StyleConfig::new().expand(ExpandType::After));
        doc.config_text_style(config);
    }
    a.get_text("text").insert(0, "abcdef").unwrap();
    a.get_text("text").mark(0..6, "color", "green").unwrap();
    a.commit();
    sync(&a, &b);
    (a, b)
}

fn sync(from: &LoroDoc, to: &LoroDoc) {
    to.import(&from.export(ExportMode::updates(&to.oplog_vv())).unwrap())
        .unwrap();
}

fn cells(doc: &LoroDoc) -> Vec<Value> {
    let runs = doc.get_text("text").get_richtext_value().to_json_value();
    let mut out = Vec::new();
    for run in runs.as_array().unwrap() {
        for ch in run["insert"].as_str().unwrap().chars() {
            out.push(json!([
                ch.to_string(),
                run.get("attributes").cloned().unwrap_or(json!({}))
            ]));
        }
    }
    out
}

fn expected(color: &str, peer_color: bool, bold: bool) -> Vec<Value> {
    "abcdef"
        .chars()
        .enumerate()
        .map(|(i, ch)| {
            let mut attrs =
                json!({"color": if peer_color && (2..4).contains(&i) { "blue" } else { color }});
            if bold && (3..5).contains(&i) {
                attrs["bold"] = json!(true);
            }
            json!([ch.to_string(), attrs])
        })
        .collect()
}

#[test]
fn earlier_local_style_undo_accepts_own_inverse_writer() {
    let (a, _) = pair([1, 2]);
    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(0);
    for color in ["red", "yellow"] {
        a.get_text("text").mark(0..6, "color", color).unwrap();
        a.commit();
    }
    assert!(undo.undo().unwrap());
    assert_eq!(cells(&a), expected("red", false, false));
    assert!(undo.undo().unwrap());
    assert_eq!(cells(&a), expected("green", false, false));
    assert!(undo.redo().unwrap());
    assert_eq!(cells(&a), expected("red", false, false));
    assert!(undo.redo().unwrap());
    assert_eq!(cells(&a), expected("yellow", false, false));
}

#[test]
fn peer_style_undo_restores_the_earlier_local_command() {
    let (a, b) = pair([1, 2]);
    let mut ua = UndoManager::new(&a);
    ua.set_merge_interval(0);
    a.get_text("text").mark(0..6, "color", "red").unwrap();
    a.commit();
    sync(&a, &b);
    let mut ub = UndoManager::new(&b);
    b.get_text("text").mark(0..6, "color", "blue").unwrap();
    b.commit();
    assert!(ub.undo().unwrap());
    assert_eq!(cells(&b), expected("red", false, false));
    sync(&b, &a);
    assert_eq!(cells(&a), expected("red", false, false));
    assert!(ua.undo().unwrap());
    assert_eq!(cells(&a), expected("green", false, false));
}
