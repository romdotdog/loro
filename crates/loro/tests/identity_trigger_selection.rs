//! Native cursor metadata, not an application-side coordinate correction.
#![cfg(feature = "experimental-identity-undo")]
use loro::{
    cursor::{Cursor, Side},
    LoroDoc, UndoItemMeta, UndoManager,
};
use std::sync::{Arc, Mutex};

#[test]
fn live_cursor_ids_follow_late_children_through_complete_history_cycles() {
    for base in ["abcd", "a🦀b字", "aaaa"] {
        for prefix in [false, true] {
            for peer in [1, 2] {
                for list in [false, true] {
                    live_sequence_case(base, prefix, peer, list);
                }
            }
        }
    }
}

fn live_sequence_case(base: &str, prefix: bool, peer: u64, list: bool) {
    eprintln!("base={base:?} prefix={prefix} peer={peer} list={list}");
    let doc = LoroDoc::new();
    doc.set_peer_id(peer).unwrap();
    doc.enable_identity_history().unwrap();
    let text = sequence(&doc, list);
    text.insert(0, base).unwrap();
    doc.commit();
    let mut undo = UndoManager::new_identity(&doc).unwrap();
    undo.set_merge_interval(0);
    let prepared = Arc::new(Mutex::new(Vec::<Cursor>::new()));
    let popped = Arc::new(Mutex::new(Vec::<Cursor>::new()));
    let push = prepared.clone();
    undo.set_on_push(Some(Box::new(move |_, _, _| {
        let mut meta = UndoItemMeta::new();
        for cursor in push.lock().unwrap().iter() {
            meta.add_cursor(cursor);
        }
        meta
    })));
    let pop = popped.clone();
    undo.set_on_pop(Some(Box::new(move |_, _, meta| {
        *pop.lock().unwrap() = meta.cursors.into_iter().map(|c| c.cursor).collect();
    })));
    let capture = |points: &[usize]| {
        *prepared.lock().unwrap() = points
            .iter()
            .map(|&i| text.get_cursor(i, Side::Middle).unwrap())
            .collect();
    };
    let positions = || {
        popped
            .lock()
            .unwrap()
            .iter()
            .map(|c| doc.get_cursor_pos(c).unwrap().current.pos)
            .collect::<Vec<_>>()
    };
    capture(&[4, 4, 0, 0]);
    undo.group_start().unwrap();
    text.insert(4, "(").unwrap();
    text.insert(0, "(").unwrap();
    doc.commit();
    undo.group_end();
    let trigger = undo.capture_trigger().unwrap();
    capture(&[2, 2, 4, 4]);
    let original = prepared.lock().unwrap().clone();
    text.insert(2, "B").unwrap();
    doc.commit();
    let offset = usize::from(prefix);
    let label = if prefix { "R" } else { "" };
    if prefix {
        let remote = LoroDoc::new();
        remote.set_peer_id(3 - peer).unwrap();
        remote.enable_identity_history().unwrap();
        remote
            .import(&doc.export(loro::ExportMode::all_updates()).unwrap())
            .unwrap();
        sequence(&remote, list).insert(0, "R").unwrap();
        remote.commit();
        doc.import(&remote.export(loro::ExportMode::all_updates()).unwrap())
            .unwrap();
    }
    undo.with_derived(&trigger, || {
        text.insert(7 + offset, ")").unwrap();
        text.insert(1 + offset, ")").unwrap();
        doc.commit();
    })
    .unwrap();
    assert_eq!(
        text.content(),
        format!("{label}()aB{}()", base.chars().skip(1).collect::<String>())
    );
    for _ in 0..3 {
        capture(&[0, 0, 9 + offset, 9 + offset]);
        assert!(undo.undo().unwrap());
        assert_eq!(text.content(), format!("{label}(){base}()"));
        // The actual original live IDs already resolve correctly; the pop path
        // must not overwrite them with a stale positional row.
        assert_eq!(
            original
                .iter()
                .map(|c| doc.get_cursor_pos(c).unwrap().current.pos)
                .collect::<Vec<_>>(),
            vec![3 + offset, 3 + offset, 5 + offset, 5 + offset]
        );
        assert_eq!(
            positions(),
            vec![3 + offset, 3 + offset, 5 + offset, 5 + offset]
        );
        for (actual, expected) in popped.lock().unwrap().iter().zip(&original) {
            assert_eq!(
                actual.id, expected.id,
                "preserve the actual still-live ID, including equal-spelling cells"
            );
        }
        // Preserve the previous pop selection between commands: the native
        // loop deliberately carries that into the intermediate opposite item.
        assert!(undo.undo().unwrap());
        assert_eq!(text.content(), format!("{label}{base}"));
        assert_eq!(positions(), vec![4 + offset, 4 + offset, offset, offset]);
        assert!(undo.redo().unwrap());
        assert_eq!(text.content(), format!("{label}(){base}()"));
        assert_eq!(
            positions(),
            vec![3 + offset, 3 + offset, 5 + offset, 5 + offset]
        );
        assert!(undo.redo().unwrap());
        assert_eq!(
            text.content(),
            format!("{label}()aB{}()", base.chars().skip(1).collect::<String>())
        );
        assert_eq!(positions(), vec![0, 0, 9 + offset, 9 + offset]);
    }
}

enum Sequence {
    Text(loro::LoroText),
    List(loro::LoroList),
}
fn sequence(doc: &LoroDoc, list: bool) -> Sequence {
    if list {
        Sequence::List(doc.get_list("sequence"))
    } else {
        Sequence::Text(doc.get_text("sequence"))
    }
}
impl Sequence {
    fn insert(&self, at: usize, value: &str) -> loro::LoroResult<()> {
        match self {
            Self::Text(t) => t.insert(at, value),
            Self::List(l) => {
                for (i, c) in value.chars().enumerate() {
                    l.insert(at + i, c.to_string())?;
                }
                Ok(())
            }
        }
    }
    fn get_cursor(&self, at: usize, side: Side) -> Option<Cursor> {
        match self {
            Self::Text(t) => t.get_cursor(at, side),
            Self::List(l) => l.get_cursor(at, side),
        }
    }
    fn content(&self) -> String {
        match self {
            Self::Text(t) => t.to_string(),
            Self::List(l) => l
                .to_vec()
                .into_iter()
                .map(|v| match v {
                    loro::LoroValue::String(s) => s.to_string(),
                    _ => panic!("non-string fixture cell"),
                })
                .collect(),
        }
    }
}
