//! Preserve deletion intent invisible to state-diff transforms.
//!
//! Insertion provenance follows the same composition/transformation as text.
//! Filtering happens only after all span inverses have composed, so an earlier
//! omission cannot change the coordinate base expected by a following inverse.
use loro_common::{ContainerID, ContainerType, IdSpan, ID};
use loro_delta::{array_vec::ArrayVec, DeltaItem, DeltaRope};
use rle::HasLength;
use rustc_hash::{FxHashMap, FxHashSet};

use super::DiffBatch;
use crate::{
    container::list::list_op::InnerListOp,
    event::{Diff, TextDiff},
    op::InnerContent,
    utils::string_slice::StringSlice,
    version::Frontiers,
    LoroDoc,
};

pub(crate) type Suppressed = FxHashMap<ContainerID, FxHashSet<ID>>;

/// Read operations, not state events: a concurrent delete can have no visible
/// effect. Only inspect history after the span's dependency version and only
/// retain IDs actually deleted by this local span.
pub(crate) fn suppressed(doc: &LoroDoc, spans: &[(IdSpan, Frontiers)]) -> Suppressed {
    let (span, deps) = &spans[0];
    let oplog = doc.oplog.lock();
    if oplog.is_shallow() {
        return Default::default();
    }
    let mut targets: Suppressed = Default::default();
    for op in spans.iter().flat_map(|(span, _)| oplog.iter_ops(*span)) {
        if op.container().get_type() != ContainerType::Text {
            continue;
        }
        if let InnerContent::List(InnerListOp::Delete(deletion)) = &op.op().content {
            let cid = doc
                .arena
                .get_container_id(op.container())
                .expect("text history container");
            let ids = targets.entry(cid).or_default();
            for offset in 0..deletion.atom_len() {
                ids.insert(ID::new(
                    deletion.id_start.peer,
                    deletion.id_start.counter + offset as i32,
                ));
            }
        }
    }
    if targets.is_empty() {
        return targets;
    }
    let before = oplog
        .dag
        .frontiers_to_vv(deps)
        .expect("undo span dependencies");
    let mut suppressed: Suppressed = Default::default();
    for range in before.diff_iter(oplog.vv()).1 {
        // Local operations outside this span can include earlier inverses.
        // This scoped repair concerns other peers' deletion intent only.
        if range.peer == span.peer {
            continue;
        }
        for op in oplog.iter_ops(range) {
            if op.container().get_type() != ContainerType::Text {
                continue;
            }
            let cid = doc
                .arena
                .get_container_id(op.container())
                .expect("text history container");
            let Some(targets) = targets.get(&cid) else {
                continue;
            };
            if let InnerContent::List(InnerListOp::Delete(deletion)) = &op.op().content {
                for offset in 0..deletion.atom_len() {
                    let id = ID::new(
                        deletion.id_start.peer,
                        deletion.id_start.counter + offset as i32,
                    );
                    if targets.contains(&id) {
                        suppressed.entry(cid.clone()).or_default().insert(id);
                    }
                }
            }
        }
    }
    suppressed
}

type Marks = DeltaRope<ArrayVec<bool, 128>, ()>;

/// Private undo-calculation carrier. DiffBatch's public representation stays
/// unchanged. Each inserted event unit records whether its original ID also
/// has a foreign deletion; retain/delete lengths match the native text delta.
pub(crate) struct Annotated {
    pub(crate) batch: DiffBatch,
    marks: FxHashMap<ContainerID, Marks>,
}

fn event_width(ch: char) -> usize {
    if cfg!(feature = "wasm") {
        ch.len_utf16()
    } else {
        1
    }
}

fn mark_shape(delta: &TextDiff, positions: &FxHashSet<usize>) -> Marks {
    let mut marks = Marks::new();
    let mut output = 0;
    for item in delta.iter() {
        match item {
            DeltaItem::Retain { len, .. } => {
                marks.push_retain(*len, ());
                output += len;
            }
            DeltaItem::Replace { value, delete, .. } => {
                let mut inserted = Vec::new();
                for ch in value.as_str().chars() {
                    let width = event_width(ch);
                    inserted.extend(std::iter::repeat_n(positions.contains(&output), width));
                    output += width;
                }
                for chunk in ArrayVec::from_many(inserted.into_iter()) {
                    marks.push_insert(chunk, ());
                }
                marks.push_delete(*delete);
            }
        }
    }
    marks
}

