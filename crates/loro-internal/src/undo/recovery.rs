//! Source-local experimental metadata, carried by existing change messages.
//! This is a trusted-producer protocol, not proof of an author's undo intent.
use super::text::AppliedRestoration;
use crate::{container::list::list_op::InnerListOp, op::InnerContent, LoroDoc, VersionVector};
use loro_common::{
    ContainerID, ContainerType, HasCounterSpan, LoroError, LoroResult, LoroValue, ID,
};
use rle::HasLength;
use serde::{Deserialize, Serialize};

const PREFIX: &str = "loro:identity-undo:1\n";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    peer: u64,
    start: i32,
    end: i32,
    inverse: bool,
    // restored peer/counter, original peer/counter. Container and payload are
    // recovered/checked against actual creation operations, not caller strings.
    edges: Vec<(u64, i32, u64, i32)>,
    message: Option<String>,
}
pub(crate) fn error(reason: &str) -> LoroError {
    LoroError::TransactionError(format!("identity history admission: {reason}").into())
}

pub(crate) fn stamp(change: &mut crate::change::Change, receipts: Option<&[AppliedRestoration]>) {
    let record = Record {
        peer: change.id.peer,
        start: change.id.counter,
        end: change.ctr_end(),
        inverse: receipts.is_some(),
        edges: receipts
            .unwrap_or(&[])
            .iter()
            .map(|r| {
                let origin = r
                    .origin
                    .expect("admitted inverse has actual restoration origin");
                (
                    r.restored.peer,
                    r.restored.counter,
                    origin.peer,
                    origin.counter,
                )
            })
            .collect(),
        message: change.commit_msg.as_ref().map(|m| m.to_string()),
    };
    change.commit_msg = Some(format!("{PREFIX}{}", serde_json::to_string(&record).unwrap()).into());
}

// Per-recovery only: immutable creation spans are decoded once, never retained
// in the document/manager or trusted instead of committed history.
type TextScalars = rustc_hash::FxHashMap<ID, Vec<char>>;
fn cell(
    oplog: &crate::oplog::OpLog,
    id: ID,
    scalars: &mut TextScalars,
) -> LoroResult<(ContainerID, LoroValue)> {
    if id.counter < 0 || !oplog.vv().includes_id(id) {
        return Err(error("missing creation ID"));
    }
    let op = oplog
        .get_op_that_includes(id)
        .ok_or_else(|| error("missing creation operation"))?;
    let offset = (id.counter - op.counter) as usize;
    let cid = oplog
        .arena
        .get_container_id(op.container)
        .ok_or_else(|| error("missing container"))?;
    let value = match &op.content {
        InnerContent::List(InnerListOp::InsertText { slice, .. })
            if cid.container_type() == ContainerType::Text =>
        {
            let key = ID::new(id.peer, op.counter);
            if !scalars.contains_key(&key) {
                let s = std::str::from_utf8(slice).map_err(|_| error("invalid text"))?;
                scalars.insert(key, s.chars().collect());
            }
            scalars[&key]
                .get(offset)
                .ok_or_else(|| error("creation offset"))?
                .to_string()
                .into()
        }
        InnerContent::List(InnerListOp::Insert { slice, .. })
            if cid.container_type() == ContainerType::List =>
        {
            let value = slice
                .as_slice()
                .get(offset)
                .ok_or_else(|| error("creation offset"))?
                .clone();
            if matches!(value, LoroValue::Container(_)) {
                return Err(error("nested container"));
            }
            value
        }
        _ => return Err(error("edge must name a plain Text/List creation")),
    };
    Ok((cid, value))
}

