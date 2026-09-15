import { test, expect } from "@playwright/test";
import { existsSync } from "node:fs";

test.beforeEach(async ({ page }) => {
  await expect.poll(() => existsSync(".e2e-ready"), { timeout: 30_000 }).toBe(true);
  await page.goto("/");
  await page.getByLabel("Username", { exact: true }).fill("browser");
  await page.getByLabel("Password", { exact: true }).fill("browser-test-password");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await page.getByRole("navigation", { name: "Primary" }).getByRole("link", { name: "Libraries" }).click();
  await expect(page.getByRole("heading", { name: "Libraries", exact: true })).toBeVisible();
});

test("movies show stored metadata and play through a scoped ticket", async ({ page }) => {
  await page.getByRole("navigation", { name: "Jellymax libraries" }).getByRole("link", { name: "Movies" }).click();
  await page.getByRole("link", { name: /Example Movie/ }).click();
  await expect(page.getByRole("heading", { name: "Example Movie", exact: true })).toBeVisible();
  await expect(page.getByText("Movie description", { exact: true })).toBeVisible();
  await expect(page.getByRole("link", { name: "View on TMDb" })).toHaveAttribute("href", "https://www.themoviedb.org/movie/42");
  await page.getByRole("link", { name: "▶ Play", exact: true }).click();
  const video = page.locator("video");
  await expect(video).toHaveAttribute("src", /PlaybackTicket=/);
  await expect(video).toHaveAttribute("src", /\/stream\?/);
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.readyState)).toBeGreaterThanOrEqual(2);
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.currentTime)).toBeGreaterThan(0);
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("custom controls seek and enable a converted subtitle track", async ({ page }) => {
  await page.getByRole("navigation", { name: "Jellymax libraries" }).getByRole("link", { name: "Movies" }).click();
  await page.getByRole("link", { name: /Example Movie/ }).click();
  await page.getByRole("link", { name: "▶ Play", exact: true }).click();
  const video = page.locator("video");
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.readyState)).toBeGreaterThanOrEqual(2);
  await video.evaluate((v: HTMLVideoElement) => { v.addTextTrack("subtitles", "Embedded track").mode = "disabled"; });
  await page.getByRole("button", { name: "Pause", exact: true }).click();
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.paused)).toBe(true);
  await page.getByRole("button", { name: "Subtitles" }).click();
  await page.getByRole("menuitemradio", { name: /EN/ }).click();
  await expect.poll(() => page.locator("video track").evaluate((track: HTMLTrackElement) => track.track.mode)).toBe("hidden");
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => Array.from(v.textTracks).find((track) => track.label === "Embedded track")?.mode)).toBe("disabled");
  await video.evaluate((v: HTMLVideoElement) => { v.currentTime = 0.5; });
  await expect.poll(() => page.locator("video track").evaluate((track: HTMLTrackElement) => track.track.activeCues?.length)).toBeGreaterThan(0);
  await expect(page.getByText("A sample subtitle", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Mute" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Back 10 seconds" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Picture-in-picture" })).toBeVisible();
  await page.getByRole("button", { name: "Subtitles" }).click();
  await page.getByRole("menuitemradio", { name: "Off" }).click();
  await expect(page.locator("video track")).toHaveCount(0);
  await page.getByRole("button", { name: "Forward 10 seconds" }).click();
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.currentTime)).toBeGreaterThan(2);
});

