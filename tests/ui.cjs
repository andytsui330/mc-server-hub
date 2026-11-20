// Windows 真正的 Tauri WebView2 畫面測試；沿用已安裝 Playwright，不安裝額外工具。
// MCHUB_DATA_DIR 必須指到 TEMP 內全新的專用資料夾，所有測試殘留保留供查證。
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const { spawn, spawnSync } = require('node:child_process');
const { chromium } = require(process.env.MCHUB_PLAYWRIGHT || 'playwright');

async function fakeServer(data) {
  const dir = path.join(data, '假 伺服器');
  fs.mkdirSync(dir);
  const java = spawnSync('java', ['-XshowSettings:properties', '-version'], { windowsHide: true, encoding: 'utf8' });
  const javaHome = (java.stderr || '').match(/java\.home\s*=\s*(.+)/)?.[1].trim();
  assert(javaHome, '讀不到 JDK 位置');
  for (const [command, args] of [
    [path.join(javaHome, 'bin', 'javac.exe'), ['-encoding', 'UTF-8', '-d', dir, path.resolve(__dirname, '../legacy/tests/FakeServer.java')]],
    [path.join(javaHome, 'bin', 'jar.exe'), ['--create', '--file', path.join(dir, 'server.jar'), '--main-class', 'FakeServer', '-C', dir, 'FakeServer.class']],
  ]) {
    const result = spawnSync(command, args, { windowsHide: true, encoding: 'utf8' });
    assert.equal(result.status, 0, `${command} 失敗：${result.stderr || result.error}`);
  }
  const net = require('node:net');
  const probe = net.createServer();
  await new Promise(resolve => probe.listen(0, '127.0.0.1', resolve));
  const port = probe.address().port;
  await new Promise(resolve => probe.close(resolve));
  fs.writeFileSync(path.join(dir, 'start.bat'), '@echo off\r\njava -Xmx256M -jar server.jar nogui\r\npause\r\n');
  fs.writeFileSync(path.join(dir, 'server.properties'), `server-port=${port}\nlevel-seed=123456789\n`);
  fs.mkdirSync(path.join(dir, 'world'));
  fs.writeFileSync(path.join(dir, 'world', 'level.dat'), 'UI test world');
  return dir;
}

