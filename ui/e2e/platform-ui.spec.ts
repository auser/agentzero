import { test, expect } from '@playwright/test'

/**
 * Platform UI end-to-end tests.
 *
 * Prerequisites:
 *   1. Gateway daemon running: `agentzero daemon start`
 *   2. Vite dev server running: `cd ui && pnpm run dev`
 *   3. A pairing code available (or existing paired token)
 *
 * These tests assume the gateway is on port 42617 and Vite on port 5173.
 * The tests run sequentially — later tests depend on state from earlier ones
 * (e.g. agents created, runs submitted).
 */

// Each test logs in via the token login form.
// Set AGENTZERO_TEST_TOKEN to a valid paired token.

// ---------------------------------------------------------------------------
// Login & Pairing
// ---------------------------------------------------------------------------

test.describe('Login & Pairing', () => {
  test('redirects unauthenticated users to login', async ({ page }) => {
    // Clear any stored token
    await page.goto('/')
    await page.evaluate(() => localStorage.clear())
    await page.goto('/dashboard')
    // Should show login form (the Shell renders login content when no token)
    await expect(page.getByText('Connect to your gateway')).toBeVisible()
  })

  test('shows error on wrong pairing code', async ({ page }) => {
    await page.goto('/login')
    await page.evaluate(() => localStorage.clear())
    await page.goto('/login')
    await page.getByRole('textbox', { name: /XXXX/ }).fill('000000')
    await page.getByRole('button', { name: 'Pair' }).click()
    // Should show an error in the pairing card
    const pairingForm = page.locator('form').filter({ hasText: 'Pairing Code' })
    await expect(pairingForm.locator('p.text-destructive')).toBeVisible({ timeout: 5000 })
  })

  test('connects with token login', async ({ page }) => {
    const token = process.env.AGENTZERO_TEST_TOKEN
    if (!token) { test.skip(); return }

    await page.goto('/login')
    await page.getByPlaceholder('az_...').fill(token)
    await page.getByRole('button', { name: 'Connect' }).click()
    await expect(page.getByText('Active Agents')).toBeVisible({ timeout: 10_000 })
  })
})

// ---------------------------------------------------------------------------
// Helper: ensure authenticated
// ---------------------------------------------------------------------------

/**
 * Ensure the page is authenticated. Navigates to /login, sets the token
 * in localStorage, and waits for the app to redirect to /dashboard.
 * Requires AGENTZERO_TEST_TOKEN env var.
 */
async function ensureAuth(page: import('@playwright/test').Page) {
  const envToken = process.env.AGENTZERO_TEST_TOKEN
  if (!envToken) throw new Error('Set AGENTZERO_TEST_TOKEN env var.')

  // Check if already authenticated (page might already be on the app)
  const url = page.url()
  if (url.includes('localhost:5173') && !url.includes('/login')) {
    // Already on an app page — check if token is set
    const hasToken = await page.evaluate(() => {
      const raw = localStorage.getItem('agentzero-auth')
      return raw ? JSON.parse(raw)?.state?.token : null
    })
    if (hasToken) return
  }

  // Navigate to login and use the Token Login form
  await page.goto('/login')
  await page.getByPlaceholder('az_...').fill(envToken)
  await page.getByRole('button', { name: 'Connect' }).click()
  // Token login navigates to /dashboard client-side (no round-trip)
  await expect(page.getByText('Active Agents')).toBeVisible({ timeout: 10_000 })
}

// ---------------------------------------------------------------------------
// Dashboard
// ---------------------------------------------------------------------------

test.describe('Dashboard', () => {
  test('loads and shows gateway status', async ({ page }) => {
    await ensureAuth(page)
    await page.goto('/dashboard')
    await page.waitForLoadState('networkidle')
    await expect(page.getByText('Active Agents')).toBeVisible({ timeout: 10_000 })
  })
})

// ---------------------------------------------------------------------------
// Agents CRUD
// ---------------------------------------------------------------------------

