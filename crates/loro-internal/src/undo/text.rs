//! Preserve deletion intent invisible to state-diff transforms.
//!
//! Insertion provenance follows the same composition/transformation as text.
//! Filtering happens only after all span inverses have composed, so an earlier
//! omission cannot change the coordinate base expected by a following inverse.
use loro_common::{ContainerID, ContainerType, IdSpan, LoroError, LoroResult, ID};
use loro_delta::{array_vec::ArrayVec, DeltaItem, DeltaRope};
use rle::HasLength;
use rustc_hash::{FxHashMap, FxHashSet};

use super::DiffBatch;
use crate::{
    container::list::list_op::InnerListOp,
    cursor::Side,
    event::{Diff, TextDiff},
    handler::TextHandler,
    op::InnerContent,
    utils::string_slice::StringSlice,
    version::Frontiers,
    HandlerTrait, LoroDoc,
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Mark {
    /// The physical creation ID seen at the raw inverse's output frontier.
    /// This is not yet a transitive logical restoration lineage.
    origin: Option<ID>,
    suppressed: bool,
}

type Marks = DeltaRope<ArrayVec<Mark, 128>, ()>;

/// Private undo-calculation carrier. DiffBatch's public representation stays
/// unchanged. Each inserted event unit retains its creation ID at the raw
/// inverse output and whether that ID also has a foreign deletion. Both fields
/// follow native compose/transform; retain/delete lengths match the text delta.
pub(crate) struct Annotated {
    pub(crate) batch: DiffBatch,
    marks: FxHashMap<ContainerID, Marks>,
    // Identity capture alone must not rebuild/normalize an otherwise untouched
    // text delta. Keep the pre-existing suppression path's container scope.
    filterable: FxHashSet<ContainerID>,
}

fn event_width(ch: char) -> usize {
    if cfg!(feature = "wasm") {
        ch.len_utf16()
    } else {
        1
    }
}

fn mark_shape(delta: &TextDiff, positions: &FxHashMap<usize, Mark>) -> Marks {
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
                    inserted.extend(std::iter::repeat_n(
                        positions.get(&output).copied().unwrap_or_default(),
                        width,
                    ));
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
        let mut filterable = FxHashSet::default();
        if inverse {
            for (cid, diff) in &batch.cid_to_events {
                let Diff::Text(delta) = diff else {
                    continue;
                };
                if suppressed.contains_key(cid) {
                    filterable.insert(cid.clone());
                }
                let idx = doc.arena.id_to_idx(cid).expect("text diff container");
                let positions = doc.state.lock().with_state_mut(idx, |state| {
                    let text = state.as_richtext_state_mut().expect("text state");
                    let mut output = 0;
                    let mut positions = FxHashMap::default();
                    for item in delta.iter() {
                        match item {
                            DeltaItem::Retain { len, .. } => output += len,
                            DeltaItem::Replace { value, .. } => {
                                for ch in value.as_str().chars() {
                                    let origin = text.get_stable_position(output, true);
                                    positions.insert(
                                        output,
                                        Mark {
                                            origin,
                                            suppressed: origin.is_some_and(|id| {
                                                suppressed
                                                    .get(cid)
                                                    .is_some_and(|ids| ids.contains(&id))
                                            }),
                                        },
                                    );
                                    output += event_width(ch);
                                }
                            }
                        }
                    }
                    positions
                });
                marks.insert(cid.clone(), mark_shape(delta, &positions));
            }
        }
        Self {
            batch,
            marks,
            filterable,
        }
    }

    pub(crate) fn compose(&mut self, other: &Self) {
        self.filterable.extend(other.filterable.iter().cloned());
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
        // Native transform moves our insertions but never adds, drops or
        // reorders their units. Keep their identity stream, then use the ACTUAL
        // transformed text's positions. Re-encoding `other` as an ArrayVec
        // delta can commute adjacent delete/insert chunks that StringSlice
        // kept separate; those equivalent state edits need not give the same
        // OT placement. Do not calculate placement a second time with that
        // differently normalized representation.
        let streams: FxHashMap<_, Vec<_>> = self
            .marks
            .iter()
            .map(|(cid, marks)| {
                let units = marks
                    .iter()
                    .filter_map(|item| match item {
                        DeltaItem::Replace { value, .. } => Some(value.iter().copied()),
                        DeltaItem::Retain { .. } => None,
                    })
                    .flatten()
                    .collect();
                (cid.clone(), units)
            })
            .collect();
        self.batch.transform(other, left);
        for (cid, stream) in streams {
            let Some(Diff::Text(delta)) = self.batch.cid_to_events.get(&cid) else {
                panic!("transformed restoration origins lost their text delta");
            };
            let mut stream = stream.into_iter();
            let mut positions = FxHashMap::default();
            let mut output = 0;
            for item in delta.iter() {
                match item {
                    DeltaItem::Retain { len, .. } => output += len,
                    DeltaItem::Replace { value, .. } => {
                        for ch in value.as_str().chars() {
                            let mark = stream
                                .next()
                                .expect("native transform added insertion units");
                            for _ in 1..event_width(ch) {
                                assert_eq!(stream.next(), Some(mark), "transformed split scalar");
                            }
                            positions.insert(output, mark);
                            output += event_width(ch);
                        }
                    }
                }
            }
            assert!(
                stream.next().is_none(),
                "native transform dropped insertion units"
            );
            self.marks.insert(cid, mark_shape(delta, &positions));
        }
    }

    pub(crate) fn filter_restorations(&mut self) {
        for (cid, marks) in &mut self.marks {
            if !self.filterable.contains(cid) {
                continue;
            }
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
            let mut filtered_marks = Marks::new();
            let mut output = 0;
            for item in delta.iter() {
                match item {
                    DeltaItem::Retain { len, attr } => {
                        filtered.push_retain(*len, attr.clone());
                        filtered_marks.push_retain(*len, ());
                        output += len;
                    }
                    DeltaItem::Replace {
                        value,
                        attr,
                        delete,
                    } => {
                        for ch in value.as_str().chars() {
                            let (position, mark) =
                                flags.next().expect("missing text insertion provenance");
                            assert_eq!(position, output, "misplaced text insertion provenance");
                            for offset in 1..event_width(ch) {
                                assert_eq!(
                                    flags.next(),
                                    Some((output + offset, mark)),
                                    "split scalar provenance"
                                );
                            }
                            if !mark.suppressed {
                                filtered.push_insert(
                                    StringSlice::from(ch.to_string().as_str()),
                                    attr.clone(),
                                );
                                for chunk in
                                    ArrayVec::from_many(std::iter::repeat_n(mark, event_width(ch)))
                                {
                                    filtered_marks.push_insert(chunk, ());
                                }
                            }
                            output += event_width(ch);
                        }
                        filtered.push_delete(*delete);
                        filtered_marks.push_delete(*delete);
                    }
                }
            }
            assert!(flags.next().is_none(), "extra text insertion provenance");
            *delta = filtered;
            *marks = filtered_marks;
        }
    }

    fn calculated_restorations(&self) -> Vec<(ContainerID, usize, Option<ID>)> {
        let mut answer = Vec::new();
        for cid in &self.batch.order {
            let Some(marks) = self.marks.get(cid) else {
                continue;
            };
            let Some(Diff::Text(delta)) = self.batch.cid_to_events.get(cid) else {
                panic!("restoration origins lost their text delta");
            };
            let mut output = 0;
            let mut units = Vec::new();
            for item in marks.iter() {
                match item {
                    DeltaItem::Retain { len, .. } => output += len,
                    DeltaItem::Replace { value, .. } => {
                        units.extend(
                            value
                                .iter()
                                .enumerate()
                                .map(|(i, mark)| (output + i, *mark)),
                        );
                        output += value.len();
                    }
                }
            }
            let mut units = units.into_iter();
            output = 0;
            for item in delta.iter() {
                match item {
                    DeltaItem::Retain { len, .. } => output += len,
                    DeltaItem::Replace { value, .. } => {
                        for ch in value.as_str().chars() {
                            let (position, mark) = units.next().expect("missing final origin");
                            assert_eq!(position, output, "misplaced final origin");
                            assert!(!mark.suppressed, "suppressed origin survived filtering");
                            for offset in 1..event_width(ch) {
                                assert_eq!(units.next(), Some((output + offset, mark)));
                            }
                            answer.push((cid.clone(), output, mark.origin));
                            output += event_width(ch);
                        }
                    }
                }
            }
            assert!(units.next().is_none(), "extra final origin");
        }
        answer
    }
}