async function main() {
  const data = process.env.MCHUB_DATA_DIR;
  assert(data && path.isAbsolute(data), '請設定 MCHUB_DATA_DIR 為 TEMP 內的全新測試資料夾');
  const relative = path.relative(os.tmpdir(), data);
  assert(relative && !relative.startsWith('..') && !path.isAbsolute(relative), '測試資料夾必須在 TEMP 內');
  assert(!fs.existsSync(data), '測試資料夾已存在，請指定全新資料夾（測試不刪檔）');
  fs.mkdirSync(data, { recursive: true });
  const evidence = path.resolve(__dirname, '../target/ui-step5');
  fs.mkdirSync(evidence, { recursive: true });
  const crypto = require('node:crypto');
  const realDir = path.join(process.env.APPDATA, 'MCServerHub');
  const realFiles = ['hub_config.json', 'config.json', 'hub_config.1x-backup.json'];
  const fingerprint = () => Object.fromEntries(realFiles.map(name => {
    const file = path.join(realDir, name);
    return [name, fs.existsSync(file) ? crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex') : null];
  }));
  const before = fingerprint();
  const port = Number(process.env.MCHUB_CDP_PORT || 9229);
  const exe = path.resolve(__dirname, '../target/release/mc-server-hub.exe');
  const child = spawn(exe, [], { windowsHide: true, env: {
    ...process.env, MCHUB_DATA_DIR: data,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
    WEBVIEW2_USER_DATA_FOLDER: path.join(data, 'webview'),
  }, stdio: ['ignore', 'pipe', 'pipe'] });
  const appOutput = [];
  child.stdout.on('data', chunk => appOutput.push(chunk.toString()));
  child.stderr.on('data', chunk => appOutput.push(chunk.toString()));
  child.on('exit', (code, signal) => console.log('APP EXIT', code, signal));
  let browser;
  let page;
  const webErrors = [];
  try {
    for (let n = 0; n < 80; n++) {
      try { browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`); break; }
      catch { await new Promise(resolve => setTimeout(resolve, 250)); }
    }
    assert(browser, '連不到測試程式的 WebView2');
    page = browser.contexts()[0].pages()[0];
    page.setDefaultTimeout(8000);
    page.on('pageerror', error => console.error('PAGE ERROR', error.message));
    page.on('crash', () => console.error('WEBVIEW CRASH'));
    page.on('console', msg => { if (msg.type() === 'error') webErrors.push(msg.text()); });
    await page.waitForLoadState('domcontentloaded');
    await page.getByRole('button', { name: '建立新伺服器', exact: true }).first().waitFor({ timeout: 8000 });
    assert(await page.getByRole('button', { name: /加入現有/ }).count(), '歡迎畫面要有加入現有資料夾');
    console.log('PASS 歡迎畫面：建立新服、加入現有資料夾');
    // 後續操作使用這個真 Java 測試服，所有資料都在專用 TEMP 資料夾。
    const folder = await fakeServer(data);
    await page.getByRole('button', { name: /加入現有/ }).first().click();
    const add = page.getByRole('dialog', { name: '加入現有伺服器', exact: true });
    await add.getByLabel('伺服器資料夾', { exact: true }).fill(path.join(data, '不存在'));
    await add.getByRole('button', { name: '加入', exact: true }).click();
    await add.getByText('找不到這個資料夾，請重新選擇', { exact: true }).waitFor();
    await add.getByLabel('伺服器資料夾', { exact: true }).fill(folder);
    await add.getByLabel('顯示名稱（可不填）').fill('測試世界');
    await add.getByRole('button', { name: '加入', exact: true }).click();
    await add.waitFor({ state: 'hidden' });
    await page.getByRole('button', { name: '開機', exact: true }).first().waitFor();
    console.log('PASS 加入資料夾：錯誤回饋、中文空白路徑、選中伺服器');
    await page.getByRole('button', { name: '詳細資訊', exact: true }).click();
    const info = page.getByRole('dialog', { name: /測試世界 · 詳細資訊/ });
    await info.getByLabel('世界種子碼').waitFor();
    assert.equal(await info.getByLabel('世界種子碼').inputValue(), '123456789');
    await info.getByLabel('朋友連線網址').fill('play.example.com:25565');
    await info.getByRole('button', { name: '儲存', exact: true }).click();
    await info.getByLabel('記憶體', { exact: true }).selectOption('1');
    await info.getByRole('button', { name: '套用', exact: true }).click();
    await info.getByLabel('正常關機後自動備份世界').check();
    await info.getByRole('button', { name: '完成', exact: true }).click();
    await info.waitFor({ state: 'hidden' });
    assert(fs.existsSync(path.join(folder, 'start.bat.bak')), '改記憶體要留下 .bak');
    assert(fs.readFileSync(path.join(folder, 'start.bat'), 'utf8').includes('-Xmx1G'));
    console.log('PASS 詳細資訊：種子、地址、記憶體 .bak、自動備份');
    await page.locator('#h-icon').click();
    const icons = page.getByRole('dialog', { name: '選擇圖示與顏色', exact: true });
    assert.equal(await icons.locator('.ig button').count(), 93);
    assert.equal(await icons.locator('.sw button').count(), 8);
    await icons.locator('.ig button').nth(1).click();
    await icons.locator('.sw button').nth(1).click();
    await icons.getByRole('button', { name: '套用', exact: true }).click();
    await icons.waitFor({ state: 'hidden' });
    await page.locator('#h-icon').click();
    assert.equal(await icons.locator('.ig button').nth(1).getAttribute('aria-pressed'), 'true');
    assert.equal(await icons.locator('.sw button').nth(1).getAttribute('aria-pressed'), 'true');
    await page.keyboard.press('Escape');
    console.log('PASS 圖示：93 個圖示、8 色、儲存與重新讀取');
    await page.locator('#h-power').click();
    await page.getByRole('button', { name: '確認並開機', exact: true }).click();
    const eula = page.getByRole('dialog', { name: '開機前要同意授權合約', exact: true });
    await eula.waitFor();
    assert(await eula.getByRole('button', { name: '同意並開機', exact: true }).isDisabled());
    await eula.getByRole('button', { name: '取消', exact: true }).click();
    assert(!fs.existsSync(path.join(folder, 'eula.txt')), '取消不得代為同意 EULA');
    // 這是自行編譯的 FakeServer 測試資料，不啟動 Mojang 軟體或代使用者接受真實 EULA。
    fs.writeFileSync(path.join(folder, 'eula.txt'), 'eula=true\n');
    await page.getByRole('button', { name: '確認並開機', exact: true }).click();
    await page.locator('#logs').getByText(/Done \(2.0s\)/).waitFor({ timeout: 30000 });
    await page.locator('.who').getByText('Tester', { exact: true }).waitFor({ timeout: 10000 });
    assert(await page.locator('#h-backup').isDisabled(), '開機中不得備份');
    await page.locator('#cmd').fill('say UI-test');
    await page.locator('#cmd').press('Enter');
    await page.locator('#logs').getByText(/收到指令：say UI-test/).waitFor();
    await page.locator('#cmd').press('ArrowUp');
    assert.equal(await page.locator('#cmd').inputValue(), 'say UI-test');
    await page.locator('#cmd').fill('');
    await page.keyboard.press('Control+k');
    const palette = page.getByRole('dialog', { name: '搜尋指令', exact: true });
    await palette.getByLabel('搜尋指令', { exact: true }).fill('白天');
    await palette.getByLabel('搜尋指令', { exact: true }).press('Enter');
    await page.locator('#logs').getByText(/收到指令：time set day/).waitFor();
    await page.keyboard.press('Control+k');
    await palette.getByLabel('搜尋指令', { exact: true }).fill('給管理員');
    await palette.getByLabel('搜尋指令', { exact: true }).press('Enter');
    const strong = page.locator('form.form');
    await strong.getByLabel('玩家', { exact: true }).fill(' Tester ');
    await strong.getByLabel('打字確認', { exact: true }).fill('不對');
    assert(await strong.getByRole('button', { name: '執行', exact: true }).isDisabled());
    await strong.getByLabel('打字確認', { exact: true }).fill('Tester');
    await strong.getByRole('button', { name: '執行', exact: true }).click();
    await page.locator('#logs').getByText(/收到指令：op Tester/).waitFor();
    await page.keyboard.press('Control+k');
    await palette.getByLabel('搜尋指令', { exact: true }).fill('給物品');
    await palette.getByLabel('搜尋指令', { exact: true }).press('Enter');
    const allPlayers = page.locator('form.form');
    await allPlayers.getByLabel('玩家', { exact: true }).fill(' @a ');
    await allPlayers.getByRole('button', { name: '執行', exact: true }).click();
    await allPlayers.getByRole('button', { name: '確定執行', exact: true }).click();
    await page.locator('#logs').getByText(/收到指令：give @a /).waitFor();
    // 為捲動回歸填入足夠紀錄；操作自動捲動開關後讓狀態輪詢跑過兩輪。
    await page.evaluate(async () => {
      const call = (path, body = {}) => window.__TAURI_INTERNALS__.invoke('api', { path, body });
      const state = await call('/api/state');
      for (let n = 0; n < 40; n++) await call('/api/cmd', { id: state.servers[0].id, cmd: `say scroll-${n}` });
    });
    await page.locator('#logs').getByText(/收到指令：say scroll-39/).waitFor();
    await page.locator('#follow').click();
    await page.locator('.log').evaluate(el => { el.scrollTop = 0; });
    await new Promise(resolve => setTimeout(resolve, 2600));
    assert.equal(await page.locator('.log').evaluate(el => el.scrollTop), 0, '關閉自動捲動後不得跳到底部');
    await page.locator('#follow').click();
    console.log('PASS 開機與指令：EULA 取消、Done、玩家、手打/歷史、Ctrl K、打字強確認');
    await page.locator('#quit').click();
    // 2.0.1：第一次關閉先詢問；選「結束平台」仍要被執行中的伺服器擋下。
    await page.getByRole('dialog').getByText('結束平台', { exact: true }).click();
    await page.getByRole('dialog').getByRole('button', { name: '確定', exact: true }).click();
    await page.locator('#toast').getByText('還有伺服器在執行或正在處理資料，請先關機再關閉平台', { exact: true }).waitFor();
    assert(!page.isClosed(), '伺服器執行中視窗不可以關閉');
    await page.locator('#h-power').click();
    await page.getByRole('dialog').getByRole('button', { name: '關機', exact: true }).click();
    await page.locator('#h-power').filter({ hasText: '開機' }).waitFor({ timeout: 30000 });
    await page.getByText(/備份完成/).first().waitFor({ timeout: 15000 });
    assert(fs.readdirSync(path.join(folder, 'backups')).length > 0, '正常關機後要自動備份');
    console.log('PASS 關機：忙碌關窗阻擋、正常 stop、自動備份');
    await page.getByRole('tab', { name: '我的筆記', exact: true }).click();
    await page.getByRole('button', { name: /新增／編輯我的筆記/ }).click();
    const notes = page.getByRole('dialog', { name: '我的筆記', exact: true });
    await notes.getByLabel('名稱', { exact: true }).fill('測試筆記');
    await notes.getByLabel('指令（不用打 /）').fill('say note-test');
    await notes.getByRole('button', { name: '儲存這筆', exact: true }).click();
    await notes.locator('.nrow').getByText('測試筆記', { exact: true }).waitFor();
    await notes.getByRole('button', { name: '完成', exact: true }).click();
    await page.keyboard.press('Control+k');
    await palette.getByLabel('搜尋指令', { exact: true }).fill('測試筆記');
    await palette.getByRole('option').filter({ hasText: '測試筆記' }).waitFor();
    await page.keyboard.press('Escape');
    await page.locator('#links-btn').click();
    await page.getByLabel('啟動時檢查新版', { exact: true }).click();
    await page.waitForFunction(() => !document.querySelector('[aria-label="啟動時檢查新版"]').checked);
    await page.getByRole('menuitem', { name: '編輯連結…', exact: true }).click();
    const links = page.getByRole('dialog', { name: '編輯實用連結', exact: true });
    await links.getByRole('button', { name: '＋ 新增一個', exact: true }).click();
    await links.getByLabel('連結名稱', { exact: true }).last().fill('測試連結');
    await links.getByLabel('網址', { exact: true }).last().fill('https://example.com/docs');
    await links.getByRole('button', { name: '儲存', exact: true }).click();
    await links.waitFor({ state: 'hidden' });
    await page.locator('#h-info').click();
    await info.getByLabel('朋友連線網址').waitFor();
    assert.equal(await page.evaluate(() => document.activeElement?.id), 'in-addr', '資訊載入後焦點要移到地址');
    await info.getByRole('button', { name: '編輯說明範本', exact: true }).click();
    const join = page.getByRole('dialog', { name: '編輯加入說明範本', exact: true });
    await join.locator('textarea').fill('測試加入 <連線網址>');
    await join.getByRole('button', { name: '儲存', exact: true }).click();
    await join.waitFor({ state: 'hidden' });
    await info.getByText('測試加入 play.example.com:25565', { exact: true }).waitFor();
    await info.getByRole('button', { name: '完成', exact: true }).click();
    console.log('PASS 個人內容：筆記/搜尋、實用連結、新版開關、加入說明範本、載入後焦點');
    await page.locator('#add-new').click();
    const wizard = page.getByRole('dialog', { name: '建立新伺服器', exact: true });
    await wizard.locator('#w-ver option').first().waitFor({ state: 'attached', timeout: 45000 });
    assert.equal(await wizard.getByRole('radio').count(), 4, '四種載入器要找得到');
    assert(await wizard.locator('#w-go').isDisabled(), '未填完整、未勾選 EULA 不可建服');
    await page.screenshot({ path: path.join(evidence, 'wizard.png') });
    await wizard.getByRole('button', { name: '取消', exact: true }).click();
    console.log('PASS 建服精靈：官方版本清單、四種載入器、Java 檢查、EULA 阻擋');
    for (const [width, height] of [[1360, 900], [1366, 768], [1280, 650], [900, 600]]) {
      await page.setViewportSize({ width, height });
      await page.screenshot({ path: path.join(evidence, `server-${width}x${height}.png`) });
      assert(await page.locator('#h-power').isVisible());
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false, '不得橫向溢出');
      await page.locator('#nav-ov').click();
      await page.screenshot({ path: path.join(evidence, `overview-${width}x${height}.png`) });
      await page.locator('.ovmain').first().click();
    }
    await page.emulateMedia({ reducedMotion: 'reduce' });
    assert.equal(await page.evaluate(() => matchMedia('(prefers-reduced-motion: reduce)').matches), true);
    const sound = await page.evaluate(() => window.__sfxLog);
    assert(sound.some(event => event.kind === 'up'), '開機音效事件');
    assert(sound.some(event => event.kind === 'down'), '關機音效事件');
    console.log('PASS 視覺與動態：三種指定尺寸、900px 最小桌面、總覽、減少動態、開關機音效');
    await page.locator('#h-info').click();
    await info.getByRole('button', { name: '從清單移除', exact: true }).click();
    await page.getByRole('dialog').last().getByRole('button', { name: '移除', exact: true }).click();
    await page.locator('#welcome').waitFor();
    assert(fs.existsSync(path.join(folder, 'server.jar')), '移除只改清單，不刪伺服器檔案');
    const saved = JSON.parse(fs.readFileSync(path.join(data, 'config.json'), 'utf8'));
    assert.equal(saved.servers.length, 0);
    assert.equal(saved.update_check, false);
    assert(saved.custom_commands.some(note => note.name === '測試筆記'));
    assert(saved.links.some(link => link.name === '測試連結'));
    assert.equal(saved.join_template, '測試加入 <連線網址>');
    console.log('PASS 設定落盤、移除保留伺服器檔案、回到歡迎畫面');
    assert.deepEqual(webErrors, [], '畫面不應有 JavaScript／CSP 錯誤');
    assert(await page.evaluate(async () => {
      await document.fonts.ready;
      return [...document.fonts].every(font => font.status === 'loaded');
    }), '內嵌字型要確實載入');
    await page.screenshot({ path: path.join(evidence, 'server.png') });
    assert.deepEqual(fingerprint(), before, '實際 APPDATA 設定被改動');
  } catch (error) {
    if (page && !page.isClosed()) {
      await page.screenshot({ path: path.join(evidence, 'failure.png') }).catch(() => {});
      console.log('FAILURE UI', (await page.locator('[role="dialog"]').allTextContents()).map(text => text.slice(0, 400)));
    }
    throw error;
  } finally {
    if (appOutput.length) console.log('APP OUTPUT', appOutput.join(''));
    if (page && !page.isClosed()) {
      // 測試失敗也先正常收掉自己的假伺服器，避免留下 Java 程序。
      await page.evaluate(async () => {
        const call = (path, body = {}) => window.__TAURI_INTERNALS__.invoke('api', { path, body });
        const state = await call('/api/state');
        for (const server of state.servers) {
          if (server.state !== 'stopped' && !server.external) await call('/api/stop', { id: server.id });
        }
        for (let n = 0; n < 50; n++) {
          const state = await call('/api/state');
          if (state.servers.every(s => s.state === 'stopped' && !s.backup_busy)) break;
          await new Promise(resolve => setTimeout(resolve, 300));
        }
      }).catch(() => {});
      await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('api', { path: '/api/desktop/quit', body: { discard: true } })).catch(() => {});
    }
    if (browser) await browser.close();
    await new Promise(resolve => {
      if (child.exitCode !== null) return resolve();
      child.once('exit', resolve);
      setTimeout(() => { child.kill(); resolve(); }, 4000).unref();
    });
    assert.deepEqual(fingerprint(), before, '實際 APPDATA 設定被改動');
    console.log(`測試資料保留：${data}；測試程序 ${child.pid}`);
  }
}

main().catch(error => { console.error(error); process.exitCode = 1; });
