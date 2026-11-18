// 真 Tauri updater、真簽章、HTTP 假更新；所有金鑰與安裝副作用都留 TEMP。
const fs=require('node:fs'),path=require('node:path'),os=require('node:os'),http=require('node:http'),assert=require('node:assert/strict');
const {spawn,spawnSync}=require('node:child_process');
const root=path.resolve(__dirname,'..'),data=fs.mkdtempSync(path.join(os.tmpdir(),'mchub-update-'));
function run(exe,args,extra={}){const r=spawnSync(exe,args,{cwd:root,windowsHide:true,encoding:'utf8',env:{...process.env,MCHUB_DATA_DIR:data,...extra}});assert.equal(r.status,0,`${path.basename(exe)} failed (output withheld for keys)`);return r;}
async function main(){
 const cli=path.join(root,'app/node_modules/@tauri-apps/cli/tauri.js');
 run(process.execPath,[cli,'signer','generate','--ci','-w',path.join(data,'test.key')]);
 const installer=path.join(data,'fake-installer.exe');
 run(path.join(os.homedir(),'.cargo/bin/rustc.exe'),[path.join(root,'tests/fake-update-installer.rs'),'-o',installer]);
 fs.writeFileSync(path.join(data,'broken.exe'),Buffer.from('not an installer'));
 for(const file of [installer,path.join(data,'broken.exe')])run(process.execPath,[cli,'signer','sign','-f',path.join(data,'test.key'),'--app-version','2.0.1',file]);
 const bytes=fs.readFileSync(installer),signature=fs.readFileSync(installer+'.sig','utf8').trim();
 const broken=fs.readFileSync(path.join(data,'broken.exe')),brokenSig=fs.readFileSync(path.join(data,'broken.exe.sig'),'utf8').trim();
 let downloads=0;const sockets=new Set();
 const server=http.createServer((req,res)=>{
   if(req.url==='/download'){downloads++;res.end(bytes);return}
   if(req.url==='/tampered-download'){downloads++;const bad=Buffer.from(bytes);bad[bad.length-1]^=1;res.end(bad);return}
   if(req.url==='/broken-download'){downloads++;res.end(broken);return}
   if(req.url==='/stall-download'){downloads++;res.writeHead(200,{'content-length':bytes.length});res.write(bytes.subarray(0,100));return}
   if(req.url==='/none'){res.writeHead(204).end();return}
   if(req.url==='/error'){res.writeHead(503).end();return}
   const brokenCase=req.url==='/broken';
   const url=`http://127.0.0.1:${server.address().port}/${brokenCase?'broken-download':req.url==='/stall'?'stall-download':req.url==='/tampered'?'tampered-download':'download'}`;
   res.setHeader('content-type','application/json');res.end(JSON.stringify({version:req.url==='/wrong-version'?'2.0.2':'2.0.1',notes:'本機假更新',pub_date:'2026-11-15T00:00:00Z',url,signature:req.url==='/bad'?'invalid signature':brokenCase?brokenSig:signature}));
 });
 server.on('connection',s=>{sockets.add(s);s.on('close',()=>sockets.delete(s))});
 await new Promise(r=>server.listen(0,'127.0.0.1',r));
 const exe=path.join(root,'target/debug/examples/update_probe.exe');
 // Cargo examples 不會繼承 Tauri 主程式的 Windows v6 controls manifest。
 fs.writeFileSync(exe+'.manifest','<?xml version="1.0" encoding="UTF-8" standalone="yes"?><assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0"><dependency><dependentAssembly><assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="amd64" publicKeyToken="6595b64144ccf1df" language="*"/></dependentAssembly></dependency></assembly>');
 const digest=()=>require('node:crypto').createHash('sha256').update(fs.readFileSync(exe)).digest('hex');const original=digest();
 async function probe(route,install=false){return await new Promise((resolve,reject)=>{
   const p=spawn(exe,[],{cwd:root,windowsHide:true,env:{...process.env,MCHUB_DATA_DIR:data,MCHUB_PROBE_URL:`http://127.0.0.1:${server.address().port}${route}`,MCHUB_PROBE_INSTALL:install?'1':'0',MCHUB_PROBE_STALL:route==='/stall'?'1':'0'},stdio:['ignore','pipe','pipe']});
   let stdout='';p.stdout.on('data',b=>stdout+=b);p.on('error',reject);p.on('exit',code=>resolve({code,stdout}));
 });}
 try{
   assert.deepEqual(await probe('/none'),{code:0,stdout:'NO_UPDATE\n'});assert.equal(downloads,0);
   assert.equal((await probe('/error')).code,2);assert.equal(downloads,0);
   assert.equal((await probe('/bad')).code,2);assert(!fs.existsSync(path.join(data,'before-exit')));
   assert.equal((await probe('/tampered')).code,2);assert(!fs.existsSync(path.join(data,'before-exit')));
   assert.equal((await probe('/wrong-version')).code,2);assert(!fs.existsSync(path.join(data,'before-exit')));
   const began=Date.now();assert.equal((await probe('/stall')).code,2);assert(Date.now()-began<5000,'停滯下載必須逾時');assert(!fs.existsSync(path.join(data,'before-exit')));
   assert.equal((await probe('/broken',true)).code,2);assert(!fs.existsSync(path.join(data,'before-exit')));
   assert.equal((await probe('/valid')).code,0);assert(!fs.existsSync(path.join(data,'installed')));
   assert.equal(digest(),original,'失敗或僅下載時不改原程式');
   assert.equal((await probe('/valid',true)).code,0);
   for(let i=0;i<40&&!fs.existsSync(path.join(data,'installed'));i++)await new Promise(r=>setTimeout(r,100));
   assert.equal(fs.readFileSync(path.join(data,'installed'),'utf8'),'fake-update-installed');
   assert.equal(fs.readFileSync(path.join(data,'before-exit'),'utf8'),'before-exit');
   assert.equal(digest(),original,'假安裝器只寫TEMP標記，原程式保留');
   console.log('PASS updater: 204 / HTTP failure / bad signature / tampered bytes / version binding / stalled download timeout / invalid package / verified download / signed fake installer');
   console.log('Evidence:',data);
 }finally{for(const socket of sockets)socket.destroy();server.close();}
}
main().catch(e=>{console.error(e);process.exitCode=1});
