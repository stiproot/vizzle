// Open a generated component page headless, expand one component by name,
// print what its drill-down drew (as JSON facts a test can assert on) and
// screenshot the opened box. This is how a change to the page is looked at
// (CLAUDE.md § Verifying): layout and colour are invisible to unit tests.
//
//   cd web && bun install
//   CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
//     bun inspect-component.mjs ../out/diagram.html persistence ../out/persistence.png
import puppeteer from "puppeteer-core";
const [,, file, wanted, out] = process.argv;
if (!file || !wanted) {
  console.error("usage: bun inspect-component.mjs <page.html> <component name> [screenshot.png]");
  process.exit(2);
}
const executablePath = process.env.CHROME || process.env.PUPPETEER_EXECUTABLE_PATH;
if (!executablePath) {
  console.error("set CHROME (or PUPPETEER_EXECUTABLE_PATH) to a Chrome/Chromium binary");
  process.exit(2);
}
const browser = await puppeteer.launch({ executablePath, headless: true });
const page = await browser.newPage();
page.on("pageerror", (e) => console.log("PAGE ERROR:", e.message));
page.on("console", (m) => { if (m.type() === "error") console.log("CONSOLE ERROR:", m.text()); });
await page.setViewport({ width: 1600, height: 1000, deviceScaleFactor: 2 });
await page.goto("file://" + (file.startsWith("/") ? file : process.cwd() + "/" + file), { waitUntil: "load" });
await new Promise(r => setTimeout(r, 1200));
const facts = await page.evaluate((wanted) => {
  const nodeGs = [...document.querySelectorAll("svg g")].filter(g => g.querySelector(":scope > rect.comp-box"));
  const hit = nodeGs.find(g => { const t = g.querySelector(":scope > text.comp-centered:nth-of-type(2)") || [...g.querySelectorAll(":scope > text.comp-centered")][1]; return t && t.textContent.replace(/\s*[✚✖✱]$/, "") === wanted; });
  if (!hit) return { found: false, names: nodeGs.map(g => [...g.querySelectorAll(":scope > text.comp-centered")].map(t => t.textContent).join("|")).slice(0, 40) };
  const sub = [...hit.querySelectorAll(":scope > text.comp-centered")].map(t => t.textContent);
  hit.querySelector("g.comp-toggle").dispatchEvent(new MouseEvent("click", { bubbles: true }));
  const ex = hit.querySelector("g.comp-explosion");
  const boxes = [...ex.querySelectorAll("g > g > g")].filter(g => g.querySelector(":scope > rect"));
  const texts = [...ex.querySelectorAll("text")].map(t => t.textContent);
  const r = hit.getBoundingClientRect();
  return {
    found: true, header: sub, boxes: boxes.length,
    classNames: boxes.map(b => b.querySelector("text[font-weight='700']")?.textContent),
    foldRows: texts.filter(t => /unchanged member/.test(t)),
    control: texts.filter(t => /^show /.test(t)),
    notes: [...ex.querySelectorAll("text.comp-note")].map(t => t.textContent),
    bands: ex.querySelectorAll("rect[opacity='0.9']").length,
    box: { x: r.x, y: r.y, w: r.width, h: r.height },
  };
}, wanted);
console.log(JSON.stringify(facts, null, 1));
if (facts.found && out) {
  await new Promise(r => setTimeout(r, 600));
  const b = await page.evaluate((wanted) => {
    const nodeGs = [...document.querySelectorAll("svg g")].filter(g => g.querySelector(":scope > rect.comp-box"));
    const hit = nodeGs.find(g => { const t = [...g.querySelectorAll(":scope > text.comp-centered")][1]; return t && t.textContent.replace(/\s*[✚✖✱]$/, "") === wanted; });
    const r = hit.getBoundingClientRect(); return { x: r.x, y: r.y, w: r.width, h: r.height };
  }, wanted);
  const clip = { x: Math.max(0, b.x - 8), y: Math.max(0, b.y - 8), width: Math.min(1590 - Math.max(0, b.x - 8), b.w + 16), height: Math.min(990 - Math.max(0, b.y - 8), b.h + 16) };
  await page.screenshot({ path: out, clip });
  console.log("screenshot", out, JSON.stringify(clip));
}
await browser.close();
