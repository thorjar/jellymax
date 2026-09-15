import { test, expect, type Page } from "@playwright/test";

// The app logo normally loads /jellymax-mark.svg from Vite's public/
// directory. Some deployments can miss that file (the static server then
// answers with the SPA fallback HTML at HTTP 200), so the sidebar and the
// login page must fall back to an inline icon tile. These specs cover both
// outcomes for both surfaces.

const user = { Id: "e2e-user", Name: "browser", Policy: { IsAdministrator: true } };

// Restore an authenticated session and stub the data calls so the main
// layout renders without the real backend.
async function mockSession(page: Page) {
  await page.addInitScript(() => localStorage.setItem("jellymax_token", "e2e-token"));
  await page.route("**/Users/Me", (route) => route.fulfill({
    status: 200, contentType: "application/json", body: JSON.stringify(user),
  }));
  // Items endpoints expect { Items, TotalRecordCount }; the rest, arrays.
  await page.route(/\/(Users\/[^/]+\/Items|Items)(\?|$|\/)/, (route) =>
    route.fulfill({ status: 200, contentType: "application/json",
      body: JSON.stringify({ Items: [], TotalRecordCount: 0 }) }));
  await page.route(/\/(Library|ScheduledTasks|RemoteServers|Recommendations)(\?|$|\/)/, (route) =>
    route.fulfill({ status: 200, contentType: "application/json", body: "[]" }));
}

// Answer the logo request like a static server with the file missing:
// SPA fallback HTML at HTTP 200.
async function serveMissingMark(page: Page) {
  await page.route("**/jellymax-mark.svg", (route) => route.fulfill({
    status: 200, contentType: "text/html", body: "<!doctype html><html></html>",
  }));
}

test.beforeEach(async ({ page }) => {
  await page.route("**/System/Info/Public", (route) => route.fulfill({
    status: 200, contentType: "application/json",
    body: JSON.stringify({ ServerName: "E2E", StartupWizardCompleted: true }),
  }));
});

test("sidebar logo loads the svg when it is served", async ({ page }) => {
  await mockSession(page);
  await page.goto("/");
  const logo = page.getByRole("link", { name: "Jellymax", exact: true });
  await expect(logo).toBeVisible();
  await expect(logo.locator('img[src="/jellymax-mark.svg"]')).toBeVisible();
  await expect(logo.locator("svg")).toHaveCount(0);
});

test("sidebar logo falls back to the icon tile when the svg is missing", async ({ page }) => {
  await mockSession(page);
  await serveMissingMark(page);
  await page.goto("/");
  const logo = page.getByRole("link", { name: "Jellymax", exact: true });
  await expect(logo).toBeVisible();
  await expect(logo.locator("span.grid.bg-brand svg")).toBeVisible();
  await expect(logo.locator("img")).toHaveCount(0);
});

test("login page logo loads the svg when it is served", async ({ page }) => {
  await page.goto("/login");
  await expect(page.getByRole("button", { name: "Sign in", exact: true })).toBeVisible();
  await expect(page.locator('form img[src="/jellymax-mark.svg"]')).toBeVisible();
  await expect(page.locator("form svg")).toHaveCount(0);
});

test("login page logo falls back to the icon tile when the svg is missing", async ({ page }) => {
  await serveMissingMark(page);
  await page.goto("/login");
  await expect(page.getByRole("button", { name: "Sign in", exact: true })).toBeVisible();
  await expect(page.locator("form span.grid.bg-brand svg")).toBeVisible();
  await expect(page.locator("form img")).toHaveCount(0);
});
