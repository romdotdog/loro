//! Identity protocol validation must include the history a receiver already has.
#![cfg(feature = "experimental-identity-undo")]
use loro::{ExportMode, LoroDoc, UndoManager, VersionVector};

#[test]
fn duplicate_identity_metadata_is_validated_before_known_history_is_trimmed() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let mut undo = UndoManager::new_identity(&doc).unwrap();
    doc.get_text("text").insert(0, "Q").unwrap();
    doc.commit();
    let good = doc
        .export_json_updates(&VersionVector::default(), &doc.oplog_vv())
        .unwrap();
    let mut bad = good.clone();
    bad.changes[0].msg = None;
    let bytes = doc.export(ExportMode::Snapshot).unwrap();
    let vv = doc.oplog_vv();
    let stacks = (undo.undo_count(), undo.redo_count());
    assert!(
        doc.import_json_updates(bad).is_err(),
        "fully known untagged metadata must reject"
    );
    assert_eq!(doc.export(ExportMode::Snapshot).unwrap(), bytes);
    assert_eq!(doc.oplog_vv(), vv);
    assert_eq!((undo.undo_count(), undo.redo_count()), stacks);
    doc.import_json_updates(good).unwrap();
    assert!(undo.undo().unwrap());
    assert_eq!(doc.get_text("text").to_string(), "");
    assert!(undo.redo().unwrap());
    assert_eq!(doc.get_text("text").to_string(), "Q");
}

