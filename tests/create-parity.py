"""兩版完整建服工作：同組離線官方回應、同一 FakeServer/FakeInstaller。
Rust 入口 include 正式 create/hub/settings 原檔，僅取代網路傳輸，不修改產品。
需先 cargo build -p mc-hub-core --example create_parity；保留 TEMP 全部證據。
"""
import hashlib, importlib.util, io, json, os, shutil, socket, subprocess, tempfile, time, traceback, zipfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]


def main():
    raw = os.environ.get('MCHUB_DATA_DIR', '')
    root, temp = Path(raw).resolve(), Path(tempfile.gettempdir()).resolve()
    assert raw and Path(raw).is_absolute() and root != temp and root.is_relative_to(temp) and not root.exists()
    root.mkdir(); (root/'create-parity-marker').touch()
    appdata = Path(os.environ['APPDATA'])/'MCServerHub'
    def fingerprint():
        return {n: hashlib.sha256((appdata/n).read_bytes()).hexdigest() if (appdata/n).is_file() else None
                for n in ('hub_config.json', 'config.json', 'hub_config.1x-backup.json')}
    before = fingerprint()
    report = {'root': str(root), 'checks': [], 'before': before, 'transport': 'offline fixture responses; no official connection'}
    transcript = (root/'transcript.jsonl').open('w', encoding='utf-8')
    spec = importlib.util.spec_from_file_location('create_hub', REPO/'legacy/src/hub.py')
    old = importlib.util.module_from_spec(spec); spec.loader.exec_module(old)
    assert old.OS == 'win'
    for side in ('python', 'rust'):
        cfg = root/side/'config'; cfg.mkdir(parents=True)
        (cfg/('hub_config.json' if side == 'python' else 'config.json')).write_text(json.dumps({'servers': [], 'update_check': False}), encoding='utf-8')
    pyhub = old.Hub(root/'python/config/hub_config.json')
    rust = subprocess.Popen([str(REPO/'target/debug/examples/create_parity.exe')], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                            stderr=(root/'rust-stderr.log').open('w'), text=True, encoding='utf-8', creationflags=subprocess.CREATE_NO_WINDOW)
    def rpc(op, **body):
        rust.stdin.write(json.dumps({'op': op, **body})+'\n'); rust.stdin.flush()
        line = rust.stdout.readline(); assert line, f'Rust runner exited {rust.poll()}'
        out = json.loads(line)
        transcript.write(json.dumps({'request': {'op': op, **body}, 'response': out}, ensure_ascii=False)+'\n'); transcript.flush()
        return out
    def java_tool(name):
        for _, p in old.installed_javas():
            candidate = Path(p).with_name(name+'.exe')
            if candidate.is_file(): return str(candidate)
        raise AssertionError('JDK missing '+name)
    def wait(side, predicate, seconds=45):
        deadline = time.monotonic()+seconds
        while time.monotonic() < deadline:
            if side == 'python':
                for s in pyhub.all_servers(): s.poll_status()
                state = {'servers': [s.snapshot() for s in pyhub.all_servers()], 'job': pyhub.job}
            else:
                rpc('poll'); state = rpc('call', path='/api/state', body={})
            if predicate(state): return state
            time.sleep(.15)
        raise AssertionError(f'{side} timeout {state}')
    class Response(io.BytesIO):
        def __init__(self, data):
            super().__init__(data); self.headers = {'Content-Length': str(len(data))}
    responses = {}
    def fake_open(url):
        r = responses[url]
        if 'json' in r: return Response(json.dumps(r['json']).encode())
        if 'text' in r: return Response(r['text'].encode())
        if 'file' in r: return Response(Path(r['file']).read_bytes())
        raise OSError('test offline')
    old._open = fake_open
    try:
        assets = root/'assets'; assets.mkdir()
        classes = assets/'classes'; classes.mkdir()
        subprocess.run([java_tool('javac'), '-encoding', 'UTF-8', '-d', str(classes), str(REPO/'legacy/tests/FakeServer.java'), str(REPO/'legacy/tests/FakeInstaller.java')], check=True, creationflags=subprocess.CREATE_NO_WINDOW)
        def jar(name, main_class, fabric_version=None):
            p = assets/name
            with zipfile.ZipFile(p, 'w') as z:
                z.writestr('META-INF/MANIFEST.MF', f'Manifest-Version: 1.0\r\nMain-Class: {main_class}\r\n\r\n')
                for c in classes.glob('*.class'): z.write(c, c.name)
                if main_class == 'FakeServer': z.writestr('version.json', json.dumps({'id': '1.21.1', 'java_version': 21}))
                if fabric_version: z.writestr('install.properties', 'game-version='+fabric_version)
            return p
        vanilla = jar('server.jar', 'FakeServer')
        fabric = jar('fabric.jar', 'FakeServer', '1.21.1')
        badfabric = jar('bad-fabric.jar', 'FakeServer', '1.20.1')
        forge = jar('forge-1.21.1-52.1.999-installer.jar', 'FakeInstaller')
        neo = jar('neoforge-21.1.999-installer.jar', 'FakeInstaller')
        broken = assets/'corrupt.jar'; broken.write_bytes(b'broken')
        meta_url = 'https://piston-meta.mojang.com/test/version.json'
        jar_url = 'https://piston-data.mojang.com/test/server.jar'
        fabric_url = old.FABRIC_META+'/loader/1.21.1/0.16.14/1.0.3/server/jar'
        forge_url = old.forge_installer_url('1.21.1', '52.1.999')
        neo_url = old.neoforge_installer_url('21.1.999')
        def mapping(loader, scenario):
            sums = {str(p): hashlib.sha1(p.read_bytes()).hexdigest() for p in (vanilla, forge)}
            m = {
                old.MANIFEST_URL: {'json': {'versions': [{'id': '1.21.1', 'type': 'release', 'url': meta_url}]}},
                meta_url: {'json': {'javaVersion': {'majorVersion': 21}, 'downloads': {'server': {'url': jar_url, 'sha1': sums[str(vanilla)], 'size': vanilla.stat().st_size}}}},
                old.FABRIC_META+'/game': {'json': [{'version': '1.21.1', 'stable': True}]},
                old.FABRIC_META+'/loader/1.21.1': {'json': [{'loader': {'version': '0.16.14', 'stable': True}}]},
                old.FABRIC_META+'/installer': {'json': [{'version': '1.0.3', 'stable': True}]},
                old.FORGE_PROMOS: {'json': {'promos': {'1.21.1-recommended': '52.1.999'}}},
                old.NEOFORGE_VERSIONS: {'json': {'versions': ['21.1.999']}},
                jar_url: {'file': str(vanilla)}, fabric_url: {'file': str(fabric)},
                forge_url: {'file': str(forge)}, forge_url+'.sha1': {'text': sums[str(forge)]},
                neo_url: {'file': str(neo)}, neo_url+'.sha256': {'text': hashlib.sha256(neo.read_bytes()).hexdigest()},
            }
            if scenario == 'download-bad':
                url = {'vanilla': jar_url, 'fabric': fabric_url, 'forge': forge_url, 'neoforge': neo_url}[loader]
                m[url] = {'file': str(badfabric if loader == 'fabric' else broken)}
            return m
        # 佔住 25565，四種載入器的工作都必須挑其他空埠。
        occupied = socket.socket(); occupied.bind(('0.0.0.0', 25565))
        try:
            for loader in ('vanilla', 'fabric', 'forge', 'neoforge'):
                for scenario in ('success', 'download-bad') + (('installer-fail',) if loader in ('forge', 'neoforge') else ()):
                    responses = mapping(loader, scenario); rpc('network', responses=responses)
                    outcomes = []
                    for side in ('python', 'rust'):
                        name = ('fail-' if scenario == 'installer-fail' else '')+loader+'-'+scenario
                        parent = root/side/'servers'; parent.mkdir(exist_ok=True)
                        req = {'name': name, 'version': '1.21.1', 'loader': loader, 'ram_gb': 1, 'parent': str(parent), 'eula': True}
                        # 這裡同意的對象只限自行編譯的 FakeServer/FakeInstaller。
                        result = pyhub.start_create(req) if side == 'python' else rpc('call', path='/api/create', body=req)
                        assert result is None if side == 'python' else result == {'ok': True}, result
                        state = wait(side, lambda st: st['job'] and st['job']['state'] != 'running')
                        job = state['job']; target = parent/name
                        transcript.write(json.dumps({'side': side, 'loader': loader, 'scenario': scenario, 'job': job}, ensure_ascii=False)+'\n'); transcript.flush()
                        if scenario == 'success':
                            assert job['state'] == 'done' and not job.get('error'), job
                            sid = job['id']
                            state = wait(side, lambda st: any(s['id'] == sid and s['state'] == 'running' and s['online'] == 1 for s in st['servers']))
                            s = next(s for s in state['servers'] if s['id'] == sid)
                            assert s['port'] != 25565 and s['names'] == ['Tester']
                            assert 'eula=true' in (target/'eula.txt').read_text().splitlines()
                            assert (target/'mods').is_dir() == (loader != 'vanilla')
                            if side == 'python': pyhub.servers[sid].stop()
                            else: rpc('call', path='/api/stop', body={'id': sid})
                            wait(side, lambda st: next(s for s in st['servers'] if s['id'] == sid)['state'] == 'stopped')
                            outcomes.append({'job': 'done', 'version': s['version'], 'players': s['names'], 'mods': loader != 'vanilla', 'free_port': True, 'stopped': True})
                        else:
                            assert job['state'] == 'error' and job['error'], job
                            # 工作狀態先寫 error 再清半成品；等清理實際完成才驗。
                            for _ in range(100):
                                if not target.exists(): break
                                time.sleep(.02)
                            assert not target.exists(), '失敗不應留下本次新建的半成品'
                            if scenario == 'installer-fail':
                                assert '結束代碼 1' in job['error'] and '安裝紀錄已保留' in job['error']
                                assert list((root/side/'config/installer-logs').glob('*.log'))
                            elif loader == 'fabric': assert '核對不符' in job['error']
                            elif loader == 'vanilla': assert '下載不完整' in job['error']
                            else: assert '核對不符' in job['error']
                            outcomes.append({'job': 'error', 'partial_removed': True, 'installer_log': scenario == 'installer-fail'})
                    assert outcomes[0] == outcomes[1], outcomes
                    report['checks'].append({'loader': loader, 'scenario': scenario, 'outcome': outcomes[0]})
                    print('PASS', loader, scenario, flush=True)
        finally: occupied.close()
        report['status'] = 'passed'
    except BaseException:
        report['status'] = 'failed'; report['failure'] = traceback.format_exc(); raise
    finally:
        for s in pyhub.all_servers():
            if s.alive(): s.stop()
        deadline = time.monotonic()+20
        while pyhub.busy() and time.monotonic() < deadline: time.sleep(.1)
        if rust.poll() is None:
            state = rpc('call', path='/api/state', body={})
            for s in state['servers']:
                if s['state'] in ('running', 'starting'): rpc('call', path='/api/stop', body={'id': s['id']})
            for _ in range(100):
                rpc('poll'); state=rpc('call', path='/api/state', body={})
                if all(s['state']=='stopped' for s in state['servers']) and (not state['job'] or state['job']['state'] != 'running'): break
                time.sleep(.2)
            rust.stdin.write('{"op":"quit"}\n'); rust.stdin.flush(); rust.wait(timeout=10)
        report['after'] = fingerprint(); report['appdata_unchanged'] = before == report['after']
        (root/'create-report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
        transcript.close(); print('測試資料保留：', root, flush=True)
        assert report['appdata_unchanged']


if __name__ == '__main__':
    main()
