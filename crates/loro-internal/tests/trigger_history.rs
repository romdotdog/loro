//! Actual native manager membership; no supplied ranges or fixture relation.
use loro_internal::{
    cursor::PosType,
    handler::{ListHandler, TextHandler},
    loro::ExportMode,
    LoroDoc, UndoManager,
};
enum Seq {
    Text(TextHandler),
    List(ListHandler),
}
impl Seq {
    fn new(doc: &LoroDoc, list: bool) -> Self {
        if list {
            Self::List(doc.get_list("s"))
        } else {
            Self::Text(doc.get_text("s"))
        }
    }
    fn insert(&self, at: usize, s: &str) {
        match self {
            Self::Text(t) => t.insert(at, s, PosType::Unicode).unwrap(),
            Self::List(l) => l.insert(at, s).unwrap(),
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
                    loro_internal::LoroValue::String(s) => s.to_string(),
                    _ => unreachable!(),
                })
                .collect(),
        }
    }
}
fn doc(peer: u64) -> LoroDoc {
    let d = LoroDoc::new_auto_commit();
    d.set_peer_id(peer).unwrap();
    d.enable_identity_history().unwrap();
    d
}
#[test]
fn late_child_is_one_real_item_through_intervening_command_import_and_full_redo() {
    for list in [false, true] {
        for peer in [1, 2] {
            for middle in ["B", "🦀"] {
                for import in [false, true] {
                    let a = doc(peer);
                    let s = Seq::new(&a, list);
                    let manager = UndoManager::new_identity(&a).unwrap();
                    s.insert(0, "(");
                    a.commit_then_renew();
                    let trigger = manager.capture_trigger().unwrap();
                    s.insert(1, middle);
                    a.commit_then_renew();
                    if import {
                        let b = doc(3 - peer);
                        b.import(&a.export(ExportMode::all_updates()).unwrap())
                            .unwrap();
                        let other = Seq::new(&b, list);
                        other.insert(2, "R");
                        b.commit_then_renew();
                        a.import(&b.export(ExportMode::all_updates()).unwrap())
                            .unwrap();
                    }
                    manager
                        .with_derived(&trigger, || s.insert(if import { 3 } else { 2 }, ")"))
                        .unwrap();
                    assert_eq!(manager.undo_count(), 2);
                    let r = if import { "R" } else { "" };
                    for _ in 0..3 {
                        assert!(manager.undo().unwrap());
                        assert_eq!(s.source(), format!("({r})"));
                        assert!(manager.undo().unwrap());
                        assert_eq!(s.source(), r);
                        assert!(!manager.can_undo());
                        assert!(manager.redo().unwrap());
                        assert_eq!(s.source(), format!("({r})"));
                        assert!(manager.redo().unwrap());
                        assert_eq!(s.source(), format!("({middle}{r})"));
                        assert_eq!(manager.undo_count(), 2);
                    }
                    // The same token follows the restored command, not its obsolete IDs.
                    manager.check_trigger(&trigger).unwrap();
                }
            }
        }
    }
}
#[test]
fn multiple_children_and_net_zero_child_never_start_an_item() {
    for list in [false, true] {
        let a = doc(1);
        let s = Seq::new(&a, list);
        let manager = UndoManager::new_identity(&a).unwrap();
        s.insert(0, "(");
        a.commit_then_renew();
        let trigger = manager.capture_trigger().unwrap();
        s.insert(1, "B");
        a.commit_then_renew();
        manager
            .with_derived(&trigger, || {
                s.insert(2, "x");
                // A real implicit export/commit barrier inside the child.
                a.export(ExportMode::all_updates()).unwrap();
                s.delete(2);
            })
            .unwrap();
        assert_eq!(manager.undo_count(), 2);
        manager.with_derived(&trigger, || s.insert(2, ")")).unwrap();
        assert_eq!(manager.undo_count(), 2);
        manager.with_derived(&trigger, || {}).unwrap();
        assert_eq!(manager.undo_count(), 2);
        assert!(manager.undo().unwrap());
        assert_eq!(s.source(), "()");
        assert!(manager.undo().unwrap());
        assert_eq!(s.source(), "");
        assert!(manager.redo().unwrap());
        assert_eq!(s.source(), "()");
        assert!(manager.redo().unwrap());
        assert_eq!(s.source(), "(B)");
    }
}

