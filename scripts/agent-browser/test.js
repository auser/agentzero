#!/usr/bin/env node
// Smoke tests for agent-browser.
// Run: cd scripts/agent-browser && npm install && npm test
//
// Each invocation spawns a fresh browser with a persistent user data dir.
// Page content does NOT persist across calls (only cookies/storage do).
// The Rust BrowserTool handles sequencing by issuing navigate before other actions.

"use strict";

const { execFileSync } = require("child_process");
const path = require("path");

const SCRIPT = path.join(__dirname, "index.js");

function run(action) {
  return execFileSync("node", [SCRIPT, "--action", JSON.stringify(action)], {
    timeout: 60000,
    encoding: "utf-8",
  });
}

let passed = 0;
let failed = 0;

function test(name, fn) {
  try {
    fn();
    console.log(`  PASS: ${name}`);
    passed++;
  } catch (e) {
    console.error(`  FAIL: ${name}: ${e.message}`);
    failed++;
  }
}

function assert(cond, msg) {
  if (!cond) throw new Error(msg || "assertion failed");
}

console.log("agent-browser smoke tests\n");

test("navigate to example.com", () => {
  const out = run({ action: "navigate", url: "https://example.com" });
  assert(out.includes("Navigated to"), "should confirm navigation");
  assert(out.includes("status=200"), "should return 200");
  assert(out.includes("title=Example Domain"), "should include title");
});

test("get_title on blank page returns empty", () => {
  const out = run({ action: "get_title" });
  assert(typeof out === "string", "should return string");
});

test("get_url on blank page returns about:blank", () => {
  const out = run({ action: "get_url" });
  assert(out.includes("about:blank"), "fresh page is about:blank");
});

test("get_text returns no elements on blank page", () => {
  const out = run({ action: "get_text", selector: "h1" });
  assert(out.includes("No elements found"), "blank page has no h1");
});

test("scroll down", () => {
  const out = run({ action: "scroll", direction: "down" });
  assert(out.includes("Scrolled down"), "should confirm scroll");
});

test("screenshot", () => {
  const out = run({ action: "screenshot" });
  assert(out.includes("Screenshot saved"), "should save screenshot");
});

test("close cleans up state", () => {
  const out = run({ action: "close" });
  assert(out.includes("Browser closed"), "should confirm close");
});

test("unknown action fails", () => {
  try {
    run({ action: "nonexistent" });
    throw new Error("should have thrown");
  } catch (e) {
    assert(e.status !== 0, "should exit non-zero for unknown action");
  }
});

test("navigate without url fails", () => {
  try {
    run({ action: "navigate" });
    throw new Error("should have thrown");
  } catch (e) {
    assert(e.status !== 0, "should exit non-zero without url");
  }
});

console.log(`\n${passed} passed, ${failed} failed`);
process.exit(failed > 0 ? 1 : 0);
