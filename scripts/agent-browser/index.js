#!/usr/bin/env node
// agent-browser — Playwright-based browser automation CLI for AgentZero.
//
// Usage: agent-browser --action '{"action":"navigate","url":"https://example.com"}'
//
// Accepts JSON via --action flag, drives a headless Chromium browser,
// and prints results to stdout. The BrowserTool in agentzero-tools spawns
// this process for each action.
//
// Supported actions (matching BrowserAction enum in browser.rs):
//   navigate, snapshot, click, fill, type, get_text, get_title, get_url,
//   screenshot, wait, press, hover, scroll, close
//
// Persistence: uses a user data dir so cookies/state survive across
// invocations without needing a long-lived daemon process.

"use strict";

const { chromium } = require("playwright");
const fs = require("fs");
const path = require("path");

const STATE_DIR =
  process.env.AGENT_BROWSER_STATE_DIR ||
  path.join(process.env.HOME || "/tmp", ".agent-browser");
const USER_DATA_DIR = path.join(STATE_DIR, "user-data");
const SCREENSHOT_DIR = STATE_DIR;

async function main() {
  const actionIdx = process.argv.indexOf("--action");
  if (actionIdx === -1 || !process.argv[actionIdx + 1]) {
    console.error("Usage: agent-browser --action '<json>'");
    process.exit(1);
  }

  let input;
  try {
    input = JSON.parse(process.argv[actionIdx + 1]);
  } catch (e) {
    console.error(`Invalid JSON: ${e.message}`);
    process.exit(1);
  }

  const action = input.action;
  if (!action) {
    console.error('Missing "action" field in input');
    process.exit(1);
  }

  // Close is special — just clean up state.
  if (action === "close") {
    cleanup();
    process.stdout.write("Browser closed\n");
    return;
  }

  let browser;
  try {
    fs.mkdirSync(STATE_DIR, { recursive: true });

    // Launch with persistent context so cookies/storage survive across calls.
    browser = await chromium.launchPersistentContext(USER_DATA_DIR, {
      headless: true,
      args: [
        "--no-sandbox",
        "--disable-setuid-sandbox",
        "--disable-dev-shm-usage",
      ],
    });

    // Reuse existing page or create one.
    const pages = browser.pages();
    const page = pages.length > 0 ? pages[0] : await browser.newPage();

    const result = await dispatch(action, input, page);
    process.stdout.write(result);
  } catch (e) {
    console.error(`error: ${e.message}`);
    process.exit(1);
  } finally {
    if (browser) {
      await browser.close().catch(() => {});
    }
  }
}

function cleanup() {
  try {
    if (fs.existsSync(USER_DATA_DIR)) {
      fs.rmSync(USER_DATA_DIR, { recursive: true, force: true });
    }
  } catch {
    // best-effort
  }
}

async function dispatch(action, input, page) {
  switch (action) {
    case "navigate":
      return await handleNavigate(page, input);
    case "snapshot":
      return await handleSnapshot(page);
    case "click":
      return await handleClick(page, input);
    case "fill":
      return await handleFill(page, input);
    case "type":
      return await handleType(page, input);
    case "get_text":
      return await handleGetText(page, input);
    case "get_title":
      return await handleGetTitle(page);
    case "get_url":
      return await handleGetUrl(page);
    case "screenshot":
      return await handleScreenshot(page, input);
    case "wait":
      return await handleWait(page, input);
    case "press":
      return await handlePress(page, input);
    case "hover":
      return await handleHover(page, input);
    case "scroll":
      return await handleScroll(page, input);
    default:
      throw new Error(`Unknown action: ${action}`);
  }
}

// --- Action handlers ---

async function handleNavigate(page, input) {
  const url = input.url;
  if (!url) throw new Error("navigate requires url");

  const response = await page.goto(url, {
    waitUntil: "domcontentloaded",
    timeout: 30000,
  });
  const status = response ? response.status() : "unknown";
  const title = await page.title();
  return `Navigated to ${url}\nstatus=${status}\ntitle=${title}\n`;
}

