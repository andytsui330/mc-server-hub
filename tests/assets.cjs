// 舊畫面是遷移對照組：圖示逐像素、色票與外觀原文都要保留。
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { pathToFileURL } = require('node:url');
const root = path.resolve(__dirname, '..');
const read = name => fs.readFileSync(path.join(root, name), 'utf8').replace(/\r\n/g, '\n');

async function main() {
  const html = read('legacy/src/index.html');
  const begin = html.indexOf('const PX = (() => {');
  assert(begin >= 0, '找不到舊版像素圖示來源');
  const end = html.indexOf('})();', begin) + 5;
  const old = vm.runInNewContext(html.slice(begin, end) + '; PX', {}, { timeout: 5000 });
  const { pixelArt } = await import(pathToFileURL(path.join(root, 'app/src/pixels.ts')));
  const want = { icons: old.ICONS.map(i => ({ id: i.id, name: i.n, cat: i.cat, pixels: i.g() })), colors: old.COLORS };
  assert.deepEqual(JSON.parse(JSON.stringify(pixelArt)), JSON.parse(JSON.stringify(want)));
  assert.equal(pixelArt.icons.length, 93);
  assert.equal(pixelArt.colors.length, 8);
  assert.equal(read('app/src/fonts.css'), read('legacy/src/fonts.css'));
  const css = [...html.matchAll(/<style>([\s\S]*?)<\/style>/g)].map(match => match[1]).find(text => text.includes(':root{'));
  assert(css, '找不到舊版 CSS');
  assert(read('app/src/style.css').trim() === css.trim(), 'CSS 與舊版不同');
  console.log('PASS 93 icons pixel-for-pixel, 8 colors, original fonts and CSS');
}

main().catch(error => { console.error(error); process.exitCode = 1; });
