// jsdom lacks layout; ProseMirror asks for rects and scrolling. Same stubs as milkdown's own
// vitest setup.
const rects = () => ({ length: 0, item: () => null, [Symbol.iterator]: function* () {} }) as unknown as DOMRectList
const box = () => ({ x: 0, y: 0, width: 0, height: 0, top: 0, right: 0, bottom: 0, left: 0, toJSON: () => ({}) }) as DOMRect
Element.prototype.getClientRects = rects
Element.prototype.getBoundingClientRect = box
Range.prototype.getClientRects = rects
Range.prototype.getBoundingClientRect = box
Element.prototype.scrollIntoView = () => {}
document.elementFromPoint = () => null