test("CC remains visible without a usable track and native video controls stay off", async ({ page }) => {
  await page.getByRole("navigation", { name: "Jellymax libraries" }).getByRole("link", { name: "Movies" }).click();
  await page.getByRole("link", { name: /Chrome Audio/ }).click();
  await page.getByRole("link", { name: "▶ Play", exact: true }).click();
  const video = page.locator("video");
  await expect(video).toBeVisible();
  await expect(page.getByRole("button", { name: "Subtitles" })).toBeVisible();
  await page.getByRole("button", { name: "Subtitles" }).click();
  await expect(page.getByText("No subtitles are available for this item.")).toBeVisible();
  await expect.poll(() => video.evaluate((element: HTMLVideoElement) => element.controls)).toBe(false);
  const contextMenuPrevented = await video.evaluate((element: HTMLVideoElement) =>
    !element.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true })));
  expect(contextMenuPrevented).toBe(true);
  await video.evaluate((element: HTMLVideoElement) => { element.controls = true; });
  await expect.poll(() => video.evaluate((element: HTMLVideoElement) => element.controls)).toBe(false);
});

test("imported subtitle is selectable and survives a page reload", async ({ page }) => {
  await page.getByRole("navigation", { name: "Jellymax libraries" }).getByRole("link", { name: "Movies" }).click();
  await page.getByRole("link", { name: /Chrome Audio/ }).click();
  await page.getByRole("link", { name: "▶ Play", exact: true }).click();
  const video = page.locator("video");
  await expect.poll(() => video.evaluate((element: HTMLVideoElement) => element.readyState)).toBeGreaterThanOrEqual(2);
  await page.getByRole("button", { name: "Subtitles" }).click();
  await page.locator('input[aria-label="Import subtitle file"]').setInputFiles({
    name: "my-subtitle.en.srt", mimeType: "text/plain",
    buffer: Buffer.from("1\n00:00:01,000 --> 00:00:05,000\nImported caption\n"),
  });
  await video.evaluate((element: HTMLVideoElement) => { element.currentTime = 2; });
  await expect(page.getByText("Imported caption", { exact: true })).toBeVisible();
  await page.reload();
  await expect(page.locator("video")).toBeVisible();
  await page.getByRole("button", { name: "Subtitles" }).click();
  await expect(page.getByRole("menuitemradio", { name: /my-subtitle.en.srt/ })).toBeVisible();
  await page.getByRole("menuitem", { name: "Get more from OpenSubtitles…" }).click();
  await page.getByRole("button", { name: "Search", exact: true }).click();
  await page.getByRole("menuitem", { name: /Example.en.srt/ }).click();
  await video.evaluate((element: HTMLVideoElement) => { element.currentTime = 2; });
  await expect(page.getByText("Downloaded caption", { exact: true })).toBeVisible();
  await page.reload();
  await page.getByRole("button", { name: "Subtitles" }).click();
  await expect(page.getByRole("menuitemradio", { name: /Example.en.srt/ })).toBeVisible();
});

test("OpenSubtitles authorization errors identify search and download failures", async ({ page }) => {
  await page.getByRole("navigation", { name: "Jellymax libraries" }).getByRole("link", { name: "Movies" }).click();
  await page.getByRole("link", { name: /Chrome Audio/ }).click();
  await page.getByRole("link", { name: /▶ (Play|Resume)/ }).click();
  await page.getByRole("button", { name: "Subtitles" }).click();
  await page.getByRole("menuitem", { name: "Get more from OpenSubtitles…" }).click();
  await page.getByRole("button", { name: "Search", exact: true }).click();
  await page.getByRole("menuitem", { name: /Denied.en.srt/ }).click();
  await expect(page.getByText(/rejected download authorization \(401\).*Account download access denied/)).toBeVisible();
  await page.getByRole("textbox", { name: "Subtitle language" }).fill("zz");
  await page.getByRole("button", { name: "Search", exact: true }).click();
  await expect(page.getByText(/rejected the API key during search \(401\).*API key not authorized for search/)).toBeVisible();
});

