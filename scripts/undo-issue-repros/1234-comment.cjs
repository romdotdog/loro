const assert = require("node:assert/strict");
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

list.move(0, 1); // move "a" after the original "b"
doc.commit();

list.delete(0, 1); // delete the original "b"
doc.commit();
assert.equal(undo.undo(), true); // restore "b" as a new copy

remote.getMovableList("sequence").move(1, 0);
remote.commit();
doc.import(remote.export({ mode: "snapshot" }));

const before = list.toArray();
assert.deepEqual(before, ["b", "b", "a"]);

assert.equal(undo.undo(), true); // undo the move of "a"
assert.equal(undo.redo(), true); // redo that same move

console.log({ before, after: list.toArray() });
// before: ["b", "b", "a"]
// after:  ["b", "a", "b"]

assert.deepEqual(list.toArray(), before); // fails