test.describe('Agents', () => {
  test('shows empty agents list', async ({ page }) => {
    await ensureAuth(page)
    await page.goto('/agents')
    await expect(page.getByRole('heading', { name: 'Agents' })).toBeVisible()
    await expect(page.getByRole('button', { name: 'New Agent' })).toBeVisible()
  })

  test('creates a new agent', async ({ page }) => {
    await ensureAuth(page)
    await page.goto('/agents')
    await page.getByRole('button', { name: 'New Agent' }).click()

    // Fill the form
    await page.getByRole('textbox').first().fill('test-agent')
    await page.getByRole('textbox').nth(1).fill('E2E test agent')
    await page.getByRole('textbox', { name: /You are a helpful/ }).fill('You are a test agent.')
    await page.getByRole('textbox', { name: /travel/ }).fill('test, e2e')

    await page.getByRole('button', { name: 'Create' }).click()

    // Verify agent appears in table
    await expect(page.getByText('test-agent')).toBeVisible()
    await expect(page.getByText('E2E test agent')).toBeVisible()
  })

  test('edits an agent', async ({ page }) => {
    await ensureAuth(page)
    await page.goto('/agents')
    await expect(page.getByText('test-agent')).toBeVisible()

    // Click the edit button (first icon button in the agent row actions)
    const row = page.getByRole('row', { name: /test-agent/ })
    await row.getByRole('button').first().click()

    // Verify edit dialog opens
    await expect(page.getByRole('heading', { name: 'Edit Agent' })).toBeVisible()

    // Update description
    await page.getByRole('textbox').nth(1).fill('Updated test agent')
    await page.getByRole('button', { name: 'Update' }).click()

    // Verify update
    await expect(page.getByText('Updated test agent')).toBeVisible()
  })

  test('deletes an agent', async ({ page }) => {
    await ensureAuth(page)
    await page.goto('/agents')
    await expect(page.getByText('test-agent')).toBeVisible()

    // Click the delete button (second icon button in actions)
    const row = page.getByRole('row', { name: /test-agent/ })
    await row.getByRole('button').nth(1).click()

    // Confirm deletion
    await expect(page.getByText('Delete "test-agent"?')).toBeVisible()
    await page.getByRole('button', { name: 'Delete' }).click()

    // Verify agent is gone
    await expect(page.getByText('test-agent')).not.toBeVisible()
  })
})

// ---------------------------------------------------------------------------
// Runs
// ---------------------------------------------------------------------------

test.describe('Runs', () => {
  test('shows runs page with filters', async ({ page }) => {
    await ensureAuth(page)
    await page.goto('/runs')
    await expect(page.getByRole('heading', { name: 'Runs' })).toBeVisible()
    await expect(page.getByRole('button', { name: 'New Run' })).toBeVisible()
    await expect(page.getByRole('button', { name: 'E-Stop' })).toBeVisible()

    // Status filter buttons
    for (const status of ['All', 'Pending', 'Running', 'Completed', 'Failed', 'Cancelled']) {
      await expect(page.getByRole('button', { name: status })).toBeVisible()
    }
  })

  test('submits a run and waits for completion', async ({ page }) => {
    await ensureAuth(page)
    await page.goto('/runs')
    await page.getByRole('button', { name: 'New Run' }).click()

    await page.getByPlaceholder('What should the agent do?').fill('Say hello')
    await page.getByRole('button', { name: 'Submit' }).click()

    // Wait for run to appear in the table
    await expect(page.getByRole('cell', { name: /run-/ }).first()).toBeVisible({ timeout: 10_000 })

    // Wait for completion (poll every 2s for up to 30s)
    await expect(page.getByText('Completed')).toBeVisible({ timeout: 30_000 })
  })

  test('views run detail panel', async ({ page }) => {
    await ensureAuth(page)
    await page.goto('/runs')

    // Click on a completed run
    const row = page.getByRole('row', { name: /Completed/ }).first()
    await row.click()

    // Verify detail panel opens
    await expect(page.getByRole('tab', { name: 'Transcript' })).toBeVisible()
    await expect(page.getByRole('tab', { name: 'Tool Events' })).toBeVisible()

    // Switch to events tab
    await page.getByRole('tab', { name: 'Tool Events' }).click()
    const eventsPanel = page.getByRole('tabpanel', { name: 'Tool Events' })
    await expect(eventsPanel.getByText('created')).toBeVisible()
    await expect(eventsPanel.getByText('completed', { exact: true })).toBeVisible()
  })
})

// ---------------------------------------------------------------------------
// Chat
// ---------------------------------------------------------------------------

test.describe('Chat', () => {
  test('connects via WebSocket and shows Connected', async ({ page }) => {
    await ensureAuth(page)
    await page.goto('/chat')
    await expect(page.getByText('Connected', { exact: true })).toBeVisible({ timeout: 5_000 })
    await expect(page.getByPlaceholder('Message…')).toBeVisible()
  })

  test('sends a message and receives a response', async ({ page }) => {
    await ensureAuth(page)
    await page.goto('/chat')
    await expect(page.getByText('Connected', { exact: true })).toBeVisible({ timeout: 5_000 })

    await page.getByPlaceholder('Message…').fill('Say OK')
    await page.locator('form').getByRole('button').click()

    // User message should appear
    await expect(page.getByText('Say OK')).toBeVisible()

    // Wait for assistant response (up to 15s)
    await expect(page.locator('p').filter({ hasText: /OK|okay|Hello/i })).toBeVisible({ timeout: 15_000 })
  })
})

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

