// Installed binary only; fixture ownership checked before every server mutation.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),{spawn}=require('node:child_process');
const {chromium}=require(process.env.MCHUB_PLAYWRIGHT||'playwright');
const plan=JSON.parse(fs.readFileSync(process.argv[2],'utf8')),exe=path.join(plan.install_dir,'mc-server-hub.exe'),out=process.argv[3];
fs.mkdirSync(out,{recursive:true});const report={checks:[],errors:[]},delay=ms=>new Promise(r=>setTimeout(r,ms));let child,exited,browser,page;
const api=(route,body={})=>page.evaluate(({route,body})=>window.__TAURI_INTERNALS__.invoke('api',{path:route,body}),{route,body});
async function owned(){const s=await api('/api/state');assert.equal(s.servers.length,1);assert.equal(s.servers[0].id,'installer-fixture');assert.equal(path.resolve(s.servers[0].dir).toLowerCase(),path.resolve(plan.fake_server_dir).toLowerCase());return s.servers[0]}
async function until(fn,ms=20000){const end=Date.now()+ms;while(Date.now()<end){if(await fn())return;await delay(100)}throw Error('等待狀態逾時')}
async function port(){const s=require('node:net').createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const n=s.address().port;await new Promise(r=>s.close(r));return n}
async function launch(){const cdp=await port();child=spawn(exe,[],{windowsHide:true,env:{...process.env,...plan.environment,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${cdp} --remote-debugging-address=127.0.0.1`},stdio:'ignore'});exited=new Promise(r=>child.once('exit',r));for(let i=0;i<120;i++){try{browser=await chromium.connectOverCDP(`http://127.0.0.1:${cdp}`);break}catch{if(child.exitCode!==null)throw Error('已安裝程式提前結束');await delay(150)}}assert(browser);page=browser.contexts()[0].pages()[0];page.setDefaultTimeout(10000);page.on('pageerror',e=>report.errors.push(e.message));await page.locator('#nav-settings').waitFor();await owned();await page.setViewportSize({width:1360,height:900})}
async function quit(){assert.equal((await owned()).state,'stopped');await api('/api/desktop/quit',{discard:true}).catch(()=>{});await Promise.race([exited,delay(7000).then(()=>{throw Error('程式未退出')})]);await browser.close();browser=null;page=null}
const btn=name=>page.getByRole('button',{name,exact:true}),pass=s=>{report.checks.push(s);console.log('PASS',s)};
async function main(){try{
 await launch();pass('安裝目錄的exe以獨立資料啟動，只有指定假服');
 await page.screenshot({path:path.join(out,'installed-stopped.png')});
 await btn('開機').click();await btn('確認並開機').waitFor();await btn('確認並開機').click();await until(async()=>(await owned()).state==='running');await until(async()=>await page.locator('#h-power').innerText()==='關機');pass('已安裝版本經開機檢查，真Java假服到Done');
 await page.locator('#cmd').fill('say 安裝版隔離測試');await btn('送出').click();await until(async()=>(await page.locator('.log').innerText()).includes('收到指令：say 安裝版隔離測試'));await page.screenshot({path:path.join(out,'installed-running.png')});pass('主控台送指令與回覆正常');
 await owned();await btn('關機').click();await page.getByRole('dialog').getByRole('button',{name:'關機',exact:true}).click();await until(async()=>(await owned()).state==='stopped');pass('確認後正常stop並退出假服');
 await page.locator('#nav-settings').click();await btn('外觀').click();await page.getByLabel('字級',{exact:false}).selectOption('16');await btn('儲存設定').click();await page.getByText('所有變更已儲存',{exact:true}).waitFor();await page.screenshot({path:path.join(out,'installed-settings.png')});await quit();
 await launch();const settings=await api('/api/settings');assert.equal(settings.preferences.font_size,16);assert.equal(settings.update_check,false);assert.equal((await owned()).state,'stopped');await quit();pass('安裝版設定重啟仍在，未自動開服或檢查正式更新');
 }catch(e){report.failure=String(e);if(page)await page.screenshot({path:path.join(out,'failure.png')}).catch(()=>{});throw e}finally{if(page&&browser){try{const s=await owned();if(['running','starting'].includes(s.state)){await api('/api/stop',{id:s.id});await until(async()=>(await owned()).state==='stopped')}await quit()}catch(e){report.cleanup=String(e)}}fs.writeFileSync(path.join(out,'report.json'),JSON.stringify(report,null,2))}assert.equal(report.errors.length,0);assert(!report.cleanup);console.log('COMPLETE',report.checks.length)}
main().catch(e=>{console.error(e);process.exitCode=1});