test("captions sit lower after custom controls disappear", async ({ page }) => {
  await page.getByRole("navigation", { name: "Jellymax libraries" }).getByRole("link", { name: "Movies" }).click();
  await page.getByRole("link", { name: /Windowed Subtitles/ }).click();
  await page.getByRole("link", { name: "▶ Play", exact: true }).click();
  const video = page.locator("video");
  await expect.poll(() => video.evaluate((element: HTMLVideoElement) => element.readyState)).toBeGreaterThanOrEqual(2);
  await page.getByRole("button", { name: "Subtitles" }).click();
  await page.getByRole("menuitemradio", { name: /Unknown language/ }).click();
  await video.evaluate((element: HTMLVideoElement) => { element.currentTime = 2; });
  const caption = page.getByText("Opening caption", { exact: true }).locator("..");
  await expect(caption).toHaveClass(/bottom-24/);
  await video.hover();
  await page.mouse.move(0, 0);
  await expect(caption).toHaveClass(/bottom-5/);
});

test("Chromium keeps subtitles visible above controls during HLS playback", async ({ page }) => {
  test.skip(await page.evaluate(() => navigator.vendor.includes("Apple")), "Chromium subtitle overlay test");
  await page.getByRole("navigation", { name: "Jellymax libraries" }).getByRole("link", { name: "Movies" }).click();
  await page.getByRole("link", { name: /Needs Transcode/ }).click();
  await page.getByRole("link", { name: "▶ Play", exact: true }).click();
  const video = page.locator("video");
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.readyState), { timeout: 20_000 }).toBeGreaterThanOrEqual(2);
  await page.getByRole("button", { name: "Subtitles" }).click();
  await page.getByRole("menuitemradio", { name: /EN/ }).click();
  await video.evaluate((v: HTMLVideoElement) => { v.currentTime = 1; });
  await expect(page.getByText("HLS subtitle", { exact: true })).toBeVisible();
  await video.evaluate((v: HTMLVideoElement) => { v.currentTime = 11; });
  await expect(page.getByText("Subtitle after seek", { exact: true })).toBeVisible();
});

test("a failed subtitle fetch can be retried without restarting playback", async ({ page }) => {
  await page.getByRole("navigation", { name: "Jellymax libraries" }).getByRole("link", { name: "Movies" }).click();
  await page.getByRole("link", { name: /Example Movie/ }).click();
  await page.getByRole("link", { name: /▶ (Play|Resume)/ }).click();
  const video = page.locator("video");
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.readyState)).toBeGreaterThanOrEqual(2);
  let requests = 0;
  await page.route("**/Items/*/Subtitles/*", async (route) => {
    requests += 1;
    if (requests === 1) await route.fulfill({ status: 503, body: "Unavailable" });
    else await route.continue();
  });
  await page.getByRole("button", { name: "Subtitles" }).click();
  await page.getByRole("menuitemradio", { name: /EN/ }).click();
  await expect(page.getByText("Subtitle request failed (503). Choose the track to retry.")).toBeVisible();
  await page.getByRole("button", { name: "Subtitles" }).click();
  await page.getByRole("menuitemradio", { name: /EN/ }).click();
  await video.evaluate((v: HTMLVideoElement) => { v.currentTime = 0.5; });
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.textTracks[0]?.activeCues?.length)).toBeGreaterThan(0);
  await expect(page.getByText("Subtitle request failed (503). Choose the track to retry.")).toHaveCount(0);
});

test("picture-in-picture control enters and exits the floating player", async ({ page }) => {
  test.skip(await page.evaluate(() => navigator.vendor.includes("Apple")), "Chromium picture-in-picture test");
  await page.getByRole("navigation", { name: "Jellymax libraries" }).getByRole("link", { name: "Movies" }).click();
  await page.getByRole("link", { name: /Example Movie/ }).click();
  await page.getByRole("link", { name: "▶ Play", exact: true }).click();
  const video = page.locator("video");
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.readyState)).toBeGreaterThanOrEqual(2);
  await page.getByRole("button", { name: "Picture-in-picture" }).click();
  await expect.poll(() => page.evaluate(() => document.pictureInPictureElement?.tagName)).toBe("VIDEO");
  await page.getByRole("button", { name: "Exit picture-in-picture" }).click();
  await expect.poll(() => page.evaluate(() => document.pictureInPictureElement)).toBe(null);
});

