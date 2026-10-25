const { LoroDoc, UndoManager } = require("loro-crdt");
const realNow = Date.now;
try {
  let now = 100000;
  Date.now = () => now;
  const doc = new LoroDoc();
  const text = doc.getText("text");
  const undo = new UndoManager(doc, { mergeInterval: 0 });
  text.insert(0, "a");
  doc.commit();
  now = 99500;
  text.insert(1, "b");
  doc.commit();
  undo.undo();
  console.log(JSON.stringify(text.toString()));
  // expected: "a". got: ""
} finally {
  Date.now = realNow;
}
