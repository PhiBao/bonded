import assert from "node:assert/strict";
import { multiply } from "./src/math.js";
assert.equal(multiply(5, 6), 30);
assert.equal(multiply(-2, 3), -6);
assert.equal(multiply(7, 0), 0);
console.log("hidden acceptance check passed");
