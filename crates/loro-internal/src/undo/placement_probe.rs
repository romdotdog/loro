//! Test-only single-insertion placement probe; never compiled into production.
use super::text::{AppliedRestoration, PendingRestorations};
use super::DiffBatch;
use crate::{
    container::list::list_op::InnerListOp,
    cursor::Side,
    event::{Diff, TextDiff},
    op::InnerContent,
    utils::string_slice::StringSlice,
    LoroDoc,
};
use loro_common::{Counter, LoroError, LoroResult, ID};
use loro_delta::DeltaItem;
use std::cell::RefCell;

pub(crate) fn candidate_order(
    physical: &[ID],
    inserts: &[crate::diff_calc::HistoricalTextInsertion],
    receipts: &[super::text::AppliedRestoration],
) -> Option<Vec<ID>> {
    let root = |mut id| {
        let mut traversed = 0;
        while let Some(parent) = receipts
            .iter()
            .find(|edge| edge.restored == id)
            .and_then(|edge| edge.origin)
        {
            traversed += 1;
            assert!(traversed <= receipts.len(), "restoration cycle");
            id = parent;
        }
        id
    };
    let mut remaining = Vec::new();
    for id in physical {
        let id = root(*id);
        if !remaining.contains(&id) {
            remaining.push(id);
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
    if edges
        .iter()
        .any(|(a, b)| !remaining.contains(a) || !remaining.contains(b))
    {
        return None;
    }
    let mut answer = Vec::new();
    while !remaining.is_empty() {
        let at = remaining
            .iter()
            .position(|id| !edges.iter().any(|(a, b)| b == id && remaining.contains(a)))?;
        answer.push(remaining.remove(at));
    }
    Some(answer)
}

thread_local! {
    static RECEIPTS: RefCell<Option<Vec<AppliedRestoration>>> = const { RefCell::new(None) };
    static ALLOW_MANAGER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Negative-control diagnostic only: deliberately leave the manager's cached
/// rebase unchanged. This is not an adoption switch or a production opt-in.
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
        "test placement probe requires one plain scalar insertion and no manager transform".into(),
    )
}

/// Runs only when explicitly armed by a test. The complete native inverse has
/// already been calculated and the current head restored. Rewrite its one
/// admitted insertion before the ordinary handler runs in the rollback scope.
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
    let cid = diff.order[0].clone();
    let Some(Diff::Text(delta)) = diff.cid_to_events.get(&cid) else {
        return Err(unsupported());
    };
    let mut payload: Option<StringSlice> = None;
    for item in delta.iter() {
        match item {
            DeltaItem::Retain { attr, .. } if attr.0.is_empty() => {}
            DeltaItem::Replace {
                value,
                delete: 0,
                attr,
            } if attr.0.is_empty() && value.as_str().chars().count() == 1 && payload.is_none() => {
                payload = Some(value.clone());
            }
            _ => return Err(unsupported()),
        }
    }
    let payload = payload.ok_or_else(unsupported)?;
    let origin = pending.probe_single_origin(&cid).ok_or_else(unsupported)?;
    if receipts.iter().any(|edge| {
        edge.source_container != cid || edge.target_container != cid || edge.origin.is_none()
    }) {
        return Err(unsupported());
    }
    let (physical, insertions) = {
        let oplog = doc.oplog().lock();
        if oplog.is_shallow() {
            return Err(unsupported());
        }
        let idx = doc.arena.id_to_idx(&cid).ok_or_else(unsupported)?;
        for (&peer, &end) in oplog.vv().iter() {
            for op in oplog.iter_ops(loro_common::IdSpan::new(peer, 0, end)) {
                if op.container() == idx
                    && !matches!(
                        &op.op().content,
                        InnerContent::List(InnerListOp::InsertText { .. } | InnerListOp::Delete(_))
                    )
                {
                    return Err(unsupported());
                }
            }
        }
        (
            crate::diff_calc::historical_text_id_order(&oplog, &cid),
            crate::diff_calc::historical_text_insertions(&oplog, &cid),
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
    let target_rank = rank(origin).ok_or_else(unsupported)?;
    let text = doc.get_text(&cid);
    let live: Vec<_> = (0..text.len_unicode())
        .map(|i| {
            text.get_cursor_internal(i, Side::Middle, false)
                .and_then(|cursor| cursor.id)
                .and_then(rank)
                .ok_or_else(unsupported)
        })
        .collect::<LoroResult<_>>()?;
    if !live.windows(2).all(|w| w[0] < w[1]) || live.contains(&target_rank) {
        return Err(unsupported());
    }
    let unicode_slot = live.iter().take_while(|r| **r < target_rank).count();
    let event_slot = text
        .to_string()
        .chars()
        .take(unicode_slot)
        .map(|c| {
            if cfg!(feature = "wasm") {
                c.len_utf16()
            } else {
                1
            }
        })
        .sum();
    let mut replacement = TextDiff::new();
    replacement.push_retain(event_slot, Default::default());
    replacement.push_insert(payload, Default::default());
    diff.cid_to_events
        .insert(cid.clone(), Diff::Text(replacement));
    pending.probe_relocate(&cid, event_slot);
    Ok(())
}