test("an interrupted play request does not replace the player with an error", async ({ page }) => {
  await page.getByRole("navigation", { name: "Jellymax libraries" }).getByRole("link", { name: "Movies" }).click();
  await page.getByRole("link", { name: /Example Movie/ }).click();
  await page.getByRole("link", { name: /▶ (Play|Resume)/ }).click();
  const video = page.locator("video");
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.readyState)).toBeGreaterThanOrEqual(2);
  await page.getByRole("button", { name: "Pause", exact: true }).click();
  await video.evaluate((v: HTMLVideoElement) => { v.play = () => Promise.reject(new DOMException("interrupted", "AbortError")); });
  await page.getByRole("button", { name: "Play", exact: true }).click();
  await expect(video).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);
  await video.evaluate((v: HTMLVideoElement) => { v.play = () => Promise.reject(new DOMException("blocked", "NotAllowedError")); });
  await page.getByRole("button", { name: "Play", exact: true }).click();
  await expect(page.getByText("The browser blocked playback. Press Play to try again.")).toBeVisible();
  await video.evaluate((v: HTMLVideoElement) => { v.play = HTMLMediaElement.prototype.play.bind(v); });
  await page.getByRole("button", { name: "Play", exact: true }).click();
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.paused)).toBe(false);
  await expect(page.getByText("The browser blocked playback. Press Play to try again.")).toHaveCount(0);
});

test("embedded text subtitles are available in the selector", async ({ page }) => {
  await page.getByRole("navigation", { name: "Jellymax libraries" }).getByRole("link", { name: "Movies" }).click();
  await page.getByRole("link", { name: /Embedded Subtitles/ }).click();
  await page.getByRole("link", { name: "▶ Play", exact: true }).click();
  const video = page.locator("video");
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.readyState)).toBeGreaterThanOrEqual(2);
  await page.getByRole("button", { name: "Subtitles" }).click();
  const subtitleRequest = page.waitForRequest((request) => request.url().includes("/Subtitles/"));
  await page.getByRole("menuitemradio", { name: /Unknown language/ }).click();
  const fetched = await subtitleRequest;
  expect(fetched.url()).not.toContain("StartSeconds");
  const complete = await page.request.get(fetched.url());
  expect(complete.ok()).toBe(true);
  expect(await complete.text()).toContain("00:02.600");
  await video.evaluate((v: HTMLVideoElement) => { v.currentTime = 0.5; });
  await expect(page.getByText("A sample subtitle", { exact: true })).toBeVisible({ timeout: 15_000 });
  await video.evaluate((v: HTMLVideoElement) => { v.currentTime = 2.95; });
  await expect(page.getByText("A sample subtitle", { exact: true })).toHaveCount(0);
});

test("default English and unlabeled MKV subtitle tracks can both be selected", async ({ page }) => {
  await page.getByRole("navigation", { name: "Jellymax libraries" }).getByRole("link", { name: "Movies" }).click();
  await page.getByRole("link", { name: /Dual Subtitles/ }).click();
  await page.getByRole("link", { name: "▶ Play", exact: true }).click();
  const video = page.locator("video");
  await expect.poll(() => video.evaluate((element: HTMLVideoElement) => element.readyState), { timeout: 20_000 }).toBeGreaterThanOrEqual(2);
  await page.getByRole("button", { name: "Pause", exact: true }).click();
  await page.getByRole("button", { name: "Subtitles" }).click();
  await page.getByRole("menuitemradio", { name: /ENG · Default · embedded SUBRIP/ }).click();
  await video.evaluate((element: HTMLVideoElement) => { element.currentTime = 2; });
  await expect(page.getByText("Opening caption", { exact: true })).toBeVisible({ timeout: 15_000 });
  await page.getByRole("button", { name: "Subtitles" }).click();
  await page.getByRole("menuitemradio", { name: /Unknown language · embedded SUBRIP/ }).click();
  await expect(page.getByText("Second track caption", { exact: true })).toBeVisible({ timeout: 15_000 });
});

