//! Unwinding metadata/publication callbacks must not discard committed history.
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use loro::{ExportMode, LoroDoc, UndoItemMeta, UndoManager, UndoOrRedo};

fn modes() -> Vec<bool> {
    let mut modes = vec![false];
    if cfg!(feature = "experimental-identity-undo") {
        modes.push(true);
    }
    modes
}

fn manager(doc: &LoroDoc, identity: bool) -> UndoManager {
    #[cfg(feature = "experimental-identity-undo")]
    if identity {
        return UndoManager::new_identity(doc).unwrap();
    }
    assert!(!identity);
    UndoManager::new(doc)
}

fn expect_panic(f: impl FnOnce(), expected: &str) {
    let payload = catch_unwind(AssertUnwindSafe(f)).unwrap_err();
    assert_eq!(payload.downcast_ref::<&str>().copied(), Some(expected));
}

fn replay(doc: &LoroDoc) {
    let cold = LoroDoc::new();
    cold.import(&doc.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    assert_eq!(cold.get_deep_value(), doc.get_deep_value());
    assert_eq!(cold.oplog_vv(), doc.oplog_vv());
}

fn insert_from_other_thread(doc: &LoroDoc) {
    let doc = doc.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let writer = std::thread::spawn(move || {
        doc.get_text("t").insert(0, "z").unwrap();
        doc.commit();
        tx.send(()).unwrap();
    });
    rx.recv_timeout(std::time::Duration::from_secs(3))
        .expect("writer gate leaked after panic");
    writer.join().unwrap();
}

#[test]
fn local_push_panic_records_the_committed_edit_once() {
    for identity in modes() {
        let doc = LoroDoc::new();
        let mut undo = manager(&doc, identity);
        let text = doc.get_text("t");
        undo.set_on_push(Some(Box::new(|_, _, _| panic!("local metadata"))));
        text.insert(0, "a🦀").unwrap();
        expect_panic(|| doc.commit(), "local metadata");
        assert_eq!(text.to_string(), "a🦀");
        assert_eq!((undo.undo_count(), undo.redo_count()), (1, 0));
        undo.set_on_push(None);
        // A later checkpoint must not record the same committed interval twice.
        doc.commit();
        assert_eq!(undo.undo_count(), 1);
        assert!(undo.undo().unwrap());
        assert_eq!(text.to_string(), "");
        assert!(undo.redo().unwrap());
        assert_eq!(text.to_string(), "a🦀");
        replay(&doc);
    }
}

#[test]
fn pop_and_opposite_push_panics_finish_each_stack_transition() {
    for identity in modes() {
        for redo in [false, true] {
            for (pop, push) in [(true, false), (false, true), (true, true)] {
                let doc = LoroDoc::new();
                let mut undo = manager(&doc, identity);
                let text = doc.get_text("t");
                text.insert(0, "a🦀").unwrap();
                doc.commit();
                if redo {
                    assert!(undo.undo().unwrap());
                }
                let calls = Arc::new(Mutex::new(Vec::new()));
                let log = calls.clone();
                undo.set_on_pop(Some(Box::new(move |kind, _, _| {
                    log.lock().unwrap().push(("pop", kind));
                    if pop {
                        panic!("pop metadata");
                    }
                })));
                let log = calls.clone();
                undo.set_on_push(Some(Box::new(move |kind, _, _| {
                    log.lock().unwrap().push(("push", kind));
                    if push {
                        panic!("push metadata");
                    }
                    UndoItemMeta {
                        value: "opposite".into(),
                        ..Default::default()
                    }
                })));
                expect_panic(
                    || {
                        if redo {
                            undo.redo().unwrap();
                        } else {
                            undo.undo().unwrap();
                        }
                    },
                    if pop { "pop metadata" } else { "push metadata" },
                );
                assert_eq!(text.to_string(), if redo { "a🦀" } else { "" });
                assert_eq!(
                    (undo.undo_count(), undo.redo_count()),
                    if redo { (1, 0) } else { (0, 1) }
                );
                let meta = if redo {
                    undo.top_undo_meta()
                } else {
                    undo.top_redo_meta()
                }
                .unwrap();
                assert_eq!(
                    meta.value,
                    if push {
                        loro::LoroValue::Null
                    } else {
                        "opposite".into()
                    }
                );
                assert_eq!(
                    *calls.lock().unwrap(),
                    if redo {
                        vec![("pop", UndoOrRedo::Redo), ("push", UndoOrRedo::Undo)]
                    } else {
                        vec![("pop", UndoOrRedo::Undo), ("push", UndoOrRedo::Redo)]
                    }
                );
                undo.set_on_pop(None);
                undo.set_on_push(None);
                for _ in 0..3 {
                    if redo {
                        assert!(undo.undo().unwrap());
                        assert_eq!(text.to_string(), "");
                        assert!(undo.redo().unwrap());
                    } else {
                        assert!(undo.redo().unwrap());
                        assert_eq!(text.to_string(), "a🦀");
                        assert!(undo.undo().unwrap());
                    }
                }
                replay(&doc);
                // The processing guard must also permit fresh local history.
                insert_from_other_thread(&doc);
                assert!(undo.undo().unwrap());
                assert_eq!(text.to_string(), if redo { "a🦀" } else { "" });
            }
        }
    }
}

#[test]
fn no_op_pop_panic_consumes_only_that_item_without_an_opposite() {
    for identity in modes() {
        let a = LoroDoc::new();
        a.set_peer_id(1).unwrap();
        let b = LoroDoc::new();
        b.set_peer_id(2).unwrap();
        let mut undo = manager(&a, identity);
        let _other = manager(&b, identity);
        let text = a.get_text("t");
        text.insert(0, "a").unwrap();
        a.commit();
        text.insert(1, "🦀").unwrap();
        a.commit();
        b.import(&a.export(ExportMode::Snapshot).unwrap()).unwrap();
        b.get_text("t").delete(1, 1).unwrap();
        b.commit();
        a.import(&b.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        let vv = a.oplog_vv();
        let bytes = a.export(ExportMode::all_updates()).unwrap();
        assert_eq!((undo.undo_count(), undo.redo_count()), (2, 0));
        undo.set_on_pop(Some(Box::new(|_, _, _| panic!("no-op metadata"))));
        expect_panic(
            || {
                undo.undo().unwrap();
            },
            "no-op metadata",
        );
        assert_eq!((undo.undo_count(), undo.redo_count()), (1, 0));
        assert_eq!(a.oplog_vv(), vv);
        assert_eq!(a.export(ExportMode::all_updates()).unwrap(), bytes);
        undo.set_on_pop(None);
        assert!(undo.undo().unwrap());
        assert_eq!(text.to_string(), "");
        assert!(undo.redo().unwrap());
        assert_eq!(text.to_string(), "a");
        replay(&a);
    }
}

#[test]
fn failed_local_metadata_still_invalidates_redo_and_obeys_grouping_and_bound() {
    for identity in modes() {
        let doc = LoroDoc::new();
        let mut undo = manager(&doc, identity);
        let text = doc.get_text("t");
        text.insert(0, "a").unwrap();
        doc.commit();
        text.insert(1, "b").unwrap();
        doc.commit();
        assert!(undo.undo().unwrap());
        assert_eq!((undo.undo_count(), undo.redo_count()), (1, 1));
        undo.set_max_undo_steps(1);
        undo.group_start().unwrap();
        undo.set_on_push(Some(Box::new(|_, _, _| panic!("group metadata"))));
        for value in ["x", "y"] {
            text.insert(text.len_unicode(), value).unwrap();
            expect_panic(|| doc.commit(), "group metadata");
            assert_eq!((undo.undo_count(), undo.redo_count()), (1, 0));
        }
        undo.set_on_push(None);
        undo.group_end();
        assert_eq!(text.to_string(), "axy");
        assert!(undo.undo().unwrap());
        assert_eq!(text.to_string(), "a");
        assert!(!undo.undo().unwrap());
        assert!(undo.redo().unwrap());
        assert_eq!(text.to_string(), "axy");
        replay(&doc);
    }
}

#[test]
fn inverse_publication_panic_keeps_the_committed_opposite() {
    for identity in modes() {
        for redo in [false, true] {
            for site in ["pre-commit", "root", "local-update"] {
                let doc = LoroDoc::new();
                let mut undo = manager(&doc, identity);
                let text = doc.get_text("t");
                text.insert(0, "a🦀").unwrap();
                doc.commit();
                if redo {
                    assert!(undo.undo().unwrap());
                }
                let receiver = LoroDoc::new();
                let _receiver_manager = manager(&receiver, identity);
                receiver
                    .import(&doc.export(ExportMode::Snapshot).unwrap())
                    .unwrap();
                let armed = Arc::new(AtomicBool::new(true));
                let flag = armed.clone();
                let _panicking = match site {
                    "pre-commit" => doc.subscribe_pre_commit(Box::new(move |_| {
                        if flag.swap(false, Ordering::SeqCst) {
                            panic!("publication callback");
                        }
                        true
                    })),
                    "root" => doc.subscribe_root(Arc::new(move |_| {
                        if flag.swap(false, Ordering::SeqCst) {
                            panic!("publication callback");
                        }
                    })),
                    _ => doc.subscribe_local_update(Box::new(move |_| {
                        if flag.swap(false, Ordering::SeqCst) {
                            panic!("publication callback");
                        }
                        true
                    })),
                };
                let events = Arc::new(AtomicUsize::new(0));
                let log = events.clone();
                let _observer = doc.subscribe_root(Arc::new(move |_| {
                    log.fetch_add(1, Ordering::SeqCst);
                }));
                let updates = Arc::new(Mutex::new(Vec::new()));
                let log = updates.clone();
                let _update = doc.subscribe_local_update(Box::new(move |bytes| {
                    log.lock().unwrap().push(bytes.to_vec());
                    true
                }));
                // A later metadata panic must not replace the first payload.
                undo.set_on_pop(Some(Box::new(|_, _, _| panic!("later metadata"))));
                expect_panic(
                    || {
                        if redo {
                            undo.redo().unwrap();
                        } else {
                            undo.undo().unwrap();
                        }
                    },
                    "publication callback",
                );
                assert!(
                    !armed.load(Ordering::SeqCst),
                    "callback did not run at {site}"
                );
                assert_eq!(text.to_string(), if redo { "a🦀" } else { "" }, "{site}");
                assert_eq!(
                    (undo.undo_count(), undo.redo_count()),
                    if redo { (1, 0) } else { (0, 1) },
                    "{site}"
                );
                assert_eq!(events.load(Ordering::SeqCst), 1, "{site}");
                assert_eq!(updates.lock().unwrap().len(), 1, "{site}");
                let update = updates.lock().unwrap()[0].clone();
                receiver.import(&update).unwrap();
                let received_vv = receiver.oplog_vv();
                receiver.import(&update).unwrap();
                assert_eq!(receiver.oplog_vv(), received_vv);
                assert_eq!(receiver.oplog_vv(), doc.oplog_vv());
                assert_eq!(receiver.get_deep_value(), doc.get_deep_value());
                replay(&doc);
                undo.set_on_pop(None);
                for _ in 0..3 {
                    if redo {
                        assert!(undo.undo().unwrap());
                        assert_eq!(text.to_string(), "");
                        assert!(undo.redo().unwrap());
                    } else {
                        assert!(undo.redo().unwrap());
                        assert_eq!(text.to_string(), "a🦀");
                        assert!(undo.undo().unwrap());
                    }
                }
                insert_from_other_thread(&doc);
                assert!(undo.undo().unwrap());
                assert_eq!(text.to_string(), if redo { "a🦀" } else { "" });
            }
        }
    }
}

#[test]
fn first_pre_commit_panic_survives_later_publication_and_metadata_panics() {
    for identity in modes() {
        let doc = LoroDoc::new();
        let mut undo = manager(&doc, identity);
        doc.get_text("t").insert(0, "a").unwrap();
        doc.commit();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let log = calls.clone();
        let once = AtomicBool::new(true);
        let _pre = doc.subscribe_pre_commit(Box::new(move |_| {
            if once.swap(false, Ordering::SeqCst) {
                log.lock().unwrap().push("pre");
                panic!("first pre-commit");
            }
            true
        }));
        let log = calls.clone();
        let once = AtomicBool::new(true);
        let _root = doc.subscribe_root(Arc::new(move |_| {
            if once.swap(false, Ordering::SeqCst) {
                log.lock().unwrap().push("root");
                panic!("later root");
            }
        }));
        let log = calls.clone();
        let once = AtomicBool::new(true);
        let _local = doc.subscribe_local_update(Box::new(move |_| {
            if once.swap(false, Ordering::SeqCst) {
                log.lock().unwrap().push("local");
                panic!("later local");
            }
            true
        }));
        let log = calls.clone();
        undo.set_on_pop(Some(Box::new(move |_, _, _| {
            log.lock().unwrap().push("pop");
            panic!("later pop");
        })));
        let log = calls.clone();
        undo.set_on_push(Some(Box::new(move |_, _, _| {
            log.lock().unwrap().push("push");
            panic!("later push");
        })));
        expect_panic(
            || {
                undo.undo().unwrap();
            },
            "first pre-commit",
        );
        assert_eq!(
            *calls.lock().unwrap(),
            ["pre", "root", "local", "pop", "push"]
        );
        assert_eq!((undo.undo_count(), undo.redo_count()), (0, 1));
        assert_eq!(doc.get_text("t").to_string(), "");
        undo.set_on_push(None);
        undo.set_on_pop(None);
        assert!(undo.redo().unwrap());
        assert_eq!(doc.get_text("t").to_string(), "a");
        replay(&doc);
    }
}
