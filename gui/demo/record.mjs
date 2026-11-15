// Records demo/gui.gif: the real GUI frontend in Chromium, with the fake backend from demo/mock.ts.
// No Tauri, no LLM and no API key needed. Needs ffmpeg.
//
//   cd gui
//   npm install && npx playwright install chromium
//   npm run demo:gif

import { createServer } from "vite";
import { chromium } from "playwright";
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const GUI = resolve(fileURLToPath(import.meta.url), "../..");
const OUT = resolve(GUI, "../demo/gui.gif");
const SIZE = { width: 1100, height: 660 };
const FPS = 12;
const GIF_WIDTH = 900;

const server = await createServer({ root: GUI, logLevel: "warn", server: { port: 1421, strictPort: true } });
await server.listen();
const browser = await chromium.launch();
const frames = mkdtempSync(join(tmpdir(), "job-radar-gif-"));

try {
  const page = await browser.newPage({ viewport: SIZE, colorScheme: "light" });
  await page.goto("http://localhost:1421/demo/index.html");
  await page.getByText("Drop your CV here").waitFor();
  await addCursor(page);

  // Capture frames while the script below drives the app.
  const shots = [];
  let recording = true;
  const capture = (async () => {
    let n = 0;
    while (recording) {
      const t = Date.now();
      const file = join(frames, `f${String(n++).padStart(5, "0")}.png`);
      await page.screenshot({ path: file });
      shots.push({ file, t });
      const rest = 1000 / FPS - (Date.now() - t);
      if (rest > 0) await sleep(rest);
    }
  })();

  // 1. Drag the CV from the bottom-left corner onto the drop zone.
  await page.mouse.move(70, 610);
  await sleep(900);
  await page.evaluate(() => window.demoCursor.carry("demo_cv.txt"));
  const drop = await center(page.locator("button.drop"));
  await page.mouse.move(drop.x - 120, drop.y + 10, { steps: 30 });
  await page.evaluate(() => window.demo.dragEnter());
  await page.mouse.move(drop.x - 100, drop.y, { steps: 8 });
  await sleep(500);
  await page.evaluate(() => {
    window.demo.drop();
    window.demoCursor.carry(null);
  });
  await page.getByText("107 words").waitFor();
  await sleep(1200);

  // 2. Find jobs and show the details of the run.
  await click(page, page.getByRole("button", { name: "Find jobs" }));
  await sleep(500);
  await click(page, page.locator("summary"));
  await page.getByText("Found 6 jobs").waitFor({ timeout: 20000 });
  await sleep(1500);

  // 3. Open the best match.
  await click(page, page.locator("tbody tr").first());
  await sleep(3500);

  // 4. Filter the list.
  await click(page, page.getByRole("button", { name: "Close the details" }));
  await sleep(300);
  await click(page, page.getByPlaceholder("Filter"));
  await page.keyboard.type("nordlicht", { delay: 90 });
  await sleep(2500);

  recording = false;
  await capture;

  // Each frame is shown until the next one was taken, so the GIF has the real timing.
  const list = shots.map((s, i) => {
    const next = shots[i + 1]?.t ?? s.t + 2000;
    return `file '${s.file}'\nduration ${((next - s.t) / 1000).toFixed(3)}`;
  });
  list.push(`file '${shots.at(-1).file}'`);
  const listFile = join(frames, "frames.txt");
  writeFileSync(listFile, list.join("\n"));

  const filter =
    `fps=${FPS},scale=${GIF_WIDTH}:-1:flags=lanczos,split[a][b];` +
    `[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle`;
  execFileSync("ffmpeg", ["-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i", listFile, "-vf", filter, OUT]);
  console.log(`Wrote ${OUT} (${shots.length} frames)`);
} finally {
  await browser.close();
  await server.close();
  rmSync(frames, { recursive: true, force: true });
}

function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

async function center(locator) {
  const b = await locator.boundingBox();
  return { x: b.x + b.width / 2, y: b.y + b.height / 2 };
}

async function click(page, locator) {
  const c = await center(locator);
  await page.mouse.move(c.x, c.y, { steps: 20 });
  await sleep(250);
  await page.mouse.down();
  await sleep(80);
  await page.mouse.up();
}

// Screenshots do not show the mouse pointer, so draw one (and the file it carries).
async function addCursor(page) {
  await page.evaluate(() => {
    const el = document.createElement("div");
    el.style.cssText = "position:fixed;left:0;top:0;z-index:99999;pointer-events:none;";
    el.innerHTML =
      '<svg width="22" height="26" viewBox="0 0 22 26"><path d="M2 2 L2 20 L7 15.5 L10.5 23 L13.5 21.7 L10 14.3 L16.5 14 Z" fill="#111" stroke="#fff" stroke-width="1.6" stroke-linejoin="round"/></svg>' +
      '<div style="display:none;margin:-4px 0 0 16px;padding:6px 10px;border-radius:8px;background:#fff;border:1px solid #ccd;box-shadow:0 4px 14px rgba(0,0,0,.18);font:13px system-ui;white-space:nowrap;">📄 <span></span></div>';
    document.body.append(el);
    const chip = el.lastElementChild;
    addEventListener("mousemove", (e) => (el.style.transform = `translate(${e.clientX - 2}px, ${e.clientY - 2}px)`));
    addEventListener("mousedown", () => (el.firstElementChild.style.transform = "scale(0.85)"));
    addEventListener("mouseup", () => (el.firstElementChild.style.transform = ""));
    window.demoCursor = {
      carry(name) {
        chip.style.display = name ? "inline-block" : "none";
        chip.querySelector("span").textContent = name ?? "";
      },
    };
  });
}
