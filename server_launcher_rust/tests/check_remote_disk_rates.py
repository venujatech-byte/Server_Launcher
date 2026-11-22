# Check the shared SSH collector's counter and mount handling without hardware access.
import ast
import io
import os
from pathlib import Path
from unittest.mock import patch

workspace = Path(__file__).resolve().parents[2]

def collector(path):
    source = (workspace / path).read_text()
    return source.split('let script = r#"python3 -c \'\n', 1)[1].split("\n'\n\"#;", 1)[0]

script = collector('server_launcher_rust/src/sensors.rs')
assert collector('server_launcher_windows/src/sensors.rs') == script
node = ast.parse(script)
start = script.index('disk_rates = {}\nfor dev, (r2, w2) in d2.items():')
end = script.index('\ndef disk_rate_for_source', start)
counters = {
    'd1': {'idle': (5, 8), 'reset': (9, 9), 'busy': (2, 3)},
    'd2': {'idle': (5, 8), 'reset': (2, 10), 'busy': (4, 7), 'new': (1, 1)},
    't_disk_elapsed': 2.0,
}
exec(script[start:end], counters)
assert counters['disk_rates']['idle'][:2] == (0.0, 0.0)
assert counters['disk_rates']['busy'][:2] == (1.0, 2.0)
assert 'reset' not in counters['disk_rates'] and 'new' not in counters['disk_rates']

functions = [n for n in node.body if isinstance(n, ast.FunctionDef)
             and n.name in {'disk_rate_for_source', 'mounted_disk_totals', 'is_base_block_device', 'read_disk_stats', 'disk_discard_supported', 'disk_bus_interface'}]
env = {'os': os}
exec(compile(ast.Module(body=functions, type_ignores=[]), '<disk fixture>', 'exec'), env)
diskstats = '8 0 sda 1 2 bad 4 5 6 7\n8 16 sdb 1 2 3 4 5 6 7\n'
with patch('builtins.open', return_value=io.StringIO(diskstats)):
    assert env['read_disk_stats']() == {'sdb': (3 * 512, 7 * 512)}
rates = {'sda1': (0.0, 0.0, 1, 2), 'sdb1': (3.0, 4.0, 1, 2)}
mounts = [{'name': '/dev/sda1'}, {'name': '/dev/sda1'}, {'name': '/dev/sdb1'}]
assert env['mounted_disk_totals'](mounts, rates) == (3.0, 4.0)
assert env['mounted_disk_totals'](mounts, {'sda1': rates['sda1']}) == (None, None)
assert env['mounted_disk_totals']([], rates) == (None, None)
assert env['disk_rate_for_source']('/dev/sda1', rates) == rates['sda1']
assert env['disk_rate_for_source']('tmpfs', rates) is None
with patch('builtins.open', return_value=io.StringIO('invalid')):
    assert env['disk_discard_supported']('/disk') is None
with patch('builtins.open', side_effect=PermissionError):
    assert env['disk_discard_supported']('/disk') is None
with patch('builtins.open', return_value=io.StringIO('4096')):
    assert env['disk_discard_supported']('/disk') is True
with patch('os.path.exists', return_value=False):
    assert env['disk_bus_interface']('/disk') == ''
with patch('os.path.isdir', return_value=True), patch('os.path.exists', return_value=False), \
     patch('os.listdir', side_effect=lambda path: ['sda', 'sdb'] if '/md0/' in path else []):
    assert env['is_base_block_device']('sda')
    assert not env['is_base_block_device']('md0')
    assert not env['is_base_block_device']('nbd0')
    assert not env['is_base_block_device']('mmcblk0boot0')
# A mounted volume alone must never be invented as a physical SSD or partition.
inventory_start = script.index('phys_disks = []\ntry:')
inventory_end = script.index('\ndata = {', inventory_start)
class NoInventory:
    @staticmethod
    def glob(_pattern):
        return []
inventory = {'glob': NoInventory(), 'disks': [{'name': '/dev/sda1', 'mount': '/'}]}
exec(script[inventory_start:inventory_end], inventory)
assert inventory['phys_disks'] == []
print('Remote disk fixtures passed: zero, reset, incomplete totals and missing inventory.')
