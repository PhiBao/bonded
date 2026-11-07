import assert from "node:assert/strict";
import { subtract } from "./src/math.js";
assert.equal(subtract(10, 4), 6);
assert.equal(subtract(2, 9), -7);
assert.equal(subtract(5, 5), 0);
console.log("hidden acceptance check passed");