test("embedded MKV subtitles follow a Safari HLS seek into a new window", async ({ page }) => {
  test.skip(!await page.evaluate(() => navigator.vendor.includes("Apple")), "Safari HLS subtitle test");
  await page.getByRole("navigation", { name: "Jellymax libraries" }).getByRole("link", { name: "Movies" }).click();
  await page.getByRole("link", { name: /Windowed Subtitles/ }).click();
  await page.getByRole("link", { name: "▶ Play", exact: true }).click();
  const video = page.locator("video");
  await expect(video).toHaveAttribute("src", /master\.m3u8/);
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.readyState), { timeout: 20_000 }).toBeGreaterThanOrEqual(2);
  await page.getByRole("button", { name: "Subtitles" }).click();
  await page.getByRole("menuitemradio", { name: /Unknown language/ }).click();
  await video.evaluate((v: HTMLVideoElement) => { v.currentTime = 2; });
  await expect(page.getByText("Opening caption", { exact: true })).toBeVisible({ timeout: 15_000 });
  await video.evaluate((v: HTMLVideoElement) => { v.currentTime = 49; });
  await expect(page.getByText("Caption after seek", { exact: true })).toBeVisible({ timeout: 20_000 });
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("remote embedded subtitles survive repeated seeks and track switches", async ({ page }) => {
  await page.getByRole("navigation", { name: "Mock Jellyfin libraries" }).getByRole("link", { name: "Remote Movies" }).click();
  await page.getByRole("link", { name: /Remote Dual Subtitles/ }).click();
  await page.getByRole("link", { name: "▶ Play", exact: true }).click();
  const video = page.locator("video");
  await expect.poll(() => video.evaluate((element: HTMLVideoElement) => element.readyState), { timeout: 20_000 }).toBeGreaterThanOrEqual(2);
  await page.getByRole("button", { name: "Pause", exact: true }).click();
  await video.evaluate((element: HTMLVideoElement) => { element.currentTime = 49; });
  const subtitleRequest = page.waitForRequest((request) => request.url().includes("/Subtitles/") && request.url().includes("StartSeconds=30"));
  await page.getByRole("button", { name: "Subtitles" }).click();
  await page.getByRole("menuitemradio", { name: /ENG · Default · embedded SUBRIP/ }).click();
  await subtitleRequest;
  await expect(page.getByText("Caption after seek", { exact: true })).toBeVisible({ timeout: 15_000 });
  await video.evaluate((element: HTMLVideoElement) => { element.currentTime = 82; });
  await expect.poll(() => video.evaluate((element: HTMLVideoElement) => element.currentTime), { timeout: 15_000 }).toBeGreaterThan(79);
  await expect(page.getByText("Caption in former gap", { exact: true })).toBeVisible({ timeout: 15_000 });
  await video.evaluate((element: HTMLVideoElement) => { element.currentTime = 2; });
  await page.getByRole("button", { name: "Subtitles" }).click();
  await page.getByRole("menuitemradio", { name: /ENG · SDH · embedded SUBRIP/ }).click();
  await expect(page.getByText("Second track caption", { exact: true })).toBeVisible({ timeout: 15_000 });
  await video.evaluate((element: HTMLVideoElement) => { element.currentTime = 49; });
  await page.getByRole("button", { name: "Subtitles" }).click();
  await page.getByRole("menuitemradio", { name: /ENG · Default · embedded SUBRIP/ }).click();
  await expect(page.getByText("Caption after seek", { exact: true })).toBeVisible({ timeout: 15_000 });
  await video.evaluate((element: HTMLVideoElement) => { element.currentTime = 2; });
  await expect(page.getByText("Opening caption", { exact: true })).toBeVisible({ timeout: 15_000 });
});