/// Evidence from a successfully applied text handler. `origin` is a raw
/// inverse-output creation ID, not yet a transitive logical identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AppliedRestoration {
    pub(crate) source_container: ContainerID,
    pub(crate) target_container: ContainerID,
    /// Event position immediately after this handler applied, not a promise
    /// about its position after future edits or later aliased batch entries.
    pub(crate) output: usize,
    pub(crate) origin: Option<ID>,
    pub(crate) restored: ID,
}

/// Private provisional receipts. Only the successful inverse's commit guard
/// receives them; an application/receipt error drops them with its rollback.
pub(crate) struct PendingRestorations {
    origins: FxHashMap<ContainerID, Vec<(usize, Option<ID>)>>,
    fresh_start: ID,
    applied: Vec<AppliedRestoration>,
}

impl PendingRestorations {
    pub(crate) fn new(doc: &LoroDoc, computed: &Annotated) -> Self {
        let peer = doc.peer_id();
        let counter = doc.oplog.lock().vv().get(&peer).copied().unwrap_or(0);
        let mut origins = FxHashMap::<_, Vec<_>>::default();
        for (cid, output, origin) in computed.calculated_restorations() {
            origins.entry(cid).or_default().push((output, origin));
        }
        #[cfg(test)]
        if POINT_RECEIPT_AT_RETAINED_TEXT.with(|flag| flag.replace(false)) {
            origins.values_mut().next().unwrap()[0].0 = 0;
        }
        Self {
            origins,
            fresh_start: ID::new(peer, counter),
            applied: Vec::new(),
        }
    }

