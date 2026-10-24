use loro::{ExportMode, LoroDoc, UndoManager};
fn sync(a: &LoroDoc, b: &LoroDoc) {
    b.import(&a.export(ExportMode::updates(&b.oplog_vv())).unwrap())
        .unwrap();
}
fn replace(a: &LoroDoc, s: &str, split: bool) {
    let t = a.get_text("text");
    let n = t.len_unicode();
    if n > 0 {
        t.delete(0, n).unwrap();
        if split {
            a.commit();
        }
    }
    if !s.is_empty() {
        t.insert(0, s).unwrap();
    }
    a.commit();
}
#[test]
fn causal_group_matrix() {
    for seq in [
        vec!["x", "y"],
        vec!["x", "", "y"],
        vec!["x", "xy", "z"],
        vec!["x", ""],
    ] {
        for mode in [
            "none",
            "map-mid",
            "map-final",
            "independent-z",
            "append-z",
            "delete-x",
            "net-zero-z",
        ] {
            for split in [false, true] {
                let a = LoroDoc::new();
                let b = LoroDoc::new();
                a.set_peer_id(1).unwrap();
                b.set_peer_id(2).unwrap();
                let t = a.get_text("text");
                let mut u = UndoManager::new(&a);
                u.set_merge_interval(0);
                u.group_start().unwrap();
                replace(&a, seq[0], split);
                if ["map-mid", "append-z", "delete-x", "net-zero-z"].contains(&mode) {
                    sync(&a, &b);
                }
                match mode {
                    "map-mid" => {
                        b.get_map("meta").insert("flag", true).unwrap();
                        b.commit();
                    }
                    "independent-z" | "append-z" => {
                        let q = b.get_text("text");
                        q.insert(q.len_unicode(), "R").unwrap();
                        b.commit();
                    }
                    "delete-x" => {
                        b.get_text("text").delete(0, 1).unwrap();
                        b.commit();
                    }
                    "net-zero-z" => {
                        let q = b.get_text("text");
                        q.insert(1, "R").unwrap();
                        b.commit();
                        q.delete(1, 1).unwrap();
                        b.commit();
                    }
                    _ => {}
                }
                for s in &seq[1..] {
                    replace(&a, s, split);
                }
                u.group_end();
                if mode == "map-final" {
                    sync(&a, &b);
                    b.get_map("meta").insert("flag", true).unwrap();
                    b.commit();
                }
                if mode != "none" {
                    sync(&b, &a);
                }
                let before = t.to_string();
                let did = u.undo().unwrap();
                let expected = if mode == "independent-z" || mode == "append-z" {
                    "R"
                } else {
                    ""
                };
                assert_eq!(t.to_string(), expected, "{seq:?}/{mode}/split={split}");
                assert!(!u.can_undo(), "one group exhausted");
                if mode.starts_with("map-") {
                    assert!(matches!(
                        a.get_map("meta").get("flag"),
                        Some(loro::ValueOrContainer::Value(loro::LoroValue::Bool(true)))
                    ));
                }
                if did {
                    assert!(u.redo().unwrap());
                    assert_eq!(t.to_string(), before, "redo {seq:?}/{mode}/split={split}");
                } else {
                    assert!(!u.can_redo());
                }
            }
        }
    }
}
#[test]
fn several_late_breakpoints() {
    let a = LoroDoc::new();
    let b = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    b.set_peer_id(2).unwrap();
    let t = a.get_text("text");
    let mut u = UndoManager::new(&a);
    u.group_start().unwrap();
    for (i, s) in ["x", "xy", "xyz", "final"].iter().enumerate() {
        replace(&a, s, false);
        sync(&a, &b);
        b.get_map("meta").insert(&format!("seen{i}"), true).unwrap();
        b.commit();
    }
    u.group_end();
    sync(&b, &a);
    assert!(u.undo().unwrap());
    assert_eq!(t.to_string(), "");
    assert!(!u.can_undo());
    assert_eq!(a.get_map("meta").len(), 4);
    assert!(u.redo().unwrap());
    assert_eq!(t.to_string(), "final");
}
#[test]
fn imported_change_between_group_spans() {
    let a = LoroDoc::new();
    let b = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    b.set_peer_id(2).unwrap();
    let t = a.get_text("text");
    let mut u = UndoManager::new(&a);
    u.group_start().unwrap();
    replace(&a, "x", false);
    sync(&a, &b);
    b.get_map("meta").insert("flag", true).unwrap();
    b.commit();
    sync(&b, &a);
    replace(&a, "y", false);
    u.group_end();
    assert!(u.undo().unwrap());
    assert_eq!(t.to_string(), "");
    assert!(u.redo().unwrap());
    assert_eq!(t.to_string(), "y");
}
