//! Experimental identity placement for native plain Text/List history.
//! The ordinary manager does not enter this path; diagnostic TLS is test-only.
use super::text::{AppliedRestoration, PendingRestorations};
use super::DiffBatch;
use crate::{
    container::list::list_op::InnerListOp,
    cursor::Side,
    event::{Diff, ListDiff, ListDiffInsertItem, TextDiff},
    handler::ValueOrHandler,
    op::InnerContent,
    utils::string_slice::StringSlice,
    LoroDoc,
};
use loro_common::{ContainerType, Counter, HasId, IdSpan, LoroError, LoroResult, LoroValue, ID};
use loro_delta::DeltaItem;
#[cfg(test)]
use std::cell::RefCell;

pub(crate) fn candidate_order(
    physical: &[ID],
    inserts: &[crate::diff_calc::HistoricalInsertion],
    receipts: &[super::text::AppliedRestoration],
) -> Option<Vec<ID>> {
    let mut parents = rustc_hash::FxHashMap::default();
    for edge in receipts {
        let parent = edge.origin?;
        if parents
            .insert(edge.restored, parent)
            .is_some_and(|old| old != parent)
        {
            return None;
        }
    }
    let root = |mut id| {
        let mut traversed = 0;
        while let Some(parent) = parents.get(&id).copied() {
            traversed += 1;
            assert!(traversed <= receipts.len(), "restoration cycle");
            id = parent;
        }
        id
    };
    let mut nodes = Vec::new();
    let mut indices = rustc_hash::FxHashMap::default();
    for id in physical {
        let id = root(*id);
        if !indices.contains_key(&id) {
            indices.insert(id, nodes.len());
            nodes.push(id);
        }
    }
    let mut edges = Vec::new();
    for insert in inserts {
        for offset in 0..insert.len {
            let id = insert.id.inc(offset as Counter);
            // A physical restoration carries identity, not a new logical
            // insertion constraint from its accidentally chosen physical gap.
            if root(id) != id {
                continue;
            }
            let left = if offset == 0 {
                insert.left
            } else {
                Some(id.inc(-1))
            };
            if let Some(left) = left {
                edges.push((root(left), id));
            }
            if offset + 1 == insert.len {
                if let Some(right) = insert.right {
                    edges.push((id, root(right)));
                }
            }
        }
    }
    let mut unique = rustc_hash::FxHashSet::default();
    let mut successors = vec![Vec::new(); nodes.len()];
    let mut indegree = vec![0usize; nodes.len()];
    for (a, b) in edges {
        let a = *indices.get(&a)?;
        let b = *indices.get(&b)?;
        if unique.insert((a, b)) {
            successors[a].push(b);
            indegree[b] += 1;
        }
    }
    // Same native physical priority as the earlier linear scan: always take
    // the lowest original index among nodes whose predecessors are complete.
    let mut ready = std::collections::BTreeSet::new();
    for (i, n) in indegree.iter().enumerate() {
        if *n == 0 {
            ready.insert(i);
        }
    }
    let mut answer = Vec::new();
    while let Some(i) = ready.pop_first() {
        answer.push(nodes[i]);
        for next in &successors[i] {
            indegree[*next] -= 1;
            if indegree[*next] == 0 {
                ready.insert(*next);
            }
        }
    }
    (answer.len() == nodes.len()).then_some(answer)
}

