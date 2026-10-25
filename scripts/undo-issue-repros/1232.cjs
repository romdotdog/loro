const { LoroDoc, UndoManager } = require("loro-crdt");

const doc = new LoroDoc();
const text = doc.getText("text");
const undo = new UndoManager(doc, { mergeInterval: 0 });
text.insert(0, "a"); doc.commit();
text.insert(1, "b"); doc.commit();
undo.undo();
undo.setMaxUndoSteps(1);
undo.redo();
undo.undo();
console.log(text.toString()); // "a"
console.log(undo.canUndo());  // true; expected false