#[test]
fn known_identity_records_reject_bad_and_conflicting_metadata_across_import_paths() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    const PREFIX: &str = "loro:identity-undo:1\n";
    for list in [false, true] {
        let a = LoroDoc::new();
        a.set_peer_id(11).unwrap();
        let mut undo = UndoManager::new_identity(&a).unwrap();
        undo.set_merge_interval(0);
        if list {
            a.get_list("sequence").insert(0, "Q").unwrap();
            a.get_list("sequence").insert(1, "Q").unwrap();
        } else {
            a.get_text("sequence").insert(0, "QQ").unwrap();
        }
        a.commit();
        if list {
            a.get_list("sequence").delete(0, 2).unwrap();
        } else {
            a.get_text("sequence").delete(0, 2).unwrap();
        }
        a.commit();
        assert!(undo.undo().unwrap());
        let good = a
            .export_json_updates(&VersionVector::default(), &a.oplog_vv())
            .unwrap();
        let restored = good
            .changes
            .iter()
            .position(|c| {
                c.msg
                    .as_ref()
                    .is_some_and(|s| s.contains("\"inverse\":true") && !s.contains("\"edges\":[]"))
            })
            .unwrap();
        let record: serde_json::Value = serde_json::from_str(
            good.changes[restored]
                .msg
                .as_ref()
                .unwrap()
                .strip_prefix(PREFIX)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(record["edges"].as_array().unwrap().len(), 2);
        let b = LoroDoc::new();
        b.set_peer_id(22).unwrap();
        b.enable_identity_history().unwrap();
        b.import(&a.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        if list {
            b.get_list("sequence").insert(0, "R").unwrap();
        } else {
            b.get_text("sequence").insert(0, "R").unwrap();
        }
        b.commit();
        let extended = b
            .export_json_updates(&VersionVector::default(), &b.oplog_vv())
            .unwrap();
        let valid_prefix = b.export(ExportMode::all_updates()).unwrap();
        let before = a.get_deep_value();
        let bytes = a.export(ExportMode::Snapshot).unwrap();
        let vv = a.oplog_vv();
        let frontiers = a.state_frontiers();
        let stacks = (undo.undo_count(), undo.redo_count());
        let events = Arc::new(AtomicUsize::new(0));
        let count = events.clone();
        let _sub = a.subscribe_root(Arc::new(move |_| {
            count.fetch_add(1, Ordering::Relaxed);
        }));
        for with_suffix in [false, true] {
            for mutation in 0..7 {
                let mut bad = if with_suffix {
                    extended.clone()
                } else {
                    good.clone()
                };
                let index = bad
                    .changes
                    .iter()
                    .position(|c| c.id == good.changes[restored].id)
                    .unwrap();
                let mut changed = record.clone();
                match mutation {
                    0 => bad.changes[index].msg = None,
                    1 => bad.changes[index].msg = Some(format!("{PREFIX}{{")),
                    2 => bad.changes[index].msg = Some(format!("loro:identity-undo:99\n{changed}")),
                    3 => {
                        changed["inverse"] = false.into();
                        changed["edges"] = serde_json::json!([]);
                    }
                    4 => {
                        changed["edges"][0][3] = changed["edges"][1][3].clone();
                    }
                    5 => {
                        changed["end"] = (changed["end"].as_i64().unwrap() + 1).into();
                    }
                    6 => {
                        changed["edges"] = serde_json::json!([]);
                    }
                    _ => unreachable!(),
                }
                if mutation >= 3 {
                    bad.changes[index].msg = Some(format!("{PREFIX}{changed}"));
                }
                let relay = LoroDoc::new();
                relay.import_json_updates(bad.clone()).unwrap();
                let update = relay.export(ExportMode::all_updates()).unwrap();
                let snapshot = relay.export(ExportMode::Snapshot).unwrap();
                for path in 0..4 {
                    let result = match path {
                        0 => a.import_json_updates(bad.clone()),
                        1 => a.import(&update),
                        2 => a.import(&snapshot),
                        3 => a.import_batch(&[valid_prefix.clone(), update.clone()]),
                        _ => unreachable!(),
                    };
                    assert!(
                        result.is_err(),
                        "list={list}, suffix={with_suffix}, mutation={mutation}, path={path}"
                    );
                    assert_eq!(a.get_deep_value(), before);
                    assert_eq!(a.oplog_vv(), vv);
                    assert_eq!(a.state_frontiers(), frontiers);
                    assert_eq!(a.export(ExportMode::Snapshot).unwrap(), bytes);
                    assert_eq!((undo.undo_count(), undo.redo_count()), stacks);
                    assert_eq!(events.load(Ordering::Relaxed), 0);
                }
            }
        }
        // Identity metadata is semantic, not JSON formatting or user annotation.
        let mut equivalent = good.clone();
        let mut reordered = record.clone();
        reordered["edges"].as_array_mut().unwrap().reverse();
        reordered["message"] = "different user annotation".into();
        equivalent.changes[restored].msg = Some(format!(
            "{PREFIX}{}",
            serde_json::to_string_pretty(&reordered).unwrap()
        ));
        a.import_json_updates(equivalent).unwrap();
        a.import_json_updates(good).unwrap();
        assert_eq!(a.export(ExportMode::Snapshot).unwrap(), bytes);
        assert!(undo.redo().unwrap());
        assert!(undo.undo().unwrap());
        assert_eq!(a.get_deep_value(), before);
    }
}

#[test]
fn ordinary_known_history_message_handling_is_unchanged() {
    let doc = LoroDoc::new();
    doc.get_text("text").insert(0, "Q").unwrap();
    doc.commit();
    let mut json = doc
        .export_json_updates(&VersionVector::default(), &doc.oplog_vv())
        .unwrap();
    json.changes[0].msg = Some("loro:identity-undo:1\n{".into());
    doc.import_json_updates(json).unwrap();
    assert_eq!(doc.get_text("text").to_string(), "Q");
}

#[test]
fn rejected_known_metadata_does_not_undo_the_pending_edit_commit_barrier() {
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new_identity(&doc).unwrap();
    undo.set_merge_interval(0);
    let text = doc.get_text("text");
    text.insert(0, "Q").unwrap();
    doc.commit();
    let mut bad = doc
        .export_json_updates(&VersionVector::default(), &doc.oplog_vv())
        .unwrap();
    bad.changes[0].msg = None;
    let previous = doc.oplog_vv();
    text.insert(1, "P").unwrap();
    assert!(doc.import_json_updates(bad).is_err());
    assert_eq!(text.to_string(), "QP");
    assert_ne!(doc.oplog_vv(), previous);
    assert!(undo.undo().unwrap());
    assert_eq!(text.to_string(), "Q");
}