async function handleSnapshot(page) {
  const title = await page.title();
  const url = page.url();

  // Get a readable text snapshot of the page.
  const text = await page.evaluate(() => {
    const walker = document.createTreeWalker(
      document.body,
      NodeFilter.SHOW_TEXT,
      {
        acceptNode: (node) => {
          const tag = node.parentElement?.tagName;
          if (tag === "SCRIPT" || tag === "STYLE" || tag === "NOSCRIPT")
            return NodeFilter.FILTER_REJECT;
          if (node.textContent.trim() === "") return NodeFilter.FILTER_REJECT;
          return NodeFilter.FILTER_ACCEPT;
        },
      }
    );
    const parts = [];
    let node;
    while ((node = walker.nextNode())) {
      parts.push(node.textContent.trim());
    }
    return parts.join("\n");
  });

  const maxLen = 32000;
  const truncated =
    text.length > maxLen ? text.substring(0, maxLen) + "\n<truncated>" : text;

  return `url=${url}\ntitle=${title}\n\n${truncated}\n`;
}

async function handleClick(page, input) {
  const selector = input.selector;
  if (!selector) throw new Error("click requires selector");
  await page.click(selector, { timeout: 10000 });
  return `Clicked: ${selector}\n`;
}

async function handleFill(page, input) {
  const { selector, value } = input;
  if (!selector) throw new Error("fill requires selector");
  if (value === undefined) throw new Error("fill requires value");
  await page.fill(selector, value, { timeout: 10000 });
  return `Filled ${selector} with value\n`;
}

async function handleType(page, input) {
  const { selector, text } = input;
  if (!selector) throw new Error("type requires selector");
  if (!text) throw new Error("type requires text");
  await page.type(selector, text, { timeout: 10000 });
  return `Typed into ${selector}\n`;
}

async function handleGetText(page, input) {
  const selector = input.selector;
  if (!selector) throw new Error("get_text requires selector");

  const elements = await page.$$(selector);
  if (elements.length === 0) {
    return `No elements found for: ${selector}\n`;
  }

  const texts = [];
  for (const el of elements) {
    const text = await el.textContent();
    if (text && text.trim()) {
      texts.push(text.trim());
    }
  }
  return texts.join("\n") + "\n";
}

async function handleGetTitle(page) {
  const title = await page.title();
  return `${title}\n`;
}

async function handleGetUrl(page) {
  return `${page.url()}\n`;
}

async function handleScreenshot(page, input) {
  const screenshotPath =
    input.path || path.join(SCREENSHOT_DIR, "screenshot.png");
  await page.screenshot({ path: screenshotPath, fullPage: false });
  return `Screenshot saved to ${screenshotPath}\n`;
}

async function handleWait(page, input) {
  if (input.selector) {
    await page.waitForSelector(input.selector, { timeout: 30000 });
    return `Element found: ${input.selector}\n`;
  }
  if (input.ms) {
    await page.waitForTimeout(input.ms);
    return `Waited ${input.ms}ms\n`;
  }
  await page.waitForLoadState("networkidle", { timeout: 30000 });
  return "Network idle\n";
}

async function handlePress(page, input) {
  const key = input.key;
  if (!key) throw new Error("press requires key");
  await page.keyboard.press(key);
  return `Pressed: ${key}\n`;
}

async function handleHover(page, input) {
  const selector = input.selector;
  if (!selector) throw new Error("hover requires selector");
  await page.hover(selector, { timeout: 10000 });
  return `Hovered: ${selector}\n`;
}

async function handleScroll(page, input) {
  const direction = input.direction || "down";
  const distance = direction === "up" ? -500 : 500;
  await page.evaluate((d) => window.scrollBy(0, d), distance);
  return `Scrolled ${direction}\n`;
}

main().catch((e) => {
  console.error(`fatal: ${e.message}`);
  process.exit(1);
});
