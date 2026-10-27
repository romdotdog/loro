//! Candidate atomicity includes its manager-owned, release-compiled relation.
use super::*;
use crate::{cursor::PosType, loro::FAIL_APPLY_DIFF_AFTER_HANDLER};
use std::sync::atomic::{AtomicUsize, Ordering};
#[test]
fn candidate_errors_preserve_manager_relation_and_both_stacks() {
    for list in [false, true] {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        doc.enable_identity_history().unwrap();
        let text = doc.get_text("text");
        let seq = doc.get_list("sequence");
        if list {
            for (i, s) in ["a", "B"].iter().enumerate() {
                seq.insert(i, *s).unwrap();
            }
        } else {
            text.insert(0, "aB", PosType::Unicode).unwrap();
        }
        doc.commit_then_renew();
        let manager = UndoManager::new_identity(&doc).unwrap();
        if list {
            seq.delete(1, 1).unwrap();
            seq.insert(1, "x").unwrap();
        } else {
            text.delete(1, 1, PosType::Unicode).unwrap();
            text.insert(1, "x", PosType::Unicode).unwrap();
        }
        doc.commit_then_renew();
        let events = Arc::new(AtomicUsize::new(0));
        let observed = events.clone();
        let _sub = doc.subscribe_root(Arc::new(move |_| {
            observed.fetch_add(1, Ordering::Relaxed);
        }));
        for _ in 0..3 {
            for redo in [false, true] {
                let before = doc.get_deep_value();
                let vv = doc.oplog_vv();
                let frontiers = doc.state_frontiers();
                let edges = manager.identity_receipts.lock().clone();
                let counts = (manager.undo_count(), manager.redo_count());
                let n = events.load(Ordering::Relaxed);
                for _ in 0..2 {
                    FAIL_APPLY_DIFF_AFTER_HANDLER.with(|flag| flag.set(true));
                    let result = if redo { manager.redo() } else { manager.undo() };
                    assert!(result.is_err());
                    assert_eq!(doc.get_deep_value(), before);
                    assert_eq!(doc.oplog_vv(), vv);
                    assert_eq!(doc.state_frontiers(), frontiers);
                    assert_eq!((manager.undo_count(), manager.redo_count()), counts);
                    assert_eq!(*manager.identity_receipts.lock(), edges);
                    assert_eq!(events.load(Ordering::Relaxed), n);
                }
                assert!(if redo {
                    manager.redo().unwrap()
                } else {
                    manager.undo().unwrap()
                });
                assert_eq!(
                    manager.identity_receipts.lock().as_ref().unwrap().len(),
                    edges.as_ref().unwrap().len() + 1
                );
            }
        }
    }
}

