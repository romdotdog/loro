const { setTimeout: sleep } = require("node:timers/promises");
const { LoroDoc, UndoManager } = require("loro-crdt");

(async () => {
  const doc = new LoroDoc();
  doc.setPeerId("1");
  const remote = new LoroDoc();
  remote.setPeerId("2");

  const text = doc.getText("text");
  text.insert(0, "ab");
  doc.commit();

  const undo = new UndoManager(doc, { mergeInterval: 500 });

  text.insert(1, "X");
  doc.commit();
  const first = performance.now();

  remote.import(doc.export({ mode: "snapshot" }));
  remote.getText("text").delete(1, 1);
  remote.commit();
  doc.import(remote.export({ mode: "snapshot" }));

  await sleep(300);
  text.delete(0, 1); // delete "a" and start a new undo item because of mergeInterval
  doc.commit();
  const second = performance.now();

  await sleep(300);
  const sinceFirst = performance.now() - first;
  const sinceSecond = performance.now() - second;
  text.delete(0, 1); // delete "b"
  doc.commit();

  const timingWindowMet =
    second - first < 500 &&
    sinceFirst > 500 &&
    sinceSecond < 500;

  console.log({
    sinceFirst,
    sinceSecond,
    timingWindowMet, // hopefully true
    beforeUndo: text.toString(), // should be ""
  });

  if (!timingWindowMet) {
    console.log("try again.");
    return;
  }

  const executed = undo.undo();
  console.log({ executed, afterUndo: text.toString() });

  // expected: { executed: true, afterUndo: "ab" }, the deletes should undo together
  // actual: { executed: true, afterUndo: "b" }
})().catch(console.error);