test.describe('Tools', () => {
  test('shows tool categories', async ({ page }) => {
    await ensureAuth(page)
    await page.goto('/tools')
    await expect(page.getByRole('heading', { name: 'Tools' })).toBeVisible()
    await expect(page.getByRole('button', { name: /file/ })).toBeVisible()
    await expect(page.getByRole('button', { name: /execution/ })).toBeVisible()
    await expect(page.getByRole('button', { name: /memory/ })).toBeVisible()
  })

  test('expands category and shows tool details', async ({ page }) => {
    await ensureAuth(page)
    await page.goto('/tools')
    await page.getByRole('button', { name: /file/ }).click()
    await expect(page.getByText('read_file')).toBeVisible()
    await expect(page.getByText('glob_search')).toBeVisible()
  })
})

// ---------------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------------

test.describe('Models', () => {
  test('lists providers and models', async ({ page }) => {
    await ensureAuth(page)
    await page.goto('/models')
    await expect(page.getByRole('heading', { name: 'Models' })).toBeVisible()
    await expect(page.getByRole('button', { name: 'Refresh' })).toBeVisible()
    // Should show at least one provider
    await expect(page.getByRole('heading', { level: 2 }).first()).toBeVisible()
  })
})

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

test.describe('Config', () => {
  test('shows config sections', async ({ page }) => {
    await ensureAuth(page)
    await page.goto('/config')
    await expect(page.getByRole('heading', { name: 'Config' })).toBeVisible()
    await expect(page.getByRole('button', { name: '[agent]' })).toBeVisible()
    await expect(page.getByRole('button', { name: '[security]' })).toBeVisible()
  })

  test('expands section and shows Edit button', async ({ page }) => {
    await ensureAuth(page)
    await page.goto('/config')
    await page.getByRole('button', { name: '[agent]' }).click()
    await expect(page.getByRole('button', { name: 'Edit' })).toBeVisible()
    await expect(page.getByText('max_tool_iterations:')).toBeVisible()
  })

  test('edits and saves a config section', async ({ page }) => {
    await ensureAuth(page)
    await page.goto('/config')
    await page.getByRole('button', { name: '[agent]' }).click()
    await page.getByRole('button', { name: 'Edit' }).click()

    // Verify JSON editor opened
    await expect(page.locator('textarea')).toBeVisible()
    await expect(page.getByRole('button', { name: 'Save' })).toBeVisible()
    await expect(page.getByRole('button', { name: 'Cancel' })).toBeVisible()

    // Cancel without saving
    await page.getByRole('button', { name: 'Cancel' }).click()
    await expect(page.locator('textarea')).not.toBeVisible()
  })
})

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

test.describe('Memory', () => {
  test('lists memory entries', async ({ page }) => {
    await ensureAuth(page)
    await page.goto('/memory')
    await expect(page.getByRole('heading', { name: 'Memory' })).toBeVisible()
    await expect(page.getByText(/\d+ entries/)).toBeVisible()
  })

  test('searches memory entries', async ({ page }) => {
    await ensureAuth(page)
    await page.goto('/memory')
    const initialCount = await page.getByText(/\d+ entries/).textContent()

    await page.getByPlaceholder('Search memory…').fill('README')
    await page.locator('form').getByRole('button').click()

    // Should show filtered results
    await expect(page.getByText(/\d+ entries/)).toBeVisible()
    const filteredCount = await page.getByText(/\d+ entries/).textContent()
    // Filtered count should be different (fewer) than initial
    expect(filteredCount).not.toBe(initialCount)
  })
})

// ---------------------------------------------------------------------------
// Channels
// ---------------------------------------------------------------------------

test.describe('Channels', () => {
  test('shows channel categories', async ({ page }) => {
    await ensureAuth(page)
    await page.goto('/channels')
    await expect(page.getByRole('heading', { name: 'Channels' })).toBeVisible()
    await expect(page.getByRole('heading', { name: 'Messaging' })).toBeVisible()
    await expect(page.getByText('Telegram').first()).toBeVisible()
    await expect(page.getByText('Discord').first()).toBeVisible()
    await expect(page.getByText('Slack').first()).toBeVisible()
  })
})

// ---------------------------------------------------------------------------
// Approvals
// ---------------------------------------------------------------------------

test.describe('Approvals', () => {
  test('shows approvals page', async ({ page }) => {
    await ensureAuth(page)
    await page.goto('/approvals')
    await expect(page.getByRole('heading', { name: 'Approvals' })).toBeVisible()
    await expect(page.getByText('No pending approvals')).toBeVisible()
  })
})

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

test.describe('Events', () => {
  test('shows events page with SSE connection', async ({ page }) => {
    await ensureAuth(page)
    await page.goto('/events')
    await expect(page.getByRole('heading', { name: 'Events' })).toBeVisible()
    await expect(page.getByText('Waiting for events')).toBeVisible()
    await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible()
    await expect(page.getByRole('button', { name: 'Clear' })).toBeVisible()
  })
})
