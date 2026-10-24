//! Replays h's delivery/placement transcript against this exact native fork.
use loro::ToJson;
use loro::{ExportMode, LoroDoc, LoroList, LoroMovableList, LoroText, UndoManager};
use std::sync::{Arc, Mutex};

enum Sequence {
    Text(LoroText),
    List(LoroList),
    Movable(LoroMovableList),
}
impl Sequence {
    fn new(doc: &LoroDoc, kind: &str) -> Self {
        match kind {
            "text" => Self::Text(doc.get_text("sequence")),
            "list" => Self::List(doc.get_list("sequence")),
            _ => Self::Movable(doc.get_movable_list("sequence")),
        }
    }
    fn insert(&self, at: usize, value: &str) {
        match self {
            Self::Text(t) => t.insert(at, value).unwrap(),
            Self::List(t) => t.insert(at, value).unwrap(),
            Self::Movable(t) => t.insert(at, value).unwrap(),
        }
    }
    fn delete(&self, at: usize) {
        match self {
            Self::Text(t) => t.delete(at, 1).unwrap(),
            Self::List(t) => t.delete(at, 1).unwrap(),
            Self::Movable(t) => t.delete(at, 1).unwrap(),
        }
    }
    fn source(&self) -> String {
        let value = match self {
            Self::Text(t) => return t.to_string(),
            Self::List(t) => t.get_deep_value(),
            Self::Movable(t) => t.get_deep_value(),
        };
        value
            .to_json_value()
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect()
    }
}
fn deliver(from: &LoroDoc, to: &LoroDoc) {
    to.import(&from.export(ExportMode::Snapshot).unwrap())
        .unwrap();
}
fn pair() -> (LoroDoc, LoroDoc) {
    let a = LoroDoc::new();
    let b = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    b.set_peer_id(2).unwrap();
    a.set_record_timestamp(false);
    b.set_record_timestamp(false);
    (a, b)
}
fn convergence(a: &LoroDoc, b: &LoroDoc) {
    deliver(a, b);
    deliver(b, a);
    assert_eq!(a.get_deep_value(), b.get_deep_value());
    let fresh = LoroDoc::new();
    deliver(a, &fresh);
    assert_eq!(a.get_deep_value(), fresh.get_deep_value());
}
#[derive(Debug)]
struct Delivery {
    before: String,
    version: Vec<(u64, i32)>,
    after: String,
}
fn batched(kind: &str, incremental: bool) -> Delivery {
    let (a, b) = pair();
    let local = Sequence::new(&a, kind);
    let other = Sequence::new(&b, kind);
    local.insert(0, "a");
    local.insert(1, "b");
    a.commit();
    deliver(&a, &b);
    local.insert(1, "x");
    a.commit();
    other.insert(0, "r");
    b.commit();
    deliver(&b, &a);
    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(0);
    local.delete(1);
    a.commit();
    other.delete(0);
    b.commit();
    if incremental {
        deliver(&b, &a);
    }
    other.delete(1);
    b.commit();
    if incremental {
        deliver(&b, &a);
    }
    other.insert(0, "s");
    b.commit();
    deliver(&b, &a);
    let before = local.source();
    let mut version: Vec<_> = a.oplog_vv().iter().map(|(p, c)| (*p, *c)).collect();
    version.sort();
    assert!(undo.undo().unwrap());
    let after = local.source();
    convergence(&a, &b);
    Delivery {
        before,
        version,
        after,
    }
}
fn tie(kind: &str, at: usize) -> (Vec<String>, String) {
    let (a, b) = pair();
    let local = Sequence::new(&a, kind);
    let other = Sequence::new(&b, kind);
    local.insert(0, "a");
    local.insert(1, "b");
    a.commit();
    deliver(&a, &b);
    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(0);
    local.delete(0);
    a.commit();
    other.insert(at, "s");
    b.commit();
    let diffs = Arc::new(Mutex::new(Vec::new()));
    let capture = diffs.clone();
    let subscription = a.subscribe_root(Arc::new(move |event| {
        for e in event.events {
            capture.lock().unwrap().push(format!("{:?}", e.diff));
        }
    }));
    deliver(&b, &a);
    drop(subscription);
    assert_eq!(local.source(), "sb");
    let diffs = diffs.lock().unwrap().clone();
    assert!(undo.undo().unwrap());
    let after = local.source();
    convergence(&a, &b);
    (diffs, after)
}
#[test]
fn native_observations_and_convergence() {
    for kind in ["list", "text", "movable"] {
        let batch = batched(kind, false);
        let incremental = batched(kind, true);
        eprintln!("{kind}: batch={batch:?}; incremental={incremental:?}");
        assert_eq!(batch.before, "sx");
        assert_eq!(batch.before, incremental.before);
        assert_eq!(batch.version, incremental.version);
        let before = tie(kind, 0);
        let after = tie(kind, 1);
        eprintln!("{kind}: s-before-a={before:?}; s-after-a={after:?}");
        assert_eq!(before.0, after.0, "projected import diffs coincide");
    }
}
#[test]
#[ignore = "known native List undo delivery partition dependence"]
fn list_undo_is_independent_of_delivery_partition() {
    assert_eq!(batched("list", false).after, batched("list", true).after);
}
#[test]
#[ignore = "desired text tombstone before/after placement is not recovered from projected diffs"]
fn text_restore_distinguishes_both_sides_of_deleted_a() {
    assert_eq!(
        (tie("text", 0).1, tie("text", 1).1),
        ("sab".to_string(), "asb".to_string())
    );
}