impl Annotated {
    /// The document is checked out at the delta's output version. Only inverse
    /// deltas need inserted-ID provenance; forward bridge deltas supply shape.
    pub(crate) fn capture(
        doc: &LoroDoc,
        batch: DiffBatch,
        suppressed: &Suppressed,
        inverse: bool,
    ) -> Self {
        let mut marks = FxHashMap::default();
        if inverse {
            for (cid, ids) in suppressed {
                let Some(Diff::Text(delta)) = batch.cid_to_events.get(cid) else {
                    continue;
                };
                let idx = doc.arena.id_to_idx(cid).expect("text diff container");
                let positions = doc.state.lock().with_state(idx, |state| {
                    let text = state.as_richtext_state().expect("text state");
                    ids.iter()
                        .filter_map(|id| text.get_text_index_of_id(*id, true))
                        .collect()
                });
                marks.insert(cid.clone(), mark_shape(delta, &positions));
            }
        }
        Self { batch, marks }
    }

    pub(crate) fn compose(&mut self, other: &Self) {
        // Unmarked deltas still supply positional shape: they can delete,
        // retain or shift an earlier marked insertion.
        let cids: FxHashSet<_> = self
            .marks
            .keys()
            .chain(other.marks.keys())
            .cloned()
            .collect();
        for cid in cids {
            let mut first = self.marks.remove(&cid).unwrap_or_else(|| {
                match self.batch.cid_to_events.get(&cid) {
                    Some(Diff::Text(delta)) => mark_shape(delta, &Default::default()),
                    _ => Marks::new(),
                }
            });
            if let Some(second) = other.marks.get(&cid) {
                first.compose(second);
            } else if let Some(Diff::Text(delta)) = other.batch.cid_to_events.get(&cid) {
                first.compose(&mark_shape(delta, &Default::default()));
            }
            self.marks.insert(cid, first);
        }
        self.batch.compose(&other.batch);
    }

    pub(crate) fn transform(&mut self, other: &DiffBatch, left: bool) {
        for (cid, marks) in &mut self.marks {
            if let Some(Diff::Text(delta)) = other.cid_to_events.get(cid) {
                marks.transform_(&mark_shape(delta, &Default::default()), left);
            }
        }
        self.batch.transform(other, left);
    }

    pub(crate) fn filter_restorations(&mut self) {
        for (cid, marks) in &self.marks {
            let Some(Diff::Text(delta)) = self.batch.cid_to_events.get_mut(cid) else {
                panic!("text provenance lost its delta");
            };
            // Output positions survive commuting adjacent insert/delete
            // chunks during normalization; their input positions need not.
            let mut output = 0;
            let flags: Vec<_> = marks
                .iter()
                .flat_map(|item| match item {
                    DeltaItem::Replace { value, .. } => {
                        let position = output;
                        output += value.len();
                        value
                            .iter()
                            .enumerate()
                            .map(|(offset, flag)| (position + offset, *flag))
                            .collect::<Vec<_>>()
                    }
                    DeltaItem::Retain { len, .. } => {
                        output += len;
                        Vec::new()
                    }
                })
                .collect();
            let mut flags = flags.into_iter();
            let mut filtered = TextDiff::new();
            let mut output = 0;
            for item in delta.iter() {
                match item {
                    DeltaItem::Retain { len, attr } => {
                        filtered.push_retain(*len, attr.clone());
                        output += len;
                    }
                    DeltaItem::Replace {
                        value,
                        attr,
                        delete,
                    } => {
                        for ch in value.as_str().chars() {
                            let (position, suppress) =
                                flags.next().expect("missing text insertion provenance");
                            assert_eq!(position, output, "misplaced text insertion provenance");
                            for offset in 1..event_width(ch) {
                                assert_eq!(
                                    flags.next(),
                                    Some((output + offset, suppress)),
                                    "split scalar provenance"
                                );
                            }
                            if !suppress {
                                filtered.push_insert(
                                    StringSlice::from(ch.to_string().as_str()),
                                    attr.clone(),
                                );
                            }
                            output += event_width(ch);
                        }
                        filtered.push_delete(*delete);
                    }
                }
            }
            assert!(flags.next().is_none(), "extra text insertion provenance");
            *delta = filtered;
        }
        self.marks.clear();
    }
}
