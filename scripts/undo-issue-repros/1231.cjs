const { LoroDoc, UndoManager } = require("loro-crdt");

function scenario(changePeer) {
  const doc = new LoroDoc();
  doc.setPeerId("1");

  const text = doc.getText("text");
  text.insert(0, "x");
  doc.commit();

  const undo = new UndoManager(doc, { mergeInterval: 0 });
  undo.groupStart();

  if (changePeer) doc.setPeerId("2");

  text.insert(1, "a");
  doc.commit();
  text.insert(2, "b");
  doc.commit();
  undo.groupEnd();

  undo.undo();
  console.log({
    changePeer,
    text: text.toString(),
    canUndo: undo.canUndo(),
  });
}

scenario(false);
// { changePeer: false, text: "x", canUndo: false }

scenario(true);
// { changePeer: true, text: "xa", canUndo: true }