#[test]
fn recovery_import_rejects_incomplete_or_forged_records_before_publication() {
    use crate::loro::ExportMode;
    for list in [false, true] {
        let a = LoroDoc::new_auto_commit();
        a.set_peer_id(1).unwrap();
        a.enable_identity_history().unwrap();
        let t = a.get_text("sequence");
        let l = a.get_list("sequence");
        if list {
            l.insert(0, "Q").unwrap();
        } else {
            t.insert(0, "Q", PosType::Unicode).unwrap();
        }
        a.commit_then_renew();
        let local = UndoManager::new_identity(&a).unwrap();
        if list {
            l.delete(0, 1).unwrap();
        } else {
            t.delete(0, 1, PosType::Unicode).unwrap();
        }
        a.commit_then_renew();
        assert!(local.undo().unwrap());
        let b = LoroDoc::new_auto_commit();
        b.set_peer_id(2).unwrap();
        b.enable_identity_history().unwrap();
        b.import(&a.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        let peer = UndoManager::new_identity(&b).unwrap();
        if list {
            b.get_list("sequence").delete(0, 1).unwrap();
        } else {
            b.get_text("sequence")
                .delete(0, 1, PosType::Unicode)
                .unwrap();
        }
        b.commit_then_renew();
        assert!(peer.undo().unwrap());
        let good = b
            .export_json_updates(&Default::default(), &b.oplog_vv(), false)
            .unwrap();
        let restored = good
            .changes
            .iter()
            .rposition(|c| {
                c.id.peer == 2
                    && c.msg.as_ref().is_some_and(|s| {
                        s.contains("\"inverse\":true") && !s.contains("\"edges\":[]")
                    })
            })
            .unwrap();
        let original = good.changes[restored].msg.clone().unwrap();
        let mut prefix_updates = good.clone();
        prefix_updates.changes.truncate(restored);
        let prefix_doc = LoroDoc::new_auto_commit();
        prefix_doc.import_json_updates(prefix_updates).unwrap();
        let valid_prefix = prefix_doc.export(ExportMode::all_updates()).unwrap();
        let prefix = "loro:identity-undo:1\n";
        let record: serde_json::Value =
            serde_json::from_str(original.strip_prefix(prefix).unwrap()).unwrap();
        let before = a.get_deep_value();
        let vv = a.oplog_vv();
        let frontiers = a.state_frontiers();
        let stacks = (local.undo_count(), local.redo_count());
        let edges = local.identity_receipts.lock().clone();
        let events = Arc::new(AtomicUsize::new(0));
        let observed = events.clone();
        let _sub = a.subscribe_root(Arc::new(move |_| {
            observed.fetch_add(1, Ordering::Relaxed);
        }));
        for mutation in 0..9 {
            let mut bad = good.clone();
            let mut r = record.clone();
            match mutation {
                0 => bad.changes[restored].msg = None,
                1 => bad.changes[restored].msg = Some(format!("{prefix}{{")),
                2 => bad.changes[restored].msg = Some(original.replace("undo:1", "undo:99")),
                3 => {
                    r["edges"] = serde_json::json!([]);
                }
                4 => {
                    let e = r["edges"][0].clone();
                    r["edges"].as_array_mut().unwrap().push(e);
                }
                5 => {
                    r["edges"][0][3] = serde_json::json!(999999);
                }
                6 => {
                    r["edges"][0][2] = r["edges"][0][0].clone();
                    r["edges"][0][3] = r["edges"][0][1].clone();
                }
                7 => {
                    r["peer"] = serde_json::json!(99);
                }
                8 => {
                    r["inverse"] = serde_json::json!(false);
                }
                _ => unreachable!(),
            }
            if mutation >= 3 {
                bad.changes[restored].msg = Some(format!("{prefix}{r}"));
            }
            // Exercise all three public native import paths with the same bad
            // histories, including a batch containing a valid prefix.
            let ordinary = LoroDoc::new_auto_commit();
            ordinary.import_json_updates(bad.clone()).unwrap();
            let binary = ordinary.export(ExportMode::all_updates()).unwrap();
            for path in 0..3 {
                let result = match path {
                    0 => a.import_json_updates(bad.clone()),
                    1 => a.import(&binary),
                    2 => a.import_batch(&[valid_prefix.clone(), binary.clone()]),
                    _ => unreachable!(),
                };
                assert!(result.is_err(), "mutation {mutation}, path {path}");
                assert_eq!(a.get_deep_value(), before);
                assert_eq!(a.oplog_vv(), vv);
                assert_eq!(a.state_frontiers(), frontiers);
                assert_eq!((local.undo_count(), local.redo_count()), stacks);
                assert_eq!(*local.identity_receipts.lock(), edges);
                assert_eq!(events.load(Ordering::Relaxed), 0);
            }
        }
        a.import_json_updates(good).unwrap();
        assert_eq!(a.get_deep_value(), b.get_deep_value());
        assert!(local.redo().unwrap());
        assert!(local.undo().unwrap());
    }
}

#[test]
fn recovery_cache_is_rebuilt_from_committed_history_not_old_manager_memory() {
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(1).unwrap();
    doc.enable_identity_history().unwrap();
    let text = doc.get_text("text");
    text.insert(0, "🦀B", PosType::Unicode).unwrap();
    doc.commit_then_renew();
    let undo = UndoManager::new_identity(&doc).unwrap();
    text.delete(0, 1, PosType::Unicode).unwrap();
    doc.commit_then_renew();
    assert!(undo.undo().unwrap());
    // Erase the entire derived cache, not the committed record.
    *undo.identity_receipts.lock() = Some(Vec::new());
    assert!(undo.redo().unwrap());
    assert_eq!(text.to_string(), "B");
    assert!(undo.undo().unwrap());
    assert_eq!(text.to_string(), "🦀B");
    assert_eq!(undo.identity_receipts.lock().as_ref().unwrap().len(), 2);
}

#[test]
fn recovered_foreign_relation_and_new_receipts_publish_only_after_success() {
    use crate::loro::ExportMode;
    for list in [false, true] {
        let a = LoroDoc::new_auto_commit();
        a.set_peer_id(1).unwrap();
        a.enable_identity_history().unwrap();
        let t = a.get_text("sequence");
        let l = a.get_list("sequence");
        let insert = |at, s| {
            if list {
                l.insert(at, s).unwrap();
            } else {
                t.insert(at, s, PosType::Unicode).unwrap();
            }
        };
        for (i, s) in ["(", "X", "B", ")"].iter().enumerate() {
            insert(i, *s);
        }
        a.commit_then_renew();
        let manager = UndoManager::new_identity(&a).unwrap();
        for _ in 0..2 {
            if list {
                l.delete(1, 1).unwrap();
            } else {
                t.delete(1, 1, PosType::Unicode).unwrap();
            }
            a.commit_then_renew();
        }
        let b = LoroDoc::new_auto_commit();
        b.set_peer_id(2).unwrap();
        b.enable_identity_history().unwrap();
        b.import(&a.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        let peer = UndoManager::new_identity(&b).unwrap();
        if list {
            b.get_list("sequence").delete(0, 2).unwrap();
        } else {
            b.get_text("sequence")
                .delete(0, 2, PosType::Unicode)
                .unwrap();
        }
        b.commit_then_renew();
        assert!(peer.undo().unwrap());
        a.import(&b.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        for expected in ["(B)", "(XB)"] {
            let before = a.get_deep_value();
            let vv = a.oplog_vv();
            let counts = (manager.undo_count(), manager.redo_count());
            let edges = manager.identity_receipts.lock().clone();
            let encoded = a.export(ExportMode::all_updates()).unwrap();
            for _ in 0..2 {
                FAIL_APPLY_DIFF_AFTER_HANDLER.with(|flag| flag.set(true));
                assert!(manager.undo().is_err());
                assert_eq!(a.get_deep_value(), before);
                assert_eq!(a.oplog_vv(), vv);
                assert_eq!((manager.undo_count(), manager.redo_count()), counts);
                assert_eq!(*manager.identity_receipts.lock(), edges);
                assert_eq!(a.export(ExportMode::all_updates()).unwrap(), encoded);
            }
            assert!(manager.undo().unwrap());
            let source = if list {
                (0..l.len())
                    .map(|i| match l.get(i).unwrap() {
                        LoroValue::String(s) => s.to_string(),
                        _ => unreachable!(),
                    })
                    .collect::<String>()
            } else {
                t.to_string()
            };
            assert_eq!(source, expected);
        }
        assert_eq!(manager.identity_receipts.lock().as_ref().unwrap().len(), 4);
    }
}

#[test]
fn metadata_wraps_final_user_message_and_recovers_without_fabricating_event_positions() {
    let doc = LoroDoc::new_auto_commit();
    doc.enable_identity_history().unwrap();
    let _sub = doc.subscribe_pre_commit(Box::new(|p| {
        p.modifier.set_message("author's message");
        true
    }));
    let text = doc.get_text("text");
    text.insert(0, "a", PosType::Unicode).unwrap();
    doc.commit_then_renew();
    let undo = UndoManager::new_identity(&doc).unwrap();
    text.delete(0, 1, PosType::Unicode).unwrap();
    doc.commit_then_renew();
    assert!(undo.undo().unwrap());
    assert!(undo.identity_receipts.lock().as_ref().unwrap()[0]
        .output
        .is_some());
    let recovered = doc.identity_recover().unwrap().unwrap();
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].output, None);
    let json = doc
        .export_json_updates(&Default::default(), &doc.oplog_vv(), false)
        .unwrap();
    for change in json.changes {
        let record: serde_json::Value = serde_json::from_str(
            change
                .msg
                .unwrap()
                .strip_prefix("loro:identity-undo:1\n")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(record["message"], "author's message");
    }
}

#[test]
fn derived_membership_tokens_and_both_stacks_survive_failed_inverse_retry() {
    for list in [false, true] {
        let doc = LoroDoc::new_auto_commit();
        doc.enable_identity_history().unwrap();
        let t = doc.get_text("s");
        let l = doc.get_list("s");
        let insert = |at, s| {
            if list {
                l.insert(at, s).unwrap();
            } else {
                t.insert(at, s, PosType::Unicode).unwrap();
            }
        };
        let manager = UndoManager::new_identity(&doc).unwrap();
        insert(0, "(");
        doc.commit_then_renew();
        let trigger = manager.capture_trigger().unwrap();
        insert(1, "B");
        doc.commit_then_renew();
        manager.with_derived(&trigger, || insert(2, ")")).unwrap();
        let membership = |s: &Stack| {
            s.stack
                .iter()
                .flat_map(|row| row.0.iter())
                .map(|i| (i.span, i.segments.clone(), i.trigger))
                .collect::<Vec<_>>()
        };
        for _ in 0..3 {
            for redo in [false, false, true, true] {
                let source = doc.get_deep_value();
                let vv = doc.oplog_vv();
                let frontier = doc.state_frontiers();
                let old_cache = manager.identity_receipts.lock().clone();
                let undo = membership(&manager.inner.lock().borrow().undo_stack);
                let redo_items = membership(&manager.inner.lock().borrow().redo_stack);
                for _ in 0..2 {
                    FAIL_APPLY_DIFF_AFTER_HANDLER.with(|f| f.set(true));
                    assert!(if redo { manager.redo() } else { manager.undo() }.is_err());
                    assert_eq!(doc.get_deep_value(), source);
                    assert_eq!(doc.oplog_vv(), vv);
                    assert_eq!(doc.state_frontiers(), frontier);
                    assert_eq!(*manager.identity_receipts.lock(), old_cache);
                    assert_eq!(membership(&manager.inner.lock().borrow().undo_stack), undo);
                    assert_eq!(
                        membership(&manager.inner.lock().borrow().redo_stack),
                        redo_items
                    );
                }
                assert!(if redo {
                    manager.redo().unwrap()
                } else {
                    manager.undo().unwrap()
                });
            }
            manager.check_trigger(&trigger).unwrap();
        }
    }
}

#[test]
fn known_metadata_rejection_preserves_the_prior_recovery_cache_and_history() {
    use crate::loro::ExportMode;
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(1).unwrap();
    let undo = UndoManager::new_identity(&doc).unwrap();
    undo.set_merge_interval(0);
    let text = doc.get_text("text");
    text.insert(0, "QQ", PosType::Unicode).unwrap();
    doc.commit_then_renew();
    text.delete(0, 2, PosType::Unicode).unwrap();
    doc.commit_then_renew();
    assert!(undo.undo().unwrap());
    let mut bad = doc
        .export_json_updates(&Default::default(), &doc.oplog_vv(), false)
        .unwrap();
    bad.changes.last_mut().unwrap().msg = None;
    let relay = LoroDoc::new_auto_commit();
    relay.import_json_updates(bad.clone()).unwrap();
    let snapshot = relay.export(ExportMode::Snapshot).unwrap();
    let before = doc.export(ExportMode::Snapshot).unwrap();
    let relation = undo.identity_receipts.lock().clone();
    let stacks = (undo.undo_count(), undo.redo_count());
    let vv = doc.oplog_vv();
    for binary in [false, true] {
        let rejected = if binary {
            doc.import(&snapshot)
        } else {
            doc.import_json_updates(bad.clone())
        };
        assert!(rejected.is_err());
        assert_eq!(*undo.identity_receipts.lock(), relation);
        assert_eq!((undo.undo_count(), undo.redo_count()), stacks);
        assert_eq!(doc.oplog_vv(), vv);
        assert_eq!(doc.export(ExportMode::Snapshot).unwrap(), before);
    }
    assert!(undo.redo().unwrap());
    assert!(undo.undo().unwrap());
    assert_eq!(text.to_string(), "QQ");
}

#[test]
#[ignore = "bounded timing probe; run explicitly with --ignored --nocapture"]
fn profile_committed_identity_recovery_at_quota() {
    for unit in ["a", "🦀"] {
        for bytes in [1024, 2048, 4096] {
            let source = unit.repeat(bytes / unit.len());
            let doc = LoroDoc::new_auto_commit();
            doc.set_peer_id(1).unwrap();
            doc.enable_identity_history().unwrap();
            let text = doc.get_text("text");
            text.insert(0, &source, PosType::Unicode).unwrap();
            doc.commit_then_renew();
            let manager = UndoManager::new_identity(&doc).unwrap();
            text.delete(0, source.chars().count(), PosType::Unicode)
                .unwrap();
            doc.commit_then_renew();
            for _ in 0..3 {
                assert!(manager.undo().unwrap());
                assert_eq!(text.to_string(), source);
                assert!(manager.redo().unwrap());
                assert_eq!(text.to_string(), "");
            }
            let mut measurements = Vec::new();
            for _ in 0..3 {
                let start = std::time::Instant::now();
                let recovered = super::recovery::recover(&doc).unwrap();
                measurements.push(start.elapsed().as_micros());
                assert_eq!(recovered.len(), 3 * source.chars().count());
            }
            eprintln!("recovery unit={unit:?} bytes={bytes} scalars={} receipts={} micros={measurements:?}",
                source.chars().count(), 3 * source.chars().count());
        }
    }
}

#[test]
fn recovery_indexes_unicode_spans_by_peer_and_actual_creation_offset() {
    let make = |peer, value: &str| {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(peer).unwrap();
        doc.enable_identity_history().unwrap();
        let text = doc.get_text("text");
        text.insert(0, value, PosType::Unicode).unwrap();
        doc.commit_then_renew();
        let undo = UndoManager::new_identity(&doc).unwrap();
        text.delete(0, value.chars().count(), PosType::Unicode)
            .unwrap();
        doc.commit_then_renew();
        assert!(undo.undo().unwrap());
        assert_eq!(text.to_string(), value);
        doc
    };
    // Both peers have the same native counters but different scalar payloads.
    // UTF8 byte offsets and UTF16 code-unit offsets are not creation offsets.
    let a = make(11, "a🦀b字");
    let b = make(22, "z🌍c語");
    a.import(&b.export(crate::loro::ExportMode::all_updates()).unwrap())
        .unwrap();
    let recovered = super::recovery::recover(&a).unwrap();
    assert_eq!(recovered.len(), 8);
    for peer in [11, 22] {
        for offset in 0..4 {
            assert!(recovered
                .iter()
                .any(|r| r.origin == Some(loro_common::ID::new(peer, offset))));
        }
    }
    let cold = LoroDoc::new_auto_commit();
    cold.enable_identity_history().unwrap();
    cold.import(&a.export(crate::loro::ExportMode::Snapshot).unwrap())
        .unwrap();
    assert_eq!(cold.get_deep_value(), a.get_deep_value());
    assert_eq!(super::recovery::recover(&cold).unwrap(), recovered);
}