fn read_record(change: &crate::change::Change) -> LoroResult<Record> {
    if change
        .commit_msg
        .as_deref()
        .is_some_and(|s| s.starts_with("loro:identity-undo:") && !s.starts_with(PREFIX))
    {
        return Err(error("unsupported metadata version"));
    }
    let record = match change
        .commit_msg
        .as_deref()
        .and_then(|s| s.strip_prefix(PREFIX))
    {
        Some(s) => serde_json::from_str::<Record>(s).map_err(|_| error("malformed metadata"))?,
        None => return Err(error("untagged change")),
    };
    // Encoding can split a native change. The original record still covers
    // its slices; only the edges within this available slice are consumed.
    if record.peer != change.id.peer
        || record.start < 0
        || record.start > change.id.counter
        || record.end < change.ctr_end()
        || record.end <= record.start
        || (!record.inverse && !record.edges.is_empty())
    {
        return Err(error("metadata/change range mismatch"));
    }
    Ok(record)
}

/// Validate overlapping identity declarations before native known-history trim.
/// Existing operation equality rules remain separate. User message text is not
/// identity metadata; JSON whitespace and edge ordering do not change a record.
pub(crate) fn check_known_records(
    oplog: &crate::OpLog,
    changes: &[crate::change::Change],
) -> LoroResult<()> {
    for change in changes {
        let end = change
            .ctr_end()
            .min(oplog.vv().get(&change.id.peer).copied().unwrap_or(0));
        if change.id.counter >= end {
            continue;
        }
        let mut incoming = read_record(change)?;
        incoming.edges.sort_unstable();
        let mut cursor = change.id.counter;
        while cursor < end {
            let known = oplog
                .try_get_change_at(ID::new(change.id.peer, cursor))?
                .ok_or_else(|| error("missing known identity change"))?;
            let mut record = read_record(&known)?;
            record.edges.sort_unstable();
            if incoming.peer != record.peer
                || incoming.start != record.start
                || incoming.end != record.end
                || incoming.inverse != record.inverse
                || incoming.edges != record.edges
            {
                return Err(error("conflicting known identity metadata"));
            }
            let next = known.ctr_end().min(end);
            if next <= cursor {
                return Err(error("invalid known identity change range"));
            }
            cursor = next;
        }
    }
    Ok(())
}

/// Read all available history. Every change must carry a record; untagged
/// historical operations cannot be silently certified as fresh typing.
/// Each relation edge must be from this change's insertion to a causally prior
/// creation in the same container with equal payload. These checks do not prove
/// that a trusted writer's claimed restoration intent is true.
pub(crate) fn recover(doc: &LoroDoc) -> LoroResult<Vec<AppliedRestoration>> {
    let oplog = doc.oplog().lock();
    if oplog.is_shallow() {
        return Err(error("full history required"));
    }
    if !oplog.pending_changes.is_empty() {
        return Err(error("pending history; resync required"));
    }
    let mut answer = Vec::new();
    let mut scalars = TextScalars::default();
    let mut seen = rustc_hash::FxHashMap::default();
    for change in oplog.iter_changes_peer_by_peer(&VersionVector::default(), oplog.vv()) {
        let record = read_record(&change)?;
        let mut marked = rustc_hash::FxHashSet::default();
        for (peer, counter, origin_peer, origin_counter) in record.edges {
            if peer != record.peer
                || counter < record.start
                || counter >= record.end
                || origin_counter < 0
                || !marked.insert(ID::new(peer, counter))
            {
                return Err(error("invalid or duplicate edge"));
            }
            let restored = ID::new(peer, counter);
            if counter < change.id.counter || counter >= change.ctr_end() {
                continue;
            }
            let origin = ID::new(origin_peer, origin_counter);
            let (cid, value) = cell(&oplog, restored, &mut scalars)?;
            let (origin_cid, original) = cell(&oplog, origin, &mut scalars)?;
            if cid != origin_cid || value != original {
                return Err(error("edge container/payload mismatch"));
            }
            let vv = oplog
                .dag
                .try_frontiers_to_vv(&change.deps)?
                .ok_or_else(|| error("missing causal history"))?;
            if !vv.includes_id(origin) {
                return Err(error("origin is not causally prior"));
            }
            if seen
                .insert(restored, origin)
                .is_some_and(|old| old != origin)
            {
                return Err(error("conflicting edge"));
            }
            answer.push(AppliedRestoration {
                source_container: cid.clone(),
                target_container: cid,
                output: None,
                origin: Some(origin),
                restored,
            });
        }
        if record.inverse {
            // No restored insertion may quietly masquerade as fresh typing.
            for op in change.ops.iter() {
                if matches!(
                    &op.content,
                    InnerContent::List(InnerListOp::InsertText { .. } | InnerListOp::Insert { .. })
                ) {
                    for offset in 0..op.atom_len() {
                        if !marked.contains(&ID::new(change.id.peer, op.counter + offset as i32)) {
                            return Err(error("inverse insertion missing its edge"));
                        }
                    }
                }
            }
        }
    }
    oplog.check_history_parsable()?;
    Ok(answer)
}

