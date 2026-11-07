import test from "node:test";
import assert from "node:assert/strict";
import { add, subtract, multiply } from "../src/math.js";

test("add returns the sum", () => {
  assert.equal(add(2, 3), 5);
  assert.equal(add(-1, 1), 0);
});

test("subtract returns the difference", () => {
  assert.equal(subtract(7, 3), 4);
  assert.equal(subtract(0, 4), -4);
});

test("multiply returns the product", () => {
  assert.equal(multiply(3, 4), 12);
});
