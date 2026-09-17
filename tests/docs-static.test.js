"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const docs = path.resolve(__dirname, "..", "docs");
const pages = fs.readdirSync(docs).filter(name => name.endsWith(".html"));

test("documentation ships the complete navigation surface", () => {
  for (const expected of ["index.html", "design.html", "testing.html", "systems.html", "ide.html"]) {
    assert.ok(pages.includes(expected), `${expected} is missing`);
  }
});

test("HTML pages have unique ids and valid local assets", () => {
  for (const page of pages) {
    const filename = path.join(docs, page);
    const html = fs.readFileSync(filename, "utf8");
    const ids = [...html.matchAll(/\bid=["']([^"']+)["']/g)].map(match => match[1]);
    assert.equal(new Set(ids).size, ids.length, `${page} contains a duplicate id`);

    for (const match of html.matchAll(/\s(?:href|src)=["']([^"']+)["']/g)) {
      const reference = match[1];
      if (/^(?:https?:|mailto:|data:|javascript:|#)/.test(reference)) continue;
      const localPath = decodeURIComponent(reference.split(/[?#]/, 1)[0]);
      assert.ok(fs.existsSync(path.resolve(docs, localPath)), `${page} references missing ${localPath}`);
    }
  }
});

test("Playground loads the Studio controller after the shared documentation script", () => {
  const html = fs.readFileSync(path.join(docs, "ide.html"), "utf8");
  const controller = fs.readFileSync(path.join(docs, "assets", "qu-studio.js"), "utf8");
  const shared = html.indexOf('src="assets/qu.js"');
  const studio = html.indexOf('src="assets/qu-studio.js"');
  assert.ok(shared >= 0 && studio > shared);
  assert.match(html, /id="studioEditor"/);
  assert.match(html, /id="studioPlot"/);
  assert.match(html, /id="studioDiagnostics"/);
  assert.match(html, /data-studio-file="selection"/);
  assert.match(html, /data-studio-file="math"/);
  assert.match(html, /id="runtimeCaps"/);
  assert.match(controller, /selection:\s*`/);
  assert.match(controller, /math:\s*`/);
  assert.match(controller, /idx1 := where x > 3/);
  assert.match(controller, /WebGPU adapter ready/);
  assert.match(controller, /editor\.scrollLeft = 0/);
});

test("3D documentation binds the frame and hull demo to its renderer", () => {
  const source = fs.readFileSync(path.join(docs, "assets", "qu.js"), "utf8");
  for (const page of ["live.html", "systems.html"]) {
    const html = fs.readFileSync(path.join(docs, page), "utf8");
    assert.match(html, /data-demo="frames3d"/);
    assert.match(html, /data-src="frames3d"/);
  }
  assert.match(source, /frames3d:\s*`/);
  assert.match(source, /frames3d\(ctx,w,h,t,p\)/);
  assert.match(source, /physics\.compile_hull/);
});