impl LoroDoc {
    /// Experimental trusted-producer protocol. Call before producing shared
    /// edits, or on a recovered document whose entire history is tagged. All future
    /// local changes carry metadata; imports require complete tagged history.
    /// This mode persists for the document's lifetime, even after manager drop.
    pub fn enable_identity_history(&self) -> LoroResult<()> {
        self.with_barrier(|| {
            if self.is_detached() {
                return Err(error("attached history required"));
            }
            recover(self)?;
            self.identity_history
                .store(true, std::sync::atomic::Ordering::Release);
            Ok(())
        })
    }
    pub(crate) fn identity_recover(&self) -> LoroResult<Option<Vec<AppliedRestoration>>> {
        if self
            .identity_history
            .load(std::sync::atomic::Ordering::Acquire)
        {
            recover(self).map(Some)
        } else {
            Ok(None)
        }
    }
    /// Dry run in an isolated full-history fork while the caller owns the
    /// target's writer/barrier guard. No target operations/events are published.
    pub(crate) fn identity_preflight(
        &self,
        import: impl FnOnce(&LoroDoc) -> LoroResult<crate::encoding::ImportStatus>,
    ) -> LoroResult<()> {
        if !self
            .identity_history
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Ok(());
        }
        if self.is_detached() || self.is_shallow() {
            return Err(error("attached full history required"));
        }
        // Do not call `fork()` here: its nested auto-commit barrier would renew
        // the target transaction before the outer import has finished.
        let snapshot = crate::encoding::fast_snapshot::encode_snapshot_inner_for_fork(self)?;
        let staged = LoroDoc::new();
        crate::encoding::fast_snapshot::decode_snapshot_inner(
            snapshot,
            &staged,
            Default::default(),
        )?;
        staged.set_config(&self.config);
        let status = import(&staged)?;
        if status.pending.is_some() {
            return Err(error("missing dependencies; resync complete history"));
        }
        recover(&staged)?;
        Ok(())
    }
}

#[cfg(test)]
mod scalar_tests {
    use super::*;
    use crate::cursor::PosType;

    #[test]
    fn text_cell_index_returns_each_actual_scalar_across_peers() {
        let a = LoroDoc::new_auto_commit();
        a.set_peer_id(11).unwrap();
        a.get_text("text")
            .insert(0, "a🦀b字", PosType::Unicode)
            .unwrap();
        a.commit_then_renew();
        let b = LoroDoc::new_auto_commit();
        b.set_peer_id(22).unwrap();
        b.get_text("text")
            .insert(0, "z🌍c語", PosType::Unicode)
            .unwrap();
        b.commit_then_renew();
        a.import(&b.export(crate::loro::ExportMode::all_updates()).unwrap())
            .unwrap();
        let oplog = a.oplog().lock();
        let mut scalars = TextScalars::default();
        for offset in [3, 0, 2, 1, 3] {
            for (peer, expected) in [(11, ['a', '🦀', 'b', '字']), (22, ['z', '🌍', 'c', '語'])]
            {
                let (_, value) = cell(&oplog, ID::new(peer, offset), &mut scalars).unwrap();
                assert_eq!(
                    value,
                    LoroValue::from(expected[offset as usize].to_string())
                );
            }
        }
        assert_eq!(scalars.len(), 2);
    }
}