test("a remote subtitle cue appears before its window finishes streaming", async ({ page }) => {
  await page.getByRole("navigation", { name: "Mock Jellyfin libraries" }).getByRole("link", { name: "Remote Movies" }).click();
  await page.getByRole("link", { name: /Remote Dual Subtitles/ }).click();
  await page.getByRole("link", { name: "▶ Play", exact: true }).click();
  const video = page.locator("video");
  await expect.poll(() => video.evaluate((element: HTMLVideoElement) => element.readyState), { timeout: 20_000 }).toBeGreaterThanOrEqual(2);
  await page.getByRole("button", { name: "Pause", exact: true }).click();
  await video.evaluate((element: HTMLVideoElement) => { element.currentTime = 2; });
  await page.route("**/Items/browser-remote-mkv/Subtitles/*", (route) => route.continue({ url: "http://127.0.0.1:18099/streamed-vtt" }));
  let finished = false;
  page.on("requestfinished", (request) => {
    if (request.url().includes("/Items/browser-remote-mkv/Subtitles/") || request.url().includes("/streamed-vtt")) finished = true;
  });
  await page.getByRole("button", { name: "Subtitles" }).click();
  await page.getByRole("menuitemradio", { name: /ENG · Default · embedded SUBRIP/ }).click();
  await expect(page.getByText("First streamed subtitle", { exact: true })).toBeVisible({ timeout: 1500 });
  expect(finished).toBe(false);
  await expect.poll(() => finished).toBe(true);
});