#[test]
#[ignore = "proposed wrapping placement when the child was created while B was inactive; tie policy remains unsettled"]
fn proposed_wrapping_slot_for_child_created_while_b_is_inactive() {
    for list in [false, true] {
        let a = doc(1);
        let s = Seq::new(&a, list);
        let manager = UndoManager::new_identity(&a).unwrap();
        s.insert(0, "(");
        a.commit_then_renew();
        let trigger = manager.capture_trigger().unwrap();
        s.insert(1, "B");
        a.commit_then_renew();
        assert!(manager.undo().unwrap());
        assert_eq!(s.source(), "(");
        assert_eq!(manager.redo_count(), 1);
        manager.with_derived(&trigger, || s.insert(1, ")")).unwrap();
        assert_eq!((manager.undo_count(), manager.redo_count()), (1, 1));
        assert!(manager.undo().unwrap());
        assert_eq!(s.source(), "");
        assert!(manager.redo().unwrap());
        assert_eq!(s.source(), "()");
        manager.check_trigger(&trigger).unwrap();
        manager.with_derived(&trigger, || s.insert(2, "!")).unwrap();
        assert_eq!((manager.undo_count(), manager.redo_count()), (1, 1));
        assert!(manager.redo().unwrap());
        assert_eq!(s.source(), "(B)!");
        assert!(manager.undo().unwrap());
        assert_eq!(s.source(), "()!");
        assert!(manager.undo().unwrap());
        assert_eq!(s.source(), "");
        assert!(manager.redo().unwrap());
        assert_eq!(s.source(), "()!");
        assert!(manager.redo().unwrap());
        assert_eq!(s.source(), "(B)!");
    }
}

#[test]
fn foreign_inactive_cleared_and_evicted_tokens_reject_without_creating_a_group() {
    let a = doc(1);
    let s = Seq::new(&a, false);
    let manager = UndoManager::new_identity(&a).unwrap();
    s.insert(0, "a");
    a.commit_then_renew();
    let trigger = manager.capture_trigger().unwrap();
    let other = UndoManager::new_identity(&a).unwrap();
    let before = a.oplog_vv();
    assert!(other
        .with_derived(&trigger, || panic!("foreign token must not execute work"))
        .is_err());
    assert_eq!(a.oplog_vv(), before);
    assert_eq!((other.undo_count(), other.redo_count()), (0, 0));
    assert!(manager.undo().unwrap());
    assert!(manager
        .with_derived(&trigger, || panic!("inactive token must not execute work"))
        .is_err());
    assert_eq!(s.source(), "");
    assert_eq!((manager.undo_count(), manager.redo_count()), (0, 1));
    assert!(manager.redo().unwrap());
    manager.check_trigger(&trigger).unwrap();
    manager.clear();
    assert!(manager.check_trigger(&trigger).is_err());
    s.insert(1, "b");
    a.commit_then_renew();
    let retained = manager.capture_trigger().unwrap();
    manager.set_max_undo_steps(1);
    s.insert(2, "c");
    a.commit_then_renew();
    assert!(manager.check_trigger(&retained).is_err());
    assert_eq!(manager.undo_count(), 1);
}

fn hidden_creator(list: bool) {
    let a = doc(1);
    let s = Seq::new(&a, list);
    let manager = UndoManager::new_identity(&a).unwrap();
    s.insert(0, "a");
    a.commit_then_renew();
    let trigger = manager.capture_trigger().unwrap();
    s.insert(1, "B");
    a.commit_then_renew();
    manager
        .with_derived(&trigger, || {
            s.delete(1);
            s.insert(1, "x");
        })
        .unwrap();
    assert_eq!(s.source(), "ax");
    assert_eq!(manager.undo_count(), 2);
    // Preserve one-item native no-op retirement: no virtual opposite for B.
    assert!(!manager.undo().unwrap());
    assert_eq!(s.source(), "ax");
    assert_eq!((manager.undo_count(), manager.redo_count()), (1, 0));
    assert!(manager.undo().unwrap());
    assert_eq!(
        s.source(),
        "",
        "retired invisible B creation must stay inactive"
    );
}
#[test]
#[ignore = "candidate lacks native inactive-creation intent after consumed invisible undo"]
fn hidden_creator_text_is_not_resurrected_by_trigger_inverse() {
    hidden_creator(false);
}
#[test]
#[ignore = "candidate lacks native inactive-creation intent after consumed invisible undo"]
fn hidden_creator_list_is_not_resurrected_by_trigger_inverse() {
    hidden_creator(true);
}

