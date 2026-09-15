import { test, expect } from "@playwright/test";

test("a fresh server creates its first administrator in the browser", async ({ page }) => {
  let setupCalls = 0;
  await page.route("**/System/Info/Public", async (route) => {
    await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({
      ServerName: "Jellymax", StartupWizardCompleted: setupCalls > 0,
    }) });
  });
  await page.route("**/System/Setup", async (route) => {
    const body = route.request().postDataJSON() as { Name: string; Password: string };
    expect(body).toEqual({ Name: "admin", Password: "secure-admin-password" });
    setupCalls += 1;
    await route.fulfill({ status: 201, contentType: "application/json", body: JSON.stringify({
      Id: "first-admin", Name: "admin", Policy: { IsAdministrator: true },
    }) });
  });
  await page.route("**/Users/AuthenticateByName", async (route) => {
    await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({
      User: { Id: "first-admin", Name: "admin", Policy: { IsAdministrator: true } },
      AccessToken: "setup-test-token", ServerId: "test-server",
    }) });
  });

  await page.goto("/login");
  await expect(page.getByRole("button", { name: "Create administrator" })).toBeVisible();
  await page.getByLabel("Password", { exact: true }).fill("secure-admin-password");
  await page.getByLabel("Confirm password").fill("secure-admin-password");
  await page.getByRole("button", { name: "Create administrator" }).click();
  await expect.poll(() => setupCalls).toBe(1);
  await expect(page).toHaveURL(/\/$/);
  await page.reload();
  await expect(page.getByRole("button", { name: "Create administrator" })).toHaveCount(0);
});