test("nonstandard movies direct-play when possible and remain fully seekable", async ({ page }) => {
  await page.getByRole("navigation", { name: "Jellymax libraries" }).getByRole("link", { name: "Movies" }).click();
  await page.getByRole("link", { name: /Needs Transcode/ }).click();
  await page.getByRole("link", { name: "▶ Play", exact: true }).click();
  const video = page.locator("video");
  await expect(video).toHaveAttribute("src", /\/stream\?|master\.m3u8|^blob:/);
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.readyState), { timeout: 20_000 }).toBeGreaterThanOrEqual(2);
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.duration)).toBeGreaterThan(12);
  await video.evaluate((v: HTMLVideoElement) => { v.currentTime = 11; });
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.currentTime)).toBeGreaterThan(10);
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("sparse-keyframe media plays through a Safari seek near the end", async ({ page }) => {
  await page.getByRole("navigation", { name: "Jellymax libraries" }).getByRole("link", { name: "Movies" }).click();
  await page.getByRole("link", { name: /Sparse Keyframes/ }).click();
  await page.getByRole("link", { name: "▶ Play", exact: true }).click();
  const video = page.locator("video");
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.readyState), { timeout: 20_000 }).toBeGreaterThanOrEqual(2);
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.duration)).toBeGreaterThan(40);
  await video.evaluate((v: HTMLVideoElement) => { v.currentTime = 42; });
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.currentTime), { timeout: 20_000 }).toBeGreaterThan(42);
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("Safari resumes HLS near the end without waiting on the opening segment", async ({ page }) => {
  await page.getByRole("navigation", { name: "Jellymax libraries" }).getByRole("link", { name: "Movies" }).click();
  await page.getByRole("link", { name: /Sparse Keyframes/ }).click();
  await page.evaluate(async () => {
    const itemId = window.location.pathname.split("/").pop();
    const token = localStorage.getItem("jellymax_token");
    const response = await fetch("/Sessions/Playing/Progress", {
      method: "POST", headers: { "Content-Type": "application/json", "X-Emby-Token": token ?? "" },
      body: JSON.stringify({ ItemId: itemId, PositionTicks: 420_000_000 }),
    });
    if (!response.ok) throw new Error(`Progress update failed: ${response.status}`);
  });
  await page.reload();
  await page.getByRole("link", { name: "▶ Resume", exact: true }).click();
  const video = page.locator("video");
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.currentTime), { timeout: 20_000 }).toBeGreaterThan(42);
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("incompatible audio uses a normalized seekable compatibility stream", async ({ page }) => {
  await page.getByRole("navigation", { name: "Jellymax libraries" }).getByRole("link", { name: "Movies" }).click();
  await page.getByRole("link", { name: /Chrome Audio/ }).click();
  await page.getByRole("link", { name: /▶ (Play|Resume)/ }).click();
  const video = page.locator("video");
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.readyState), { timeout: 20_000 }).toBeGreaterThanOrEqual(2);
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.duration)).toBeGreaterThan(12);
  if (!(await page.evaluate(() => navigator.vendor.includes("Apple")))) {
    await expect(page.getByText(/Converting audio|Transcoding for browser compatibility/)).toBeVisible();
    await expect(page.locator("audio")).toHaveCount(0);
  }
  await video.evaluate((v: HTMLVideoElement) => { v.currentTime = 11; });
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.currentTime)).toBeGreaterThan(10);
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("incompatible audio and HLS video are converted together", async ({ page }) => {
  await page.getByRole("navigation", { name: "Jellymax libraries" }).getByRole("link", { name: "Movies" }).click();
  await page.getByRole("link", { name: /Mixed Codecs/ }).click();
  await page.getByRole("link", { name: "▶ Play", exact: true }).click();
  const video = page.locator("video");
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.readyState), { timeout: 20_000 }).toBeGreaterThanOrEqual(2);
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.duration)).toBeGreaterThan(12);
  if (!(await page.evaluate(() => navigator.vendor.includes("Apple")))) {
    await expect(page.getByText(/Transcoding/)).toBeVisible();
  }
  await video.evaluate((v: HTMLVideoElement) => { v.currentTime = 11; });
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.currentTime)).toBeGreaterThan(10);
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("series navigate seasons, ordered episodes, breadcrumbs and next season", async ({ page }) => {
  await page.getByRole("navigation", { name: "Jellymax libraries" }).getByRole("link", { name: "Series" }).click();
  await page.getByRole("link", { name: /Example Show/ }).click();
  await expect(page.getByRole("heading", { name: "Seasons", exact: true })).toBeVisible();
  await expect(page.getByRole("link", { name: "▶ Play", exact: true })).toHaveCount(0);
  await page.getByRole("link", { name: /Season 1.*2 episodes/ }).click();
  await expect(page.getByRole("heading", { name: "Episodes", exact: true })).toBeVisible();
  const cards = page.locator('section[aria-label="Episodes"] a');
  await expect(cards.nth(0)).toContainText("E02");
  await expect(cards.nth(1)).toContainText("E10");
  await page.getByRole("link", { name: /Episode Two/ }).click();
  await expect(page.getByRole("heading", { name: "Episode Two", exact: true })).toBeVisible();
  await expect(page.getByRole("navigation", { name: "Breadcrumb" }).getByRole("link", { name: "Example Show" })).toBeVisible();
  await expect(page.getByRole("link", { name: "View on TMDb" })).toHaveAttribute("href", "https://www.themoviedb.org/tv/100/season/1/episode/2");
  await page.getByRole("link", { name: /Next\s+Episode/ }).click();
  await expect(page.getByRole("heading", { name: "S01E10", exact: true })).toBeVisible();
  await page.getByRole("link", { name: /Next\s+Episode/ }).click();
  await expect(page.getByRole("heading", { name: "S02E01", exact: true })).toBeVisible();
  await page.getByRole("navigation", { name: "Breadcrumb" }).getByRole("link", { name: "Season 2" }).click();
  await expect(page.getByRole("heading", { name: "Season 2", exact: true })).toBeVisible();
  await page.reload();
  await expect(page.getByRole("heading", { name: "Season 2", exact: true })).toBeVisible();
});
