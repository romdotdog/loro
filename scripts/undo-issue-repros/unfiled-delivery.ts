import assert from "node:assert/strict";
import { LoroDoc, UndoManager } from "loro-crdt";
function run(incremental: boolean) {
  const doc = new LoroDoc(); doc.setPeerId("1");
  const remote = new LoroDoc(); remote.setPeerId("2");
  doc.setRecordTimestamp(false); remote.setRecordTimestamp(false);
  const list = doc.getList("sequence"), other = remote.getList("sequence");
  list.insert(0, "a"); list.insert(1, "b"); doc.commit();
  remote.import(doc.export({ mode: "snapshot" }));
  list.insert(1, "x"); doc.commit();
  other.insert(0, "r"); remote.commit();
  const deliver = () => doc.import(remote.export({ mode: "snapshot" }));
  deliver();
  const undo = new UndoManager(doc, { mergeInterval: 0 });
  list.delete(1, 1); doc.commit();
  other.delete(0, 1); remote.commit(); if (incremental) deliver();
  other.delete(1, 1); remote.commit(); if (incremental) deliver();
  other.insert(0, "s"); remote.commit(); deliver();
  assert.deepEqual(list.toArray(), ["s", "x"]);
  assert.equal(undo.undo(), true);
  const after = list.toArray();
  undo.free(); remote.free(); doc.free();
  return after;
}
console.log(run(false), run(true)); // ["s", "a", "x"] vs ["a", "s", "x"]
assert.deepEqual(run(false), run(true));
