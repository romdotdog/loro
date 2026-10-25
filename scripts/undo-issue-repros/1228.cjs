const { LoroDoc, UndoManager } = require('loro-crdt');

const a = new LoroDoc(), b = new LoroDoc();
a.setPeerId('1');
b.setPeerId('2');
const text = a.getText('text');
const undo = new UndoManager(a, { mergeInterval: 0 });
const sync = (from, to) => to.import(from.export({
  mode: 'update', from: to.oplogVersion()
}));

undo.groupStart();
text.insert(0, 'x');
a.commit();
sync(a, b);                         // B sees the intermediate "x"

b.getMap('metadata').set('flag', true);   // B never edits the text
b.commit();

text.delete(0, 1);
text.insert(0, 'y');
a.commit();
undo.groupEnd();                    // group closed BEFORE the import

sync(b, a);
console.log(text.toString());                    // "y"
undo.undo();
console.log(text.toString());                    // "x"  (expected "")
console.log(undo.canUndo());                     // false
console.log(a.getMap('metadata').get('flag'));   // true
undo.redo();
console.log(text.toString());                    // "y"
