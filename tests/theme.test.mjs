import test from "node:test";
import assert from "node:assert/strict";
import { THEMES, getTerminalTheme, withOpacity } from "../src/lib/theme.ts";

test("every listed theme has a complete terminal palette", () => {
  assert.deepEqual([...THEMES].sort(), ["dark", "frappe", "latte", "light", "macchiato", "mocha", "nord"]);
  for (const theme of THEMES) {
    const palette = getTerminalTheme(theme);
    for (const [name, value] of Object.entries(palette)) {
      assert.match(value, /^#[0-9a-f]{6}$/i, `${theme}.${name}`);
    }
  }
});

test("catppuccin flavors use their official base and text colors", () => {
  assert.equal(getTerminalTheme("mocha").background, "#1e1e2e");
  assert.equal(getTerminalTheme("mocha").foreground, "#cdd6f4");
  assert.equal(getTerminalTheme("latte").background, "#eff1f5");
  assert.equal(getTerminalTheme("frappe").background, "#303446");
  assert.equal(getTerminalTheme("macchiato").background, "#24273a");
});

test("withOpacity builds rgba and keeps opaque colors untouched", () => {
  assert.equal(withOpacity("#1e1e2e", 100), "#1e1e2e");
  assert.equal(withOpacity("#1e1e2e", 50), "rgba(30, 30, 46, 0.5)");
  assert.equal(withOpacity("not-a-color", 50), "not-a-color");
  assert.equal(getTerminalTheme("mocha", 80).background, "rgba(30, 30, 46, 0.8)");
  assert.equal(getTerminalTheme("mocha", 80).foreground, "#cdd6f4");
});
