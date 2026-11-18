// 真 WebView2，第7步設定/系統匣/單一實例驗收；不安裝、不刪檔、不碰剪貼簿。
const fs=require('node:fs'),path=require('node:path'),os=require('node:os'),crypto=require('node:crypto'),assert=require('node:assert/strict');
const {spawn,spawnSync}=require('node:child_process');
const {chromium}=require(process.env.MCHUB_PLAYWRIGHT||'playwright');
const root=path.resolve(__dirname,'..'),data=fs.mkdtempSync(path.join(os.tmpdir(),'mchub-maturity-ui-'));
const evidence=path.join(root,'target/step7-ui');fs.mkdirSync(evidence,{recursive:true});
const report={data,checks:[]};
const delay=ms=>new Promise(r=>setTimeout(r,ms));
async function port(){const s=require('node:net').createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const n=s.address().port;await new Promise(r=>s.close(r));return n;}
function real(){return Object.fromEntries(['hub_config.json','config.json','hub_config.1x-backup.json'].map(n=>{const p=path.join(process.env.APPDATA,'MCServerHub',n);return[n,fs.existsSync(p)?crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex'):null]}))}
function run(exe,args){const r=spawnSync(exe,args,{windowsHide:true,encoding:'utf8'});assert.equal(r.status,0,r.stderr||String(r.error));return r;}
const invoke=(p,route,body={})=>p.evaluate(({route,body})=>window.__TAURI_INTERNALS__.invoke('api',{path:route,body}),{route,body});
let child,browser,page,exited;
const exe=path.join(root,'target/release/mc-server-hub.exe');
function owned(s){assert.equal(s.servers.length,1);assert.equal(s.servers[0].id,'step7');assert.equal(path.resolve(s.servers[0].dir).toLowerCase(),path.resolve(data,'假服').toLowerCase());}
async function launch(){
 const cdp=await port();
 child=spawn(exe,[],{windowsHide:true,env:{...process.env,MCHUB_DATA_DIR:data,WEBVIEW2_USER_DATA_FOLDER:path.join(data,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${cdp}`},stdio:'ignore'});
 exited=new Promise(r=>child.once('exit',r));
 for(let i=0;i<100;i++){try{browser=await chromium.connectOverCDP(`http://127.0.0.1:${cdp}`);break}catch{if(child.exitCode!==null)throw Error('APP 提早結束 '+child.exitCode);await delay(150)}}
 assert(browser,'CDP did not start');page=browser.contexts()[0].pages()[0];page.setDefaultTimeout(7000);await page.waitForFunction(()=>typeof window.__TAURI_INTERNALS__?.invoke==='function');await page.locator('#nav-settings').waitFor();owned(await invoke(page,'/api/state'));
 page.on('pageerror',e=>{report.errors??=[];report.errors.push(e.message)});
}
async function quit(){owned(await invoke(page,'/api/state'));await invoke(page,'/api/desktop/quit').catch(()=>{});await Promise.race([exited,delay(5000).then(()=>{throw Error('APP未正常退出')})]);await browser.close();browser=null;}
async function main(){
 report.before=real();
 const dir=path.join(data,'假服');fs.mkdirSync(dir);
 const j=run('java',['-XshowSettings:properties','-version']);const jdk=j.stderr.match(/java\.home\s*=\s*(.+)/)[1].trim();
 run(path.join(jdk,'bin/javac.exe'),['-encoding','UTF-8','-d',dir,path.join(root,'legacy/tests/FakeServer.java')]);
 run(path.join(jdk,'bin/jar.exe'),['cfe',path.join(dir,'server.jar'),'FakeServer','-C',dir,'FakeServer.class']);
 fs.writeFileSync(path.join(dir,'start.bat'),'@echo off\r\njava -Xmx256M -jar server.jar nogui\r\npause\r\n');
 fs.writeFileSync(path.join(dir,'server.properties'),`server-port=${await port()}\n`);fs.writeFileSync(path.join(dir,'eula.txt'),'eula=true\n');
 fs.writeFileSync(path.join(data,'config.json'),JSON.stringify({servers:[{id:'step7',name:'成熟配備假服',dir,bat:'start.bat',version:'1.21.1'}],update_check:false,unknown:{keep:true}}));
 const pass=s=>{report.checks.push(s);console.log('PASS',s)};
 try{
   await launch();assert.deepEqual(await invoke(page,'/api/desktop/status'),{visible:true,tray:true,isolated:true,launch_at_login:false});
   await page.locator('#nav-settings').click();await page.getByRole('heading',{name:'設定',exact:true}).waitFor();
   await page.getByLabel('登入電腦時啟動平台').check();await page.getByLabel('關閉視窗時').selectOption('tray');
   await page.getByRole('button',{name:'儲存設定',exact:true}).click();await page.getByText('所有變更已儲存',{exact:true}).waitFor();
   assert.equal((await invoke(page,'/api/desktop/status')).launch_at_login,true);pass('登入啟動設定透過隔離替身保存；真系統匣已建立');
   await page.getByRole('button',{name:'外觀',exact:true}).click();await page.getByLabel('主題').selectOption('light');
   await page.locator('#links-btn').click();await page.getByLabel('啟動時檢查新版',{exact:true}).click();
   await page.waitForFunction(async()=>{const s=await window.__TAURI_INTERNALS__.invoke('api',{path:'/api/settings',body:{}});return s.update_check===true});
   await page.keyboard.press('Escape');await delay(100);assert.equal(await page.getByLabel('主題').inputValue(),'light','無關設定不可清掉未儲存草稿');pass('實用連結設定變更保留未儲存的主題草稿');
   await page.getByLabel('字級').selectOption('16');await page.getByLabel('播放音效').uncheck();
   await page.getByRole('button',{name:'儲存設定',exact:true}).click();await page.waitForFunction(()=>document.documentElement.dataset.theme==='light');
   await page.evaluate(()=>window.__sfxLog=[]);await page.getByRole('button',{name:'一般',exact:true}).click();await delay(100);assert.equal(await page.evaluate(()=>window.__sfxLog.length),0);pass('淺色/字級實際套用，關音效後不播放');
   await page.screenshot({path:path.join(evidence,'light-1360.png')});
   await page.locator('.srv').filter({hasText:'成熟配備假服'}).click();await page.screenshot({path:path.join(evidence,'light-server-1360.png')});await page.locator('#nav-settings').click();
   await page.getByRole('button',{name:'外觀',exact:true}).click();await page.getByLabel('主題').selectOption('dark');await page.getByLabel('字級').selectOption('14');await page.getByRole('button',{name:'儲存設定',exact:true}).click();await page.waitForFunction(()=>document.documentElement.dataset.theme==='dark');
   await page.getByRole('button',{name:'伺服器預設值',exact:true}).click();await page.getByLabel('新伺服器記憶體（GB）').fill('6');await page.getByLabel('新伺服器存放位置').fill(data);await page.getByLabel('建立新服使用的 Java').fill(path.join(jdk,'bin/java.exe'));await page.getByLabel('關機後等待幾秒').fill('180');await page.getByLabel('新加入的伺服器').check();await page.getByRole('button',{name:'儲存設定',exact:true}).click();await page.getByText('所有變更已儲存',{exact:true}).waitFor();
   const pref=(await invoke(page,'/api/settings')).preferences;assert.equal(pref.stop_wait,180);assert.equal(pref.default_ram,6);assert.equal(pref.java_path,path.join(jdk,'bin/java.exe'));pass('記憶體/位置/Java/關機等待/自動備份預設值已存檔');
   for(const tab of ['一般','更新','外觀','伺服器預設值','備份','進階','關於']){await page.getByRole('button',{name:tab,exact:true}).click();await page.locator('.preferences-content h3').getByText(tab,{exact:true}).waitFor();}
   await page.getByRole('button',{name:'更新',exact:true}).click();await page.getByRole('button',{name:'檢查更新',exact:true}).click();await page.getByText('測試模式：不連線到正式更新服務',{exact:true}).waitFor();pass('七類設定可到達；手動檢查遵守測試隔離');
   await page.getByRole('button',{name:'一般',exact:true}).click();
   for(const [width,height]of[[1360,900],[1366,768],[1280,650],[900,600]]){await page.setViewportSize({width,height});await page.screenshot({path:path.join(evidence,`general-${width}.png`)});assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);}
   await page.setViewportSize({width:1360,height:900});pass('四種桌面尺寸無整頁水平溢出，已存截圖');
   await page.getByRole('button',{name:'進階',exact:true}).click();await page.getByRole('button',{name:'匯出診斷資訊',exact:true}).click();await page.getByText(/診斷檔已存到/).waitFor();
   const diagnostic=fs.readdirSync(path.join(data,'reports')).find(n=>n.startsWith('diagnostic-'));assert(diagnostic);const diag=fs.readFileSync(path.join(data,'reports',diagnostic),'utf8');assert(!diag.includes(data)&&!diag.includes('成熟配備假服'));pass('診斷匯出不含個人路徑/伺服器名');
   await invoke(page,'/api/settings',{preferences:{auto_start:['step7'],start_hidden:true}});
   await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('close_window'));
   assert.equal((await invoke(page,'/api/desktop/status')).visible,false);
   const second=spawn(exe,[],{windowsHide:true,env:{...process.env,MCHUB_DATA_DIR:data},stdio:'ignore'});const secondExit=await new Promise(r=>second.on('exit',r));assert.equal(secondExit,0);await delay(250);assert.equal((await invoke(page,'/api/desktop/status')).visible,true);pass('關窗縮到真系統匣；第二實例退出並喚回原視窗');
   await quit();await launch();assert.equal((await invoke(page,'/api/desktop/status')).visible,false);
   assert.equal((await invoke(page,'/api/settings')).preferences.default_ram,6);
   for(let i=0;i<80;i++){if((await invoke(page,'/api/state')).servers[0].state==='running')break;await delay(200)}
   assert.equal((await invoke(page,'/api/state')).servers[0].state,'running');pass('重開設定仍在、啟動縮小、自動啟動真 Java 假服至 Done');
   assert.match((await invoke(page,'/api/updates/install',{confirm:true})).error,/關閉所有伺服器/);pass('假服執行中實際阻擋更新');
   assert.equal((await invoke(page,'/api/stop',{id:'step7'})).ok,true);
   for(let i=0;i<80;i++){if((await invoke(page,'/api/state')).servers[0].state==='stopped')break;await delay(150)}
   assert.equal((await invoke(page,'/api/state')).servers[0].state,'stopped');
   assert.match((await invoke(page,'/api/updates/install',{confirm:true})).error,/先檢查更新/);assert.equal((await invoke(page,'/api/settings',{preferences:{volume:30}})).preferences.volume,30);pass('更新前置失敗後維護鎖釋放，仍可變更設定');
   const resetSource=fs.readFileSync(path.join(data,'config.json'));
   const reset=await invoke(page,'/api/reset-settings',{confirm:true});assert.equal(reset.preferences.theme,'dark');assert.deepEqual(fs.readFileSync(reset.backup),resetSource);assert.deepEqual(JSON.parse(fs.readFileSync(path.join(data,'config.json'))).unknown,{keep:true});pass('重設先逐byte備份，保留伺服器/未知欄位');
   await quit();assert.deepEqual(real(),report.before);report.after=real();assert(!report.errors?.length,JSON.stringify(report.errors));pass('真 APPDATA 三檔雜湊不變；無畫面錯誤；剪貼簿未操作');
 }finally{
   if(browser&&page){try{owned(await invoke(page,'/api/state'));const s=(await invoke(page,'/api/state')).servers[0];if(['running','starting'].includes(s.state)){await invoke(page,'/api/stop',{id:'step7'});for(let i=0;i<80&&(await invoke(page,'/api/state')).servers[0].state!=='stopped';i++)await delay(200)}await quit()}catch{console.error('保留本輪程式，未強制結束任何程序')}}
   if(browser)await browser.close().catch(()=>{});
   fs.writeFileSync(path.join(evidence,'report.json'),JSON.stringify(report,null,2));
 }
}
main().catch(e=>{console.error(e);process.exitCode=1});
