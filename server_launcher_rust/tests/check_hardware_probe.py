# Read-only probe fixture; run with python3. Hardware I/O and subprocesses are mocked.
import contextlib, io, json, types
from pathlib import Path
from unittest.mock import patch
workspace = Path(__file__).resolve().parents[2]
source = (workspace / 'server_launcher_rust/src/sensors.rs').read_text()
script=source.split('const PROTECTED_HARDWARE_SCRIPT: &str = r##"',1)[1].split('"##;',1)[0]
windows_source = (workspace / 'server_launcher_windows/src/sensors.rs').read_text()
assert windows_source.split('const PROTECTED_HARDWARE_SCRIPT: &str = r##\"', 1)[1].split('\"##;', 1)[0] == script
energies=iter(['1000000','2000000'])
def fake_open(path,*args,**kwargs):
    if path.endswith('/max_energy_range_uj'): return io.StringIO('1000000000')
    if path.endswith('/energy_uj'): return io.StringIO(next(energies))
    raise FileNotFoundError(path)
def fake_glob(pattern):
    if 'powercap' in pattern: return ['/sys/class/powercap/intel-rapl:0']
    if '/drm/' in pattern: return ['/sys/class/drm/card0']
    return []
def fake_path(path):
    if path.endswith('/device/driver'): return '/drivers/i915'
    if path.endswith('/device'): return '/pci/intel'
    return path
def fake_run(args,**kwargs):
    if args[0]=='dmidecode':
        return types.SimpleNamespace(returncode=0, stderr='', stdout='Memory Device\n\tSize: 8192 MB\n\tType: DDR4\n\tConfigured Memory Speed: 2400 MT/s\n\tLocator: DIMM A\n')
    assert args[0]=='intel_gpu_top'
    return types.SimpleNamespace(returncode=0, stderr='', stdout=json.dumps([{'engines':{'Render/3D':{'busy':12.5},'Copy':{'busy':0}},'power':{'GPU':0.4}}]))
output=io.StringIO()
with patch('builtins.open',fake_open),patch('glob.glob',fake_glob),patch('os.path.realpath',fake_path),patch('subprocess.run',fake_run),patch('sys.argv',['probe','inventory']),patch('time.monotonic',side_effect=[100.0,100.5,100.5]),patch.dict('os.environ',{},clear=False),contextlib.redirect_stdout(output):
    exec(compile(script,'hardware-probe','exec'),{})
result=json.loads(output.getvalue())
assert result['cpu_watts']==2.0, result
assert result['gpus'][0]['engines']['Render/3D']==12.5
assert result['gpus'][0]['device']=='/pci/intel'
assert result['memory'][0]['kind']=='DDR4'
assert result['memory'][0]['speed']=='2400 MT/s'
assert result['notes']==[]
# A partial package measurement must not be reported as total CPU power.
energies=iter(['1000000','2000000'])
def missing_package_open(path,*args,**kwargs):
    if 'intel-rapl:1/energy_uj' in path:
        raise PermissionError(path)
    return fake_open(path,*args,**kwargs)
def two_package_glob(pattern):
    if 'powercap' in pattern:
        return ['/sys/class/powercap/intel-rapl:0', '/sys/class/powercap/intel-rapl:1']
    return fake_glob(pattern)
output=io.StringIO()
with patch('builtins.open',missing_package_open),patch('glob.glob',two_package_glob),patch('os.path.realpath',fake_path),patch('subprocess.run',fake_run),patch('sys.argv',['probe','inventory']),patch('time.monotonic',side_effect=[100.0,100.5,100.5]),patch.dict('os.environ',{},clear=False),contextlib.redirect_stdout(output):
    exec(compile(script,'hardware-probe','exec'),{})
result=json.loads(output.getvalue())
assert result['cpu_watts'] is None, result
assert any('intel-rapl:1' in note for note in result['notes'])
# Malformed GPU output must not discard independently valid CPU and memory data.
energies=iter(['1000000','2000000'])
def malformed_gpu_run(args,**kwargs):
    if args[0] == 'intel_gpu_top':
        return types.SimpleNamespace(returncode=0, stderr='', stdout='{"engines": {}}')
    return fake_run(args,**kwargs)
output=io.StringIO()
with patch('builtins.open',fake_open),patch('glob.glob',fake_glob),patch('os.path.realpath',fake_path),patch('subprocess.run',malformed_gpu_run),patch('sys.argv',['probe','inventory']),patch('time.monotonic',side_effect=[100.0,100.5,100.5]),patch.dict('os.environ',{},clear=False),contextlib.redirect_stdout(output):
    exec(compile(script,'hardware-probe','exec'),{})
result=json.loads(output.getvalue())
assert result['cpu_watts'] == 2.0 and result['memory'] and not result['gpus']
assert any('unexpected GPU sample format' in note for note in result['notes'])
print('Hardware probe fixture passed: CPU packages, GPU samples, device identity and RAM metadata.')
