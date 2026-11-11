#!/usr/bin/python3
import json
import os
from pathlib import Path
import sys

root = Path('/tmp/fixture')
assert os.environ['XDG_RUNTIME_DIR'] == '/tmp/fixture/runtime'
assert os.environ['DBUS_SESSION_BUS_ADDRESS'] == 'unix:path=/tmp/fixture/runtime/bus'
assert (root / 'fixture-marker').read_text() == 'calendar-activation-test'
arguments = sys.argv[1:]
assert arguments[0] == '--user', arguments
with (root / 'manager-calls.jsonl').open('a') as log:
    log.write(json.dumps(arguments) + '\n')
if (root / 'unavailable').exists():
    sys.exit('isolated manager is unavailable')
operation = arguments[1]
unit = arguments[-1]
record = root / 'runtime' / ('loaded-' + unit)
if operation == 'show':
    print('Id=' + unit)
    print('LoadState=' + ('loaded' if record.exists() else 'not-found'))
    print('FragmentPath=' + (record.read_text() if record.exists() else ''))
elif operation == 'enable':
    assert unit.endswith('.timer'), arguments
    record.write_text('/tmp/fixture/config/systemd/user/' + unit)
elif operation == 'disable':
    record.unlink(missing_ok=True)
elif operation not in ('daemon-reload', 'restart'):
    sys.exit('unexpected fixture operation: ' + repr(arguments))
