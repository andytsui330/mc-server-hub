// 2.0.1 關窗詢問：真 WebView2＋真 Tauri 關窗事件，三種情境（不記住／記住縮匣／記住結束）。TEMP 隔離；不安裝、不刪檔、不碰剪貼簿。
const fs=require('node:fs'),path=require('node:path'),os=require('node:os'),crypto=require('node:crypto'),assert=require('node:assert/strict');
const {spawn}=require('node:child_process');
const {chromium}=require(process.env.MCHUB_PLAYWRIGHT||'playwright');
const root=path.resolve(__dirname,'..'),data=fs.mkdtempSync(path.join(os.tmpdir(),'mchub-close-ask-'));
const evidence=path.join(root,'target/close-ask-ui');fs.mkdirSync(evidence,{recursive:true});
const exe=path.join(root,'target/release/mc-server-hub.exe');
const report={data,checks:[]},delay=ms=>new Promise(r=>setTimeout(r,ms));
const pass=s=>{report.checks.push(s);console.log('PASS',s)};
async function port(){const s=require('node:net').createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const n=s.address().port;await new Promise(r=>s.close(r));return n}
function real(){return Object.fromEntries(['hub_config.json','config.json','hub_config.1x-backup.json'].map(n=>{const p=path.join(process.env.APPDATA,'MCServerHub',n);return[n,fs.existsSync(p)?crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex'):null]}))}
let child,browser,page,exited;
const invoke=(route,body={})=>page.evaluate(({route,body})=>window.__TAURI_INTERNALS__.invoke('api',{path:route,body}),{route,body});
const prefs=async()=>(await invoke('/api/settings')).preferences;
const visible=async()=>(await invoke('/api/desktop/status')).visible;
const closeX=()=>page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('close_window'));
const dialog=()=>page.getByRole('dialog').filter({hasText:'關閉視窗時要做什麼？'});
async function launch(){
 const cdp=await port();
 child=spawn(exe,[],{windowsHide:true,env:{...process.env,MCHUB_DATA_DIR:data,WEBVIEW2_USER_DATA_FOLDER:path.join(data,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${cdp}`},stdio:'ignore'});
 exited=new Promise(r=>child.once('exit',r));
 for(let i=0;i<100;i++){try{browser=await chromium.connectOverCDP(`http://127.0.0.1:${cdp}`);break}catch{if(child.exitCode!==null)throw Error('程式提早結束 '+child.exitCode);await delay(150)}}
 assert(browser,'CDP 沒有啟動');page=browser.contexts()[0].pages()[0];page.setDefaultTimeout(7000);
 page.on('pageerror',e=>{report.errors??=[];report.errors.push(e.message)});
 await page.waitForFunction(()=>typeof window.__TAURI_INTERNALS__?.invoke==='function');await page.locator('#nav-settings').waitFor();
 assert.equal((await invoke('/api/desktop/status')).isolated,true,'一定要在 TEMP 隔離模式');
}
async function waitExit(){await Promise.race([exited,delay(7000).then(()=>{throw Error('程式沒有正常結束')})]);await browser.close().catch(()=>{});browser=null}
async function wake(){const second=spawn(exe,[],{windowsHide:true,env:{...process.env,MCHUB_DATA_DIR:data},stdio:'ignore'});assert.equal(await new Promise(r=>second.on('exit',r)),0);for(let i=0;i<20&&!await visible();i++)await delay(150);assert.equal(await visible(),true,'第二實例沒有叫回視窗')}
async function main(){
 report.before=real();
 fs.writeFileSync(path.join(data,'config.json'),JSON.stringify({servers:[],update_check:false,preferences:{close_action:'quit',channel:'beta'}}));
 try{
  await launch();
  assert.equal(await page.locator('.brand small').innerText(),'v2.0.0');
  let p=await prefs();assert.equal(p.close_asked,false);assert.equal(p.channel,'stable');
  await page.locator('#nav-settings').click();await page.getByText('還沒選過：按 X 時會先問你一次。').waitFor();
  await page.getByRole('button',{name:'更新',exact:true}).click();await page.getByText('目前版本 2.0.0',{exact:false}).waitFor();assert.equal(await page.getByLabel('更新頻道').count(),0);
  await page.getByRole('button',{name:'關於',exact:true}).click();await page.getByText('MC 開服平台 2.0.0',{exact:true}).waitFor();
  await page.getByRole('button',{name:'一般',exact:true}).click();
  pass('舊設定（2.0.0 的 quit＋beta）：仍會詢問、頻道當穩定版且選項已隱藏；三處版本號讀自程式');
  // 情境一：不記住
  await closeX();await dialog().waitFor();assert.equal(await dialog().getByLabel('縮到系統匣（伺服器繼續跑）',{exact:false}).isChecked(),true,'預設要選縮到系統匣');
  await page.screenshot({path:path.join(evidence,'close-ask.png')});
  await dialog().getByRole('button',{name:'確定',exact:true}).click();for(let i=0;i<20&&await visible();i++)await delay(100);
  assert.equal(await visible(),false);assert.equal((await prefs()).close_asked,false);await wake();
  await closeX();await dialog().waitFor();await dialog().getByRole('button',{name:'取消',exact:true}).click();assert.equal(await dialog().count(),0);assert.equal(await visible(),true);
  pass('不記住：縮到真系統匣、設定不變；下次按 X 再問一次；取消會留在視窗');
  // 情境二：記住縮匣
  await closeX();await dialog().waitFor();await dialog().getByLabel('記住我的選擇',{exact:false}).check();await dialog().getByRole('button',{name:'確定',exact:true}).click();
  for(let i=0;i<20&&await visible();i++)await delay(100);assert.equal(await visible(),false);p=await prefs();assert.equal(p.close_action,'tray');assert.equal(p.close_asked,true);
  await wake();await closeX();await delay(400);assert.equal(await visible(),false,'記住後應直接縮下去');assert.equal(await dialog().count(),0);await wake();
  assert.equal(await page.getByText('還沒選過：按 X 時會先問你一次。').count(),0);
  pass('記住縮匣：存 close_action=tray、close_asked=true；之後按 X 直接縮匣不再問');
  // 情境三：記住結束（先把詢問重設回未選）
  await invoke('/api/settings',{preferences:{close_asked:false}});
  await closeX();await dialog().waitFor();await dialog().getByLabel('結束平台',{exact:false}).check();await dialog().getByLabel('記住我的選擇',{exact:false}).check();
  await dialog().getByRole('button',{name:'確定',exact:true}).click();await waitExit();
  p=JSON.parse(fs.readFileSync(path.join(data,'config.json'),'utf8')).preferences;assert.equal(p.close_action,'quit');assert.equal(p.close_asked,true);
  await launch();await closeX().catch(()=>{});await waitExit();
  pass('記住結束：存 close_action=quit、close_asked=true；重開後按 X 直接結束不再問');
  // 設定頁改關窗方式也算選過
  await launch();await invoke('/api/settings',{preferences:{close_asked:false}});await page.locator('#nav-settings').click();
  await page.getByLabel('關閉視窗時').selectOption('tray');await page.getByRole('button',{name:'儲存設定',exact:true}).click();await page.getByText('所有變更已儲存',{exact:true}).waitFor();
  assert.equal((await prefs()).close_asked,true);pass('在設定頁改「關閉視窗時」也視為選過');
  await invoke('/api/desktop/quit');await waitExit();
  report.after=real();assert.deepEqual(report.after,report.before);assert(!report.errors?.length,JSON.stringify(report.errors));pass('真 APPDATA 三檔雜湊不變；無畫面錯誤；剪貼簿未操作');
 }catch(e){report.failure=String(e);if(page&&browser)await page.screenshot({path:path.join(evidence,'failure.png')}).catch(()=>{});throw e}
 finally{
  if(browser&&page){try{await invoke('/api/desktop/quit',{discard:true});await waitExit()}catch{console.error('保留本輪程式，未強制結束任何程序')}}
  fs.writeFileSync(path.join(evidence,'report.json'),JSON.stringify(report,null,2));
 }
 console.log('COMPLETE',report.checks.length);
}
main().catch(e=>{console.error(e);process.exitCode=1});
