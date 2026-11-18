// 1.2.0 真 HTTP 畫面與 2.0 真 WebView2，用同一個 FakeServer 跑完整指令選單。
// 不刪測試殘留；不改真設定。既有 UI 全流程另見 ui.cjs。
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const crypto = require('node:crypto');
const {spawn,spawnSync} = require('node:child_process');
const {chromium} = require(process.env.MCHUB_PLAYWRIGHT || 'playwright');
const repo = path.resolve(__dirname,'..');
const delay = ms => new Promise(r=>setTimeout(r,ms));
async function port() { const s=require('node:net').createServer(); await new Promise(r=>s.listen(0,'127.0.0.1',r)); const n=s.address().port; await new Promise(r=>s.close(r)); return n; }
function run(exe,args){const r=spawnSync(exe,args,{windowsHide:true,encoding:'utf8'});assert.equal(r.status,0,r.stderr||String(r.error));return r;}
function fingerprint(){return Object.fromEntries(['hub_config.json','config.json','hub_config.1x-backup.json'].map(n=>{const p=path.join(process.env.APPDATA,'MCServerHub',n);return [n,fs.existsSync(p)?crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex'):null];}));}
function assertOwnedFixture(state,dir){
  const servers=state?.servers;
  assert(Array.isArray(servers)&&servers.length===1&&servers[0].id==='parity'&&typeof servers[0].dir==='string'&&path.resolve(servers[0].dir).toLowerCase()===path.resolve(dir).toLowerCase(),'連到的不是本輪 TEMP 假伺服器；拒絕操作');
}
async function cdpPort(){
  if(!process.env.MCHUB_CDP_PORT)return port();
  const n=Number(process.env.MCHUB_CDP_PORT);assert(Number.isInteger(n)&&n>=1024&&n<=65535,'CDP 埠格式不對');
  const s=require('node:net').createServer();await new Promise((resolve,reject)=>{s.once('error',reject);s.listen(n,'127.0.0.1',resolve);});await new Promise(r=>s.close(r));return n;
}
async function call(page,side,route,body={}){
  return page.evaluate(async ({side,route,body})=>{
    if(side==='rust') return window.__TAURI_INTERNALS__.invoke('api',{path:route,body});
    const u=new URL(route,location.href),get=['/api/state','/api/log','/api/commands','/api/info','/api/settings'].includes(route)&&Object.keys(body).length===0;
    if(route==='/api/log'||route==='/api/info'||route==='/api/commands'){for(const[k,v]of Object.entries(body))u.searchParams.set(k,v);}
    const headers={'X-Hub-Token':document.documentElement.outerHTML.match(/const TOKEN = "([^"]+)"/)[1],'Content-Type':'application/json'};
    const read=get||['/api/log','/api/info','/api/commands'].includes(route);
    return (await fetch(u,{method:read?'GET':'POST',headers,...(read?{}:{body:JSON.stringify(body)})})).json();
  },{side,route,body});
}
async function main(){
  const root=process.env.MCHUB_DATA_DIR;
  assert(root&&path.isAbsolute(root));const rel=path.relative(os.tmpdir(),root);
  assert(rel&&!rel.startsWith('..')&&!path.isAbsolute(rel)&&!fs.existsSync(root),'需要 TEMP 下全新 MCHUB_DATA_DIR');
  fs.mkdirSync(root,{recursive:true});const before=fingerprint(),report={root,checks:[],runs:{},before};
  const handles=[];let edge,webview;const pages=[];
  const pass=name=>{report.checks.push(name);console.log('PASS',name);};
  try{
    const prop=run('java',['-XshowSettings:properties','-version']);const jdk=(prop.stderr||'').match(/java\.home\s*=\s*(.+)/)[1].trim();
    const classes=path.join(root,'classes');fs.mkdirSync(classes);
    run(path.join(jdk,'bin/javac.exe'),['-encoding','UTF-8','-d',classes,path.join(repo,'legacy/tests/FakeServer.java')]);
    const jar=path.join(root,'server.jar');run(path.join(jdk,'bin/jar.exe'),['cfe',jar,'FakeServer','-C',classes,'.']);
    const httpPort=await port(),cdp=await cdpPort();
    for(const side of ['python','rust']){
      const data=path.join(root,side),dir=path.join(data,'假 伺服器');fs.mkdirSync(dir,{recursive:true});fs.copyFileSync(jar,path.join(dir,'server.jar'));
      fs.writeFileSync(path.join(dir,'start.bat'),'@echo off\r\njava -Xmx256M -jar server.jar nogui\r\npause\r\n');
      fs.writeFileSync(path.join(dir,'server.properties'),`server-port=${await port()}\nlevel-seed=123456789\n`);
      // 本程式自行編譯的 FakeServer，不啟動 Mojang 軟體。
      fs.writeFileSync(path.join(dir,'eula.txt'),'eula=true\n');
      const cfg={servers:[{id:'parity',name:'UI 對照',dir,bat:'start.bat',version:'1.21.1',address:'play.example.com:25565'}],links:[{name:'對照外部連結',url:'https://example.com'}],update_check:false};
      fs.writeFileSync(path.join(data,side==='python'?'hub_config.json':'config.json'),JSON.stringify(cfg));
      const args=side==='python'?['-3',path.join(repo,'legacy/src/hub.py'),'--config',path.join(data,'hub_config.json'),'--port',String(httpPort),'--no-window']:[];
      const child=spawn(side==='python'?'py':path.join(repo,'target/release/mc-server-hub.exe'),args,{windowsHide:true,env:{...process.env,MCHUB_DATA_DIR:data,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${cdp}`,WEBVIEW2_USER_DATA_FOLDER:path.join(data,'webview')},stdio:['ignore','ignore',fs.openSync(path.join(data,'stderr.log'),'w')]});
      handles.push(child);
      child.on('exit',(code,signal)=>console.log(`${side} APP EXIT`,code,signal));
    }
    edge=await chromium.launch({channel:'msedge',headless:true});const lp=await edge.newPage();
    for(let n=0;n<80;n++){try{await lp.goto(`http://127.0.0.1:${httpPort}`);break;}catch{await delay(200);}}
    for(let n=0;n<80;n++){try{webview=await chromium.connectOverCDP(`http://127.0.0.1:${cdp}`);break;}catch{await delay(200);}}
    assert(webview,'WebView2 CDP 連線失敗');let candidates=[];
    for(let n=0;n<80;n++){candidates=webview.contexts()[0].pages().filter(p=>/^(http:\/\/tauri\.localhost|tauri:\/\/localhost)\//.test(p.url()));if(candidates.length)break;await delay(100);}
    assert.equal(candidates.length,1,'找不到唯一 Tauri 測試頁面');const rp=candidates[0];
    for(const {side,page}of [{side:'python',page:lp},{side:'rust',page:rp}]){
      const expectedDir=path.join(root,side,'假 伺服器');
      await page.waitForFunction(side=>side!=='rust'||!!window.__TAURI_INTERNALS__,side);
      assertOwnedFixture(await call(page,side,'/api/state'),expectedDir);
      pages.push({side,page,expectedDir});
    }
    let canonical;
    for(const {side,page}of pages){
      page.setDefaultTimeout(10000);await page.locator('#h-power').waitFor();
      await page.locator('#h-power').click();await page.locator('#logs').getByText(/Done \(2.0s\)/).waitFor({timeout:30000});
      await page.locator('.who').getByText('Tester',{exact:true}).waitFor();
      const data=await call(page,side,'/api/commands',{id:'parity'});assert.equal(data.commands.length,55);
      if(canonical)assert.deepEqual(data,canonical);else canonical=data;
      async function choose(c){
        await page.keyboard.press('Control+k');const pal=page.getByRole('dialog',{name:'搜尋指令',exact:true});
        await pal.locator('input').fill(c.id);await pal.getByRole('option').filter({has:page.locator('b').getByText(c.name,{exact:true})}).click();
      }
      async function exercise(c,all=false){
        report.current={side,command:c.id,all};
        const beforeLog=await call(page,side,'/api/log',{id:'parity'});
        const sentLines=async()=>{const v=await call(page,side,'/api/log',{id:'parity',since:beforeLog.seq});return v.lines.map(x=>x[1]).filter(t=>t.startsWith('> ')||t.includes('清敵對生物：暫時切到和平'));};
        await choose(c);
        if(c.fields.length||c.danger){
          const form=page.locator('form.form');await form.waitFor();
          for(const k of c.fields){const f=data.fields[k];const el=side==='python'?form.locator(`[data-k="${k}"]`):f.kind==='bool'?form.getByRole('radiogroup',{name:f.label,exact:true}):form.locator(`#field-${k}`);
            assert.equal(await el.count(),1,`${side} ${c.id} 欄位 ${k}`);
            if(f.kind==='bool')continue;
            if(f.kind==='id'||f.kind==='ench')await el.selectOption(f.picks[0][0]);
            else await el.fill(f.kind==='player'?(all&&k==='player'?' @a ':' Tester '):f.kind==='int'?String(f.default??f.min):f.kind==='num'?'0':f.optional?'':'UI-parity');
          }
          if(c.danger==='strong'){
            const typed=form.getByLabel('打字確認',{exact:true});await typed.fill('錯誤');assert(await form.getByRole('button',{name:'執行',exact:true}).isDisabled());
            await typed.fill(c.fields.includes('player')?'Tester':'確定');
          }
          await form.getByRole('button',{name:'執行',exact:true}).click();
          if(c.danger==='confirm'||all){assert.deepEqual(await sentLines(),[],'第一次確認不得送出');await form.getByRole('button',{name:'確定執行',exact:true}).click();}
        }
        let observed=[];for(let n=0;n<50;n++){observed=await sentLines();if(observed.length)break;await delay(100);}
        assert.equal(observed.length,1,`${side} ${c.id} 應只送出一次：${JSON.stringify(observed)}`);
        if(c.special)await delay(2400);
        return {command:c.id,all,sent:observed[0]};
      }
      const out=[];for(const c of data.commands)out.push(await exercise(c));
      const give=data.commands.find(c=>c.id==='give');out.push(await exercise(give,true));
      report.runs[side]=out;pass(`${side}：55 指令逐條欄位/一般確認/強確認/Ctrl K＋空白 @a 升級確認`);
      const logs=await call(page,side,'/api/log',{id:'parity'});const text=logs.lines.map(x=>x[1]).join('\n');
      assert(text.includes('set to Peaceful')&&text.includes('set to Normal'),'清敵對生物應恢復原難度');
      await page.locator('#h-power').click();await page.getByRole('dialog').getByRole('button',{name:'關機',exact:true}).click();
      await page.waitForFunction(()=>document.querySelector('#h-power')?.textContent.includes('開機'),{timeout:30000});
      pass(`${side}：主控台、清敵對切和平再切回、正常關機`);
      const sound=await page.evaluate(()=>({up:window.__sfxLog?.some(e=>e.kind==='up'),down:window.__sfxLog?.some(e=>e.kind==='down')}));
      assert.deepEqual(sound,{up:true,down:true});
      await page.locator('#h-icon').click();const icons=page.getByRole('dialog',{name:'選擇圖示與顏色',exact:true});
      assert.equal(await icons.locator('.ig button').count(),93);assert.equal(await icons.locator('.sw button').count(),8);
      await icons.locator('.ig button').nth(1).click();await icons.locator('.sw button').nth(1).click();await icons.getByRole('button',{name:'套用',exact:true}).click();
      await page.getByRole('tab',{name:'我的筆記',exact:true}).click();await page.getByRole('button',{name:/新增／編輯我的筆記/}).click();
      const notes=page.getByRole('dialog',{name:/我的筆記/});await notes.getByLabel('名稱',{exact:true}).fill('對等筆記');await notes.getByLabel('指令（不用打 /）').fill('say parity-note');
      await notes.getByRole('button',{name:'儲存這筆',exact:true}).click();await notes.locator('.nrow').getByText('對等筆記',{exact:true}).waitFor();await notes.getByRole('button',{name:'完成',exact:true}).click();
      await page.keyboard.press('Control+k');const noteSearch=page.getByRole('dialog',{name:'搜尋指令',exact:true});await noteSearch.locator('input').fill('對等筆記');await noteSearch.getByRole('option').filter({hasText:'對等筆記'}).waitFor();await page.keyboard.press('Escape');
      await page.locator('#links-btn').click();const check=page.getByLabel('啟動時檢查新版',{exact:true});await check.click();await check.click();
      await page.getByRole('menuitem',{name:'編輯連結…',exact:true}).click();const links=page.getByRole('dialog',{name:'編輯實用連結',exact:true});await links.getByLabel('網址',{exact:true}).first().fill('https://example.com/');await links.getByRole('button',{name:'儲存',exact:true}).click();
      await page.locator('#h-info').click();const detail=page.getByRole('dialog',{name:/UI 對照 · 詳細資訊/});await detail.getByLabel('朋友連線網址',{exact:true}).waitFor();
      await detail.getByLabel(/^(記憶體上限|記憶體)$/).selectOption('1');await detail.getByRole('button',{name:'套用',exact:true}).click();
      await detail.getByRole('button',{name:/編輯.*範本/}).click();const join=page.getByRole('dialog',{name:'編輯加入說明範本',exact:true});await join.locator('textarea').fill('對等加入 <連線網址>');await join.getByRole('button',{name:'儲存',exact:true}).click();await join.waitFor({state:'hidden'});await detail.getByRole('button',{name:'完成',exact:true}).click();
      const settings=await call(page,side,'/api/settings');
      assert.equal(settings.update_check,false);assert.equal(settings.join_template,'對等加入 <連線網址>');
      assert(settings.custom_commands.some(c=>c.name==='對等筆記'&&c.tpl==='say parity-note'));
      const folder=path.join(root,side,'假 伺服器');assert(fs.readFileSync(path.join(folder,'start.bat.bak'),'utf8').includes('-Xmx256M'));assert(fs.readFileSync(path.join(folder,'start.bat'),'utf8').includes('-Xmx1G'));
      report[`${side}_settings`]={...settings,custom_commands:settings.custom_commands.map(({id,...c})=>c)};
      pass(`${side}：93圖示/8色、音效事件、筆記/搜尋、連結/新版開關、加入範本、記憶體.bak`);
      if(process.env.MCHUB_TEST_CLIPBOARD==='1'){
        // 只在使用者當次同意覆寫剪貼簿後設定；不讀取或保存原有私人內容。
        await page.context().grantPermissions(['clipboard-read','clipboard-write']);
        await page.evaluate(()=>navigator.clipboard.writeText('MC-HUB disposable clipboard test'));
        await page.locator('#h-copy').click();
        assert((await page.evaluate(()=>navigator.clipboard.readText()))==='play.example.com:25565','地址複製結果不符（不記錄剪貼簿內容）');
        await page.locator('#h-info').click();const info=page.getByRole('dialog',{name:/UI 對照 · 詳細資訊/});
        const expected=await call(page,side,'/api/info',{id:'parity'});
        await info.locator('.sec').filter({has:page.getByRole('heading',{name:/種子碼/})}).getByRole('button',{name:'複製',exact:true}).click();
        assert((await page.evaluate(()=>navigator.clipboard.readText()))==='123456789','種子複製結果不符（不記錄剪貼簿內容）');
        await info.getByRole('button',{name:/複製.*說明/}).click();
        assert((await page.evaluate(()=>navigator.clipboard.readText())).replace(/\r\n/g,'\n')===expected.join_text,'加入說明複製結果不符（不記錄剪貼簿內容）');
        await info.getByRole('button',{name:'完成',exact:true}).click();
        pass(`${side}：地址/種子/加入說明三種真剪貼簿讀回（當次同意覆寫）`);
      }else{
        (report.skipped??=[]).push(`${side} 剪貼簿：需要使用者當次同意覆寫，再設 MCHUB_TEST_CLIPBOARD=1`);
      }
      await page.locator('#links-btn').click();
      if(side==='python'){
        const opened=page.waitForEvent('popup');await page.getByRole('menuitem',{name:'對照外部連結',exact:true}).click();
        const popup=await opened;await popup.waitForLoadState('domcontentloaded');assert(popup.url().startsWith('https://example.com'));await popup.close();
        pass('python：實用連結實際開啟 HTTPS 頁面');
      }else{
        await page.getByRole('menuitem',{name:'對照外部連結',exact:true}).click();
        pass('rust：按下實用連結，瀏覽器開啟結果另由桌面讀回');
      }
      await page.screenshot({path:path.join(root,`${side}-commands.png`)});
    }
    assert.deepEqual(report.runs.python,report.runs.rust);pass('兩版 56 次實際 UI 操作送出的遊戲指令完全相同');
    assert.deepEqual(report.python_settings,report.rust_settings);pass('兩版畫面編輯後的筆記/連結/範本/新版開關一致（隨機筆記 ID 除外）');
    report.status='passed';
  }catch(e){report.status='failed';report.failure=e.stack;
    for(const {side,page}of pages)if(!page.isClosed()){
      await page.screenshot({path:path.join(root,`${side}-failure.png`)}).catch(()=>{});
      report[`${side}_failure`]=await page.evaluate(()=>({runs:window.__parityRuns,toast:document.querySelector('#toast')?.textContent,form:document.querySelector('form.form')?.textContent,inputs:[...document.querySelectorAll('form.form input')].map(el=>({value:el.value,valid:el.validity.valid,message:el.validationMessage}))})).catch(()=>null);
    }throw e;}
  finally{
    for(const {side,page,expectedDir}of pages){if(page.isClosed())continue;try{const state=await call(page,side,'/api/state');assertOwnedFixture(state,expectedDir);for(const s of state.servers)if(s.state==='running'||s.state==='starting')await call(page,side,'/api/stop',{id:s.id});for(let i=0;i<100;i++){const st=await call(page,side,'/api/state');assertOwnedFixture(st,expectedDir);if(st.servers.every(s=>s.state==='stopped'&&!s.backup_busy))break;await delay(200);}if(side==='rust')await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('close_window'));else await call(page,side,'/api/quit');}catch(e){report.cleanup_warning=String(e);}}
    if(webview)await webview.close();if(edge)await edge.close();
    for(const child of handles){if(child.exitCode===null){await Promise.race([new Promise(r=>child.once('exit',r)),delay(3000)]);if(child.exitCode===null)child.kill();}}
    report.after=fingerprint();report.appdata_unchanged=JSON.stringify(before)===JSON.stringify(report.after);
    fs.writeFileSync(path.join(root,'ui-report.json'),JSON.stringify(report,null,2));console.log('測試資料保留：',root);
    assert.deepEqual(report.after,before,'真 APPDATA 設定遭變更');
  }
}
if(process.argv.includes('--self-test')){
  const expected=path.join(os.tmpdir(),'mchub-ownership','fake');
  assertOwnedFixture({servers:[{id:'parity',dir:expected}]},expected);
  for(const state of [{servers:[]},{servers:[{id:'real',dir:expected}]},{servers:[{id:'parity',dir:'D:/real-server'}]},{servers:[{id:'parity',dir:expected},{id:'real',dir:'D:/real-server'}]}])assert.throws(()=>assertOwnedFixture(state,expected),/拒絕操作/);
  console.log('PASS 隔離自測：正確假服可通過；空清單/真服 ID/真路徑/額外伺服器全拒絕');
}else main().catch(e=>{console.error(e);process.exitCode=1;});
