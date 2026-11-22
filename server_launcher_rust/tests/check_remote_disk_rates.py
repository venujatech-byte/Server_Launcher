# Check the shared SSH collector's counter and mount handling without hardware access.
import ast
import os
from pathlib import Path

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
             and n.name in {'disk_rate_for_source', 'mounted_disk_totals'}]
env = {'os': os}
exec(compile(ast.Module(body=functions, type_ignores=[]), '<disk fixture>', 'exec'), env)
rates = {'sda1': (0.0, 0.0, 1, 2), 'sdb1': (3.0, 4.0, 1, 2)}
mounts = [{'name': '/dev/sda1'}, {'name': '/dev/sda1'}, {'name': '/dev/sdb1'}]
assert env['mounted_disk_totals'](mounts, rates) == (3.0, 4.0)
assert env['mounted_disk_totals'](mounts, {'sda1': rates['sda1']}) == (None, None)
assert env['mounted_disk_totals']([], rates) == (None, None)
assert env['disk_rate_for_source']('/dev/sda1', rates) == rates['sda1']
assert env['disk_rate_for_source']('tmpfs', rates) is None
print('Remote disk-rate fixtures passed: zero, reset, new devices and incomplete totals.')