#[test]
fn token_follows_redo_and_later_child_does_not_widen_over_intervening_items() {
    for list in [false, true] {
        let a = doc(1);
        let s = Seq::new(&a, list);
        let manager = UndoManager::new_identity(&a).unwrap();
        s.insert(0, "(");
        a.commit_then_renew();
        let trigger = manager.capture_trigger().unwrap();
        s.insert(1, "B");
        a.commit_then_renew();
        manager.with_derived(&trigger, || s.insert(2, ")")).unwrap();
        for _ in 0..2 {
            assert!(manager.undo().unwrap());
        }
        for _ in 0..2 {
            assert!(manager.redo().unwrap());
        }
        manager.with_derived(&trigger, || s.insert(3, "!")).unwrap();
        assert_eq!(manager.undo_count(), 2);
        for _ in 0..3 {
            assert!(manager.undo().unwrap());
            assert_eq!(s.source(), "()!");
            assert!(manager.undo().unwrap());
            assert_eq!(s.source(), "");
            assert!(manager.redo().unwrap());
            assert_eq!(s.source(), "()!");
            assert!(manager.redo().unwrap());
            assert_eq!(s.source(), "(B)!");
        }
    }
}

#[test]
fn derived_scope_finishes_attribution_on_error_value_and_unwinding() {
    for list in [false, true] {
        let a = doc(1);
        let s = Seq::new(&a, list);
        let manager = UndoManager::new_identity(&a).unwrap();
        s.insert(0, "(");
        a.commit_then_renew();
        let trigger = manager.capture_trigger().unwrap();
        s.insert(1, "B");
        a.commit_then_renew();
        let result: Result<(), &str> = manager
            .with_derived(&trigger, || {
                s.insert(2, ")");
                a.commit_then_renew();
                Err("work failed after its native commit")
            })
            .unwrap();
        assert!(result.is_err());
        assert_eq!(manager.undo_count(), 2);
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = manager.with_derived(&trigger, || {
                s.insert(3, "!");
                panic!("scoped work panic");
            });
        }));
        assert!(panic.is_err());
        assert_eq!(s.source(), "(B)!");
        assert_eq!(manager.undo_count(), 2);
        assert!(manager.undo().unwrap());
        assert_eq!(s.source(), "()!");
        assert!(manager.undo().unwrap());
        assert_eq!(s.source(), "");
        assert!(manager.redo().unwrap());
        assert_eq!(s.source(), "()!");
        assert!(manager.redo().unwrap());
        assert_eq!(s.source(), "(B)!");
    }
}

#[test]
fn subscriber_panic_during_final_commit_keeps_actual_child_attribution() {
    let a = doc(1);
    let s = Seq::new(&a, false);
    let manager = UndoManager::new_identity(&a).unwrap();
    s.insert(0, "(");
    a.commit_then_renew();
    let trigger = manager.capture_trigger().unwrap();
    s.insert(1, "B");
    a.commit_then_renew();
    let once = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let seen = once.clone();
    let sub = a.subscribe_root(std::sync::Arc::new(move |_| {
        if seen.swap(false, std::sync::atomic::Ordering::Relaxed) {
            panic!("derived subscriber panic");
        }
    }));
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        manager.with_derived(&trigger, || s.insert(2, ")")).unwrap();
    }))
    .unwrap_err();
    assert_eq!(
        panic.downcast_ref::<&str>().copied(),
        Some("derived subscriber panic")
    );
    drop(sub);
    assert_eq!(s.source(), "(B)");
    assert_eq!(manager.undo_count(), 2);
    assert!(manager.undo().unwrap());
    assert_eq!(s.source(), "()");
    assert!(manager.undo().unwrap());
    assert_eq!(s.source(), "");
}

#[test]
fn derived_work_does_not_split_primary_coalescing_or_widen_a_group_over_another_trigger() {
    for list in [false, true] {
        let a = doc(1);
        let s = Seq::new(&a, list);
        let manager = UndoManager::new_identity(&a).unwrap();
        s.insert(0, "(");
        a.commit_then_renew();
        let trigger = manager.capture_trigger().unwrap();
        manager.group_start().unwrap();
        s.insert(1, "B");
        a.commit_then_renew();
        manager.with_derived(&trigger, || s.insert(2, ")")).unwrap();
        s.insert(2, "C");
        a.commit_then_renew();
        manager.group_end();
        assert_eq!(manager.undo_count(), 2);
        assert_eq!(s.source(), "(BC)");
        assert!(manager.undo().unwrap());
        assert_eq!(s.source(), "()");
        assert!(manager.undo().unwrap());
        assert_eq!(s.source(), "");
        assert!(manager.redo().unwrap());
        assert_eq!(s.source(), "()");
        assert!(manager.redo().unwrap());
        assert_eq!(s.source(), "(BC)");
    }
}

