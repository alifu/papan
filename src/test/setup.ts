import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// jsdom has no layout; CodeMirror measures text through these two Range methods.
const emptyRects = { length: 0, item: () => null, [Symbol.iterator]: [][Symbol.iterator] };
const zeroRect = {
  x: 0, y: 0, width: 0, height: 0, top: 0, right: 0, bottom: 0, left: 0,
  toJSON: () => ({}),
};
Range.prototype.getClientRects = () => emptyRects as unknown as DOMRectList;
Range.prototype.getBoundingClientRect = () => zeroRect as DOMRect;

afterEach(() => {
  cleanup();
});