    pub(crate) fn record(
        &mut self,
        source_container: &ContainerID,
        text: &TextHandler,
    ) -> LoroResult<()> {
        let Some(origins) = self.origins.remove(source_container) else {
            return Ok(());
        };
        for (output, origin) in origins {
            let restored = text
                .get_cursor(output, Side::Middle)
                .and_then(|cursor| cursor.id)
                .ok_or_else(|| {
                    LoroError::TransactionError("missing applied restoration ID".into())
                })?;
            if restored.peer != self.fresh_start.peer || restored.counter < self.fresh_start.counter
            {
                return Err(LoroError::TransactionError(
                    "restoration receipt names an existing element".into(),
                ));
            }
            self.applied.push(AppliedRestoration {
                source_container: source_container.clone(),
                target_container: text.id(),
                output,
                origin,
                restored,
            });
        }
        Ok(())
    }

    pub(crate) fn finish(self) -> Vec<AppliedRestoration> {
        // Unconsumed entries correspond to containers skipped by the native
        // reachability/remapping rules. They must not fabricate receipts.
        self.applied
    }
}

// Scoped observation of the *real* native calculation, before its normal
// application. This neither supplies identities nor changes an inverse.
#[cfg(test)]
thread_local! {
    static CALCULATED: std::cell::RefCell<Option<Vec<(ContainerID, usize, Option<ID>)>>> = const { std::cell::RefCell::new(None) };
    pub(crate) static POINT_RECEIPT_AT_RETAINED_TEXT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn record_calculated_restorations(computed: &Annotated) {
    CALCULATED.with(|slot| {
        if let Some(origins) = slot.borrow_mut().as_mut() {
            assert!(
                origins.is_empty(),
                "multiple native calculations in one observation"
            );
            *origins = computed.calculated_restorations();
        }
    });
}

#[cfg(test)]
pub(crate) fn with_calculated_restorations<T>(
    f: impl FnOnce() -> T,
) -> (T, Vec<(ContainerID, usize, Option<ID>)>) {
    struct Clear;
    impl Drop for Clear {
        fn drop(&mut self) {
            CALCULATED.with(|slot| *slot.borrow_mut() = None);
        }
    }
    CALCULATED.with(|slot| {
        assert!(slot.borrow().is_none(), "nested restoration observation");
        *slot.borrow_mut() = Some(Vec::new());
    });
    let _clear = Clear;
    let result = f();
    let origins = CALCULATED.with(|slot| slot.borrow_mut().take().unwrap());
    (result, origins)
}
