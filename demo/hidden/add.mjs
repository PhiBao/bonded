import assert from "node:assert/strict";
import { add } from "./src/math.js";
assert.equal(add(4, 5), 9);
assert.equal(add(10, -3), 7);
assert.equal(add(0, 0), 0);
console.log("hidden acceptance check passed");
