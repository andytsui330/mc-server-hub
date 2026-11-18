const fs=require('node:fs'),path=require('node:path'),os=require('node:os'),assert=require('node:assert/strict');
const {spawnSync}=require('node:child_process');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'mchub-panic-'));
const r=spawnSync(path.resolve(__dirname,'../target/debug/examples/crash_probe.exe'),[],{windowsHide:true,encoding:'utf8',env:{...process.env,MCHUB_DATA_DIR:dir}});
assert.equal(r.status,101);
const files=fs.readdirSync(dir).filter(n=>n.startsWith('crash-')&&n.endsWith('.json'));assert.equal(files.length,1);
const report=fs.readFileSync(path.join(dir,files[0]),'utf8');
assert(!/PRIVATE|PAYLOAD|Users|path/.test(report));assert.equal(JSON.parse(report).kind,'panic');
console.log('PASS real panic hook: one metadata-only report, private payload absent');console.log('Evidence:',dir);
