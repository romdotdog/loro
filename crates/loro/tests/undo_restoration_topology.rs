//! Passing native behavior a replacement placement algorithm must preserve.
use loro::{ExportMode, LoroDoc, UndoManager};

#[test]
fn redo_keeps_original_order_before_restored_item_and_its_remote_child() {
    for peer in [1, 2] {
        for cell in ["a", "🦀"] {
            let doc = LoroDoc::new();
            doc.set_peer_id(peer).unwrap();
            let remote = LoroDoc::new();
            remote.set_peer_id(3 - peer).unwrap();
            let text = doc.get_text("text");
            let mut undo = UndoManager::new(&doc);
            undo.set_merge_interval(0);
            text.insert(0, cell).unwrap();
            doc.commit();
            text.insert(0, "B").unwrap();
            doc.commit();
            assert_eq!(text.to_string(), format!("B{cell}"));
            assert!(undo.undo().unwrap());
            assert_eq!(text.to_string(), cell);
            assert!(undo.undo().unwrap());
            assert_eq!(text.to_string(), "");
            assert!(undo.redo().unwrap());
            assert_eq!(text.to_string(), cell);
            remote
                .import(&doc.export(ExportMode::all_updates()).unwrap())
                .unwrap();
            remote.get_text("text").insert(1, "R").unwrap();
            remote.commit();
            doc.import(&remote.export(ExportMode::all_updates()).unwrap())
                .unwrap();
            assert_eq!(text.to_string(), format!("{cell}R"));
            assert!(
                undo.can_redo(),
                "remote child must not invalidate local redo"
            );
            assert!(undo.redo().unwrap());
            assert_eq!(text.to_string(), format!("B{cell}R"));
            remote
                .import(&doc.export(ExportMode::all_updates()).unwrap())
                .unwrap();
            assert_eq!(remote.get_text("text").to_string(), text.to_string());
            let imported = LoroDoc::new();
            imported
                .import(&doc.export(ExportMode::all_updates()).unwrap())
                .unwrap();
            assert_eq!(imported.get_text("text").to_string(), text.to_string());
        }
    }
}
