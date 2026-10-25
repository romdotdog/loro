const { LoroDoc, UndoManager } = require("loro-crdt");

const doc = new LoroDoc();
doc.setPeerId("1");
const remote = new LoroDoc();
remote.setPeerId("2");

const list = doc.getMovableList("sequence");
list.insert(0, "a");
list.insert(1, "b");
doc.commit();
remote.import(doc.export({ mode: "snapshot" }));

const undo = new UndoManager(doc, { mergeInterval: 0 });

list.delete(0, 1); // A deletes the original "a"
doc.commit();

remote.getMovableList("sequence").move(0, 1); // B moves the original "a"
remote.commit();

undo.undo(); // A creates a replacement "a" before receiving the move
console.log(list.toArray()); // ["a", "b"]

doc.import(remote.export({ mode: "snapshot" }));
console.log(list.toArray()); // ["a", "b", "a"]

list.delete(2, 1); // delete the original, leaving the replacement
doc.commit();
console.log(list.toArray()); // ["a", "b"]
console.log(undo.canUndo()); // true

const executed = undo.undo();
console.log({ executed, restored: list.toArray() });
// expected: { executed: true, restored: ["a", "b", "a"] }
// actual: { executed: false, restored: ["a", "b"] }

console.log(undo.canUndo()); // false
