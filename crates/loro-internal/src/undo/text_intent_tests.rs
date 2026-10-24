//! The deletion intent needed by undo survives import, but not in state diffs.
use super::*;
use crate::{
    container::list::list_op::InnerListOp, cursor::PosType, encoding::ExportMode, op::InnerContent,
};
use loro_common::ID;
use rle::HasLength;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn invisible_import_retains_both_original_delete_targets_in_native_history() {
    for (local_peer, remote_peer) in [(1, 2), (2, 1)] {
        for original in ["B", "🦀", "B🦀C"] {
            let a = LoroDoc::new_auto_commit();
            a.set_peer_id(local_peer).unwrap();
            let text = a.get_text("text");
            text.insert(0, "a", PosType::Unicode).unwrap();
            a.commit_then_renew();
            let b = a.fork();
            b.set_peer_id(remote_peer).unwrap();
            let remote_text = b.get_text("text");
            remote_text.insert(1, original, PosType::Unicode).unwrap();
            b.commit_then_renew();
            a.import(&b.export(ExportMode::all_updates()).unwrap())
                .unwrap();
            let local_start = get_counter_end(&a, local_peer);
            let remote_start = get_counter_end(&a, remote_peer);
            let len = original.chars().count();
            text.delete(1, len, PosType::Unicode).unwrap();
            text.insert(1, "x", PosType::Unicode).unwrap();
            a.commit_then_renew();
            remote_text.delete(1, len, PosType::Unicode).unwrap();
            b.commit_then_renew();

            let events = Arc::new(AtomicUsize::new(0));
            let seen = events.clone();
            let _subscription = a.subscribe_root(Arc::new(move |_| {
                seen.fetch_add(1, Ordering::Relaxed);
            }));
            let before = a.oplog_vv();
            a.import(&b.export(ExportMode::all_updates()).unwrap())
                .unwrap();
            assert_eq!(text.to_string(), "ax");
            assert_eq!(
                events.load(Ordering::Relaxed),
                0,
                "no state-diff notification"
            );
            assert_ne!(a.oplog_vv(), before, "import still advanced native history");

            let oplog = a.oplog.lock();
            let targets = |peer, start| {
                let mut targets = Vec::new();
                for op in oplog.iter_ops(IdSpan::new(
                    peer,
                    start,
                    oplog.vv().get(&peer).copied().unwrap(),
                )) {
                    if let InnerContent::List(InnerListOp::Delete(deletion)) = &op.op().content {
                        // id_start is the smallest target counter, even for a
                        // backward positional delete. Each ID is one scalar.
                        for offset in 0..deletion.atom_len() {
                            targets.push(ID::new(
                                deletion.id_start.peer,
                                deletion.id_start.counter + offset as i32,
                            ));
                        }
                    }
                }
                targets
            };
            let expected: Vec<_> = (0..len)
                .map(|offset| ID::new(remote_peer, offset as i32))
                .collect();
            assert_eq!(targets(local_peer, local_start), expected, "local deletion");
            assert_eq!(
                targets(remote_peer, remote_start),
                expected,
                "invisible remote deletion"
            );
        }
    }
}
