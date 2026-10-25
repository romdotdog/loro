const assert = require("node:assert/strict");
const { LoroDoc, UndoManager } = require("loro-crdt");

const a = new LoroDoc(), b = new LoroDoc();
a.setPeerId("1"); b.setPeerId("2");
for (const doc of [a, b])
  doc.configTextStyle({ color: { expand: "after" } });

function sync(from, to) {
  const vv = to.oplogVersion();
  try { to.import(from.export({ mode: "update", from: vv })); }
  finally { vv.free(); }
}

const text = a.getText("text");
text.insert(0, "x");
a.commit(); sync(a, b);
const undo = new UndoManager(a, { mergeInterval: 0 });

text.mark({ start: 0, end: 1 }, "color", "red");
a.commit(); sync(a, b);
b.getText("text").mark({ start: 0, end: 1 }, "color", "blue");
b.commit(); sync(b, a);

const expected = [{ insert: "x", attributes: { color: "blue" } }];
assert.deepEqual(text.toDelta(), expected); // passes
undo.undo();
assert.deepEqual(text.toDelta(), expected); // fails: [{ insert: "x" }]