// A transport/resync boundary, not a fixture-supplied inactive-creation index.
// The producer's *real* no-op retirement is absent even from a full snapshot.
fn peer_retirement(list: bool, peer: u64, middle: &str, delivery: usize, retire: bool) -> String {
    let a = doc(peer);
    let b = doc(3 - peer);
    let s = Seq::new(&a, list);
    let other = Seq::new(&b, list);
    let undo_a = UndoManager::new_identity(&a).unwrap();
    s.insert(0, "a");
    a.commit_then_renew();
    let trigger = undo_a.capture_trigger().unwrap();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let undo_b = UndoManager::new_identity(&b).unwrap();
    other.insert(1, middle);
    b.commit_then_renew();
    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    undo_a
        .with_derived(&trigger, || {
            s.delete(1);
            s.insert(1, "x");
        })
        .unwrap();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(other.source(), "ax");
    assert_eq!((undo_a.undo_count(), undo_b.undo_count()), (1, 1));
    let vv = b.oplog_vv();
    let frontiers = b.oplog_frontiers();
    let state_frontiers = b.state_frontiers();
    let updates = b.export(ExportMode::all_updates()).unwrap();
    let snapshot = b.export(ExportMode::Snapshot).unwrap();
    let incremental = b.export(ExportMode::updates(&vv)).unwrap();
    let json = b
        .export_json_updates(&Default::default(), &vv, false)
        .unwrap();
    if retire {
        assert!(!undo_b.undo().unwrap());
        assert_eq!((undo_b.undo_count(), undo_b.redo_count()), (0, 0));
    }
    assert_eq!(other.source(), "ax");
    assert_eq!(b.oplog_vv(), vv);
    assert_eq!(b.oplog_frontiers(), frontiers);
    assert_eq!(b.state_frontiers(), state_frontiers);
    assert_eq!(b.export(ExportMode::all_updates()).unwrap(), updates);
    assert_eq!(b.export(ExportMode::Snapshot).unwrap(), snapshot);
    assert_eq!(b.export(ExportMode::updates(&vv)).unwrap(), incremental);
    let after_json = b
        .export_json_updates(&Default::default(), &b.oplog_vv(), false)
        .unwrap();
    assert_eq!(
        serde_json::to_value(&json).unwrap(),
        serde_json::to_value(&after_json).unwrap()
    );
    // A cold producer also cannot convey a semantic event absent from history.
    let cold = doc(10);
    cold.import(&snapshot).unwrap();
    UndoManager::new_identity(&cold).unwrap();
    assert_eq!(Seq::new(&cold, list).source(), "ax");
    match delivery {
        0 => {
            a.import(&b.export(ExportMode::all_updates()).unwrap())
                .unwrap();
        }
        1 => {
            a.import(&b.export(ExportMode::Snapshot).unwrap()).unwrap();
        }
        2 => {
            a.import_json_updates(after_json).unwrap();
        }
        3 => {
            a.import_batch(&[b.export(ExportMode::all_updates()).unwrap()])
                .unwrap();
        }
        4 => {
            a.import(&b.export(ExportMode::updates(&vv)).unwrap())
                .unwrap();
        }
        5 => {
            a.import(&cold.export(ExportMode::all_updates()).unwrap())
                .unwrap();
        }
        _ => unreachable!(),
    }
    assert!(undo_a.undo().unwrap());
    s.source()
}

#[test]
fn invisible_peer_retirement_is_absent_from_native_exports_and_cold_recovery() {
    for list in [false, true] {
        for peer in [1, 2] {
            for middle in ["B", "🦀"] {
                for delivery in 0..6 {
                    // Active creation must be revealed by undoing its overwriter.
                    assert_eq!(peer_retirement(list, peer, middle, delivery, false), middle);
                    // Observed information-loss control, NOT the desired outcome.
                    assert_eq!(peer_retirement(list, peer, middle, delivery, true), middle);
                }
            }
        }
    }
}

