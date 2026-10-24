//! Preserve deletion intent invisible to state-diff transforms.
//!
//! This first path handles one continuous inverse span. Filtering an
//! intermediate output in a multi-span composition would change the coordinate
//! base expected by the next inverse, so that case keeps the existing path.
use loro_common::{ContainerID, ContainerType, IdSpan, ID};
use loro_delta::DeltaItem;
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
    if spans.len() != 1 {
        return Default::default();
    }
    let (span, deps) = &spans[0];
    let oplog = doc.oplog.lock();
    if oplog.is_shallow() {
        return Default::default();
    }
    let mut targets: Suppressed = Default::default();
    for op in oplog.iter_ops(*span) {
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

/// Called while checked out at the inverse's output version. Positions are
/// output/event coordinates, never byte or entity coordinates. Preserve all
/// deletion lengths and attributes; suppress only restored characters whose
/// original IDs also have a foreign deletion.
pub(crate) fn filter(doc: &LoroDoc, diff: &mut DiffBatch, suppressed: &Suppressed) {
    for (cid, ids) in suppressed {
        let Some(Diff::Text(delta)) = diff.cid_to_events.get_mut(cid) else {
            continue;
        };
        let Some(idx) = doc.arena.id_to_idx(cid) else {
            continue;
        };
        let positions: FxHashSet<usize> = doc.state.lock().with_state(idx, |state| {
            let text = state.as_richtext_state().expect("text state");
            ids.iter()
                .filter_map(|id| text.get_text_index_of_id(*id, true))
                .collect()
        });
        if positions.is_empty() {
            continue;
        }
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
                        if !positions.contains(&output) {
                            filtered.push_insert(
                                StringSlice::from(ch.to_string().as_str()),
                                attr.clone(),
                            );
                        }
                        output += if cfg!(feature = "wasm") {
                            ch.len_utf16()
                        } else {
                            1
                        };
                    }
                    filtered.push_delete(*delete);
                }
            }
        }
        *delta = filtered;
    }
}