#[cfg(test)]
thread_local! {
    static RECEIPTS: RefCell<Option<Vec<AppliedRestoration>>> = const { RefCell::new(None) };
    static ALLOW_MANAGER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Diagnostic scope only: leave the manager's cached rebase unchanged, and
/// retain actual successful receipts for subsequent admitted insertions in this
/// scope. No production opt-in or persistent manager history is provided.
#[cfg(test)]
pub(crate) fn with_manager_probe<T>(receipts: &[AppliedRestoration], f: impl FnOnce() -> T) -> T {
    struct Clear;
    impl Drop for Clear {
        fn drop(&mut self) {
            ALLOW_MANAGER.with(|slot| slot.set(false));
        }
    }
    ALLOW_MANAGER.with(|slot| assert!(!slot.replace(true)));
    let _clear = Clear;
    with_receipts(receipts, f)
}

#[cfg(test)]
pub(crate) fn manager_receipts() -> Vec<AppliedRestoration> {
    RECEIPTS.with(|slot| {
        slot.borrow()
            .as_ref()
            .expect("unarmed receipt query")
            .clone()
    })
}

/// Called by the real manager only after dropping a successful commit guard.
/// Failed attempts never reach this point or publish provisional receipt IDs.
#[cfg(test)]
pub(crate) fn retain_manager_receipts(receipts: &[AppliedRestoration]) {
    if ALLOW_MANAGER.with(|slot| slot.get()) {
        RECEIPTS.with(|slot| {
            slot.borrow_mut()
                .as_mut()
                .expect("unarmed manager probe")
                .extend_from_slice(receipts);
        });
    }
}

#[cfg(test)]
pub(crate) fn with_receipts<T>(receipts: &[AppliedRestoration], f: impl FnOnce() -> T) -> T {
    struct Clear;
    impl Drop for Clear {
        fn drop(&mut self) {
            RECEIPTS.with(|slot| *slot.borrow_mut() = None);
        }
    }
    RECEIPTS.with(|slot| {
        assert!(slot.borrow().is_none(), "nested placement probe");
        *slot.borrow_mut() = Some(receipts.to_vec());
    });
    let _clear = Clear;
    f()
}

fn unsupported() -> LoroError {
    LoroError::TransactionError(
        "identity candidate requires one plain Text or List item with full history".into(),
    )
}

/// Runs only when explicitly armed by a test. The complete native inverse has
/// already been calculated and the current head restored. Plan all admitted
/// insertions before the ordinary handler runs in the rollback scope.
#[cfg(test)]
pub(crate) fn maybe_rewrite(
    doc: &LoroDoc,
    diff: &mut DiffBatch,
    pending: &mut PendingRestorations,
    has_manager_transform: bool,
) -> LoroResult<()> {
    let receipts = RECEIPTS.with(|slot| slot.borrow().clone());
    let Some(receipts) = receipts else {
        return Ok(());
    };
    if (has_manager_transform && !ALLOW_MANAGER.with(|slot| slot.get()))
        || diff.order.len() != 1
        || diff.cid_to_events.len() != 1
        || diff.full_state
    {
        return Err(unsupported());
    }
    rewrite(doc, diff, pending, &receipts, None)
}

pub(crate) fn rewrite(
    doc: &LoroDoc,
    diff: &mut DiffBatch,
    pending: &mut PendingRestorations,
    receipts: &[AppliedRestoration],
    selected: Option<&[IdSpan]>,
) -> LoroResult<()> {
    let mixed = selected.is_some();
    if diff.order.is_empty() && diff.cid_to_events.is_empty() && mixed {
        return Ok(());
    }
    if diff.order.len() != 1 || diff.cid_to_events.len() != 1 || diff.full_state {
        return Err(unsupported());
    }
    let cid = diff.order[0].clone();
    let input = diff.cid_to_events.get(&cid).ok_or_else(unsupported)?;
    let is_list = matches!(input, Diff::List(_)) && cid.container_type() == ContainerType::List;
    if !is_list && !matches!(input, Diff::Text(_)) {
        return Err(unsupported());
    }
    let origins = pending.origins_for(&cid).ok_or_else(unsupported)?;
    let mut payloads = Vec::new();
    let mut output = 0;
    for item in pieces(input)? {
        match item {
            Piece::Retain(len) => output += len,
            Piece::Edit { cells, delete } if mixed || delete == 0 => {
                for cell in cells {
                    let (position, origin) = origins.get(payloads.len()).ok_or_else(unsupported)?;
                    if *position != output {
                        return Err(unsupported());
                    }
                    output += cell.width();
                    payloads.push((origin.ok_or_else(unsupported)?, cell));
                }
            }
            _ => return Err(unsupported()),
        }
    }
    if (!mixed && payloads.is_empty()) || payloads.len() != origins.len() {
        return Err(unsupported());
    }
    let receipts: Vec<_> = receipts
        .iter()
        .filter(|edge| edge.source_container == cid || edge.target_container == cid)
        .cloned()
        .collect();
    if receipts.iter().any(|edge| {
        edge.source_container != cid || edge.target_container != cid || edge.origin.is_none()
    }) {
        return Err(unsupported());
    }
    let (physical, insertions, creations) = {
        let oplog = doc.oplog().lock();
        if oplog.is_shallow() {
            return Err(unsupported());
        }
        let idx = doc.arena.id_to_idx(&cid).ok_or_else(unsupported)?;
        for (&peer, &end) in oplog.vv().iter() {
            for op in oplog.iter_ops(loro_common::IdSpan::new(peer, 0, end)) {
                if op.container() == idx {
                    match &op.op().content {
                        InnerContent::List(InnerListOp::InsertText { .. }) if !is_list => {}
                        InnerContent::List(InnerListOp::Insert { .. }) if is_list => {}
                        InnerContent::List(InnerListOp::Delete(_)) => {}
                        _ => return Err(unsupported()),
                    }
                }
            }
        }
        let mut creations = Vec::new();
        for span in selected.unwrap_or(&[]) {
            for op in oplog.iter_ops(*span) {
                if op.container() != idx {
                    return Err(unsupported());
                }
                match &op.op().content {
                    InnerContent::List(
                        InnerListOp::InsertText { .. } | InnerListOp::Insert { .. },
                    ) => {
                        creations.extend(
                            (0..op.end - op.start).map(|i| op.id_start().inc(i as Counter)),
                        );
                    }
                    InnerContent::List(InnerListOp::Delete(_)) => {}
                    _ => return Err(unsupported()),
                }
            }
        }
        (
            if is_list {
                crate::diff_calc::historical_list_id_order(&oplog, &cid)
            } else {
                crate::diff_calc::historical_text_id_order(&oplog, &cid)
            },
            if is_list {
                crate::diff_calc::historical_list_insertions(&oplog, &cid)
            } else {
                crate::diff_calc::historical_text_insertions(&oplog, &cid)
            },
            creations,
        )
    };
    let order = candidate_order(&physical, &insertions, &receipts).ok_or_else(unsupported)?;
    let root = |mut id| {
        for _ in 0..=receipts.len() {
            match receipts
                .iter()
                .find(|edge| edge.restored == id)
                .and_then(|edge| edge.origin)
            {
                Some(parent) => id = parent,
                None => return Some(id),
            }
        }
        None
    };
    let rank = |id| root(id).and_then(|id| order.iter().position(|x| *x == id));
    let removed = creations
        .into_iter()
        .map(|id| rank(id).ok_or_else(unsupported))
        .collect::<LoroResult<Vec<_>>>()?;
    let mut targets = payloads
        .into_iter()
        .map(|(id, cell)| Ok((rank(id).ok_or_else(unsupported)?, id, cell)))
        .collect::<LoroResult<Vec<_>>>()?;
    targets.sort_by_key(|entry| entry.0);
    let (ids, cells) = if is_list {
        let list = doc.get_list(&cid);
        let ids = (0..list.len())
            .map(|i| {
                list.get_cursor(i, Side::Middle)
                    .and_then(|c| c.id)
                    .ok_or_else(unsupported)
            })
            .collect::<LoroResult<Vec<_>>>()?;
        let cells = (0..list.len())
            .map(|i| {
                let value = list.get(i).ok_or_else(unsupported)?;
                if matches!(value, LoroValue::Container(_)) {
                    return Err(unsupported());
                }
                Ok(Cell::Value(value))
            })
            .collect::<LoroResult<Vec<_>>>()?;
        (ids, cells)
    } else {
        let text = doc.get_text(&cid);
        let ids = (0..text.len_unicode())
            .map(|i| {
                text.get_cursor_internal(i, Side::Middle, false)
                    .and_then(|c| c.id)
                    .ok_or_else(unsupported)
            })
            .collect::<LoroResult<Vec<_>>>()?;
        (ids, text.to_string().chars().map(Cell::Text).collect())
    };
    let live = ids
        .into_iter()
        .map(|id| rank(id).ok_or_else(unsupported))
        .collect::<LoroResult<Vec<_>>>()?;
    if !live.windows(2).all(|w| w[0] < w[1])
        || !targets.windows(2).all(|w| w[0].0 < w[1].0)
        || targets
            .iter()
            .any(|target| live.contains(&target.0) && !removed.contains(&target.0))
    {
        return Err(unsupported());
    }
    let mut replacement = if is_list {
        Diff::List(ListDiff::new())
    } else {
        Diff::Text(TextDiff::new())
    };
    let mut relocated = Vec::new();
    let mut at = 0;
    let mut output = 0;
    for (target_rank, origin, cell) in targets {
        while at < live.len() && live[at] < target_rank {
            if removed.contains(&live[at]) {
                erase(&mut replacement, cells[at].width());
            } else {
                keep(&mut replacement, cells[at].width());
                output += cells[at].width();
            }
            at += 1;
        }
        let width = cell.width();
        emit(&mut replacement, cell);
        relocated.push((output, Some(origin)));
        output += width;
    }
    while at < live.len() {
        if removed.contains(&live[at]) {
            erase(&mut replacement, cells[at].width());
        } else {
            keep(&mut replacement, cells[at].width());
        }
        at += 1;
    }
    diff.cid_to_events.insert(cid.clone(), replacement);
    pending.relocate_origins(&cid, relocated);
    Ok(())
}

fn event_len(cell: char) -> usize {
    if cfg!(feature = "wasm") {
        cell.len_utf16()
    } else {
        1
    }
}

#[derive(Clone)]
enum Cell {
    Text(char),
    Value(LoroValue),
}
impl Cell {
    fn width(&self) -> usize {
        match self {
            Self::Text(c) => event_len(*c),
            Self::Value(_) => 1,
        }
    }
}
enum Piece {
    Retain(usize),
    Edit { cells: Vec<Cell>, delete: usize },
}
fn pieces(diff: &Diff) -> LoroResult<Vec<Piece>> {
    let mut answer = Vec::new();
    match diff {
        Diff::Text(delta) => {
            for item in delta.iter() {
                answer.push(match item {
                    DeltaItem::Retain { len, attr } if attr.0.is_empty() => Piece::Retain(*len),
                    DeltaItem::Replace {
                        value,
                        delete,
                        attr,
                    } if attr.0.is_empty() => Piece::Edit {
                        cells: value.as_str().chars().map(Cell::Text).collect(),
                        delete: *delete,
                    },
                    _ => return Err(unsupported()),
                });
            }
        }
        Diff::List(delta) => {
            for item in delta.iter() {
                answer.push(match item {
                    DeltaItem::Retain { len, attr } if !attr.from_move => Piece::Retain(*len),
                    DeltaItem::Replace {
                        value,
                        delete,
                        attr,
                    } if !attr.from_move => Piece::Edit {
                        cells: value
                            .iter()
                            .map(|v| match v {
                                ValueOrHandler::Value(v)
                                    if !matches!(v, LoroValue::Container(_)) =>
                                {
                                    Ok(Cell::Value(v.clone()))
                                }
                                _ => Err(unsupported()),
                            })
                            .collect::<LoroResult<_>>()?,
                        delete: *delete,
                    },
                    _ => return Err(unsupported()),
                });
            }
        }
        _ => return Err(unsupported()),
    }
    Ok(answer)
}
fn keep(diff: &mut Diff, len: usize) {
    match diff {
        Diff::Text(d) => {
            d.push_retain(len, Default::default());
        }
        Diff::List(d) => {
            d.push_retain(len, Default::default());
        }
        _ => unreachable!(),
    };
}
fn erase(diff: &mut Diff, len: usize) {
    match diff {
        Diff::Text(d) => {
            d.push_delete(len);
        }
        Diff::List(d) => {
            d.push_delete(len);
        }
        _ => unreachable!(),
    };
}
fn emit(diff: &mut Diff, cell: Cell) {
    match (diff, cell) {
        (Diff::Text(d), Cell::Text(c)) => {
            d.push_insert(StringSlice::from(c.to_string()), Default::default());
        }
        (Diff::List(d), Cell::Value(v)) => {
            for chunk in ListDiffInsertItem::from_many(std::iter::once(ValueOrHandler::Value(v))) {
                d.push_insert(chunk, Default::default());
            }
        }
        _ => unreachable!(),
    }
}