#[test]
#[ignore = "desired peer activity cannot cross the current operation/change-message protocol"]
fn invisible_peer_retirement_prevents_later_resurrection() {
    let mut failures = Vec::new();
    for list in [false, true] {
        for peer in [1, 2] {
            for middle in ["B", "🦀"] {
                for delivery in 0..6 {
                    let actual = peer_retirement(list, peer, middle, delivery, true);
                    if !actual.is_empty() {
                        failures.push(format!(
                            "list={list}, peer={peer}, middle={middle}, delivery={delivery}: {actual:?} != empty"
                        ));
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// A future activity protocol must also update an already-restored contribution.
// Unlike the byte-equality witness above, this fixture makes no assertion that
// retirement leaves VV/bytes unchanged, and exports an actual post-retirement
// snapshot. It can therefore remain a semantic oracle if activity is adopted.
fn retirement_after_restoration(
    list: bool,
    peer: u64,
    middle: &str,
    delivery: usize,
    retire: bool,
) -> [String; 3] {
    let a = doc(peer);
    let b = doc(3 - peer);
    let s = Seq::new(&a, list);
    let other = Seq::new(&b, list);
    let undo_a = UndoManager::new_identity(&a).unwrap();
    s.insert(0, "a");
    a.commit_then_renew();
    let trigger = undo_a.capture_trigger().unwrap();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let undo_b = UndoManager::new_identity(&b).unwrap();
    other.insert(1, middle);
    b.commit_then_renew();
    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    undo_a
        .with_derived(&trigger, || {
            s.delete(1);
            s.insert(1, "x");
        })
        .unwrap();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(other.source(), "ax");

    // A has not learned retirement: its inverse must reveal the active B.
    // Do not deliver that restoration to B until its hidden undo has run.
    assert!(undo_a.undo().unwrap());
    assert_eq!(s.source(), middle);
    if retire {
        assert!(!undo_b.undo().unwrap());
        assert_eq!((undo_b.undo_count(), undo_b.redo_count()), (0, 0));
    }
    assert_eq!(other.source(), "ax");
    let post_retirement_snapshot = b.export(ExportMode::Snapshot).unwrap();
    let cold_producer = doc(10);
    cold_producer.import(&post_retirement_snapshot).unwrap();
    UndoManager::new_identity(&cold_producer).unwrap();
    let a_version = a.oplog_vv();
    let mut once = None;
    for _ in 0..2 {
        match delivery {
            0 => {
                a.import(&b.export(ExportMode::all_updates()).unwrap())
                    .unwrap();
            }
            1 => {
                a.import(&post_retirement_snapshot).unwrap();
            }
            2 => {
                a.import_json_updates(
                    b.export_json_updates(&Default::default(), &b.oplog_vv(), false)
                        .unwrap(),
                )
                .unwrap();
            }
            3 => {
                a.import_batch(&[b.export(ExportMode::all_updates()).unwrap()])
                    .unwrap();
            }
            4 => {
                a.import(&b.export(ExportMode::updates(&a_version)).unwrap())
                    .unwrap();
            }
            5 => {
                a.import(&cold_producer.export(ExportMode::all_updates()).unwrap())
                    .unwrap();
            }
            _ => unreachable!(),
        }
        let observed = (
            s.source(),
            a.oplog_vv(),
            a.oplog_frontiers(),
            a.state_frontiers(),
        );
        if let Some(prior) = &once {
            assert_eq!(
                &observed, prior,
                "duplicate retirement delivery changed state/history"
            );
        } else {
            once = Some(observed);
        }
    }
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(a.oplog_vv(), b.oplog_vv());
    assert_eq!(a.oplog_frontiers(), b.oplog_frontiers());
    let cold_reader = doc(11);
    cold_reader
        .import(&a.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    UndoManager::new_identity(&cold_reader).unwrap();
    [
        s.source(),
        other.source(),
        Seq::new(&cold_reader, list).source(),
    ]
}

#[test]
fn late_retirement_active_control_preserves_restored_peer_contribution() {
    for list in [false, true] {
        for peer in [1, 2] {
            for middle in ["B", "🦀"] {
                for delivery in 0..6 {
                    assert_eq!(
                        retirement_after_restoration(list, peer, middle, delivery, false),
                        [middle.to_string(), middle.to_string(), middle.to_string()]
                    );
                }
            }
        }
    }
}

#[test]
#[ignore = "desired activity must also hide a contribution restored before retirement arrives"]
fn late_retirement_hides_already_restored_peer_contribution() {
    let mut failures = Vec::new();
    for list in [false, true] {
        for peer in [1, 2] {
            for middle in ["B", "🦀"] {
                for delivery in 0..6 {
                    let actual = retirement_after_restoration(list, peer, middle, delivery, true);
                    if actual != [String::new(), String::new(), String::new()] {
                        failures.push(format!(
                            "list={list}, peer={peer}, middle={middle}, delivery={delivery}: {actual:?} != empty on both peers and cold reader"
                        ));
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
