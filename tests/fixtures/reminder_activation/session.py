import os
from pathlib import Path
import subprocess
import sys
import time

root = Path('/tmp/fixture')
assert os.environ['HOME'] == '/tmp/fixture/home'
assert os.environ['XDG_RUNTIME_DIR'] == '/tmp/fixture/runtime'
assert os.environ['DBUS_SESSION_BUS_ADDRESS'] == 'unix:path=/tmp/fixture/runtime/bus'
assert (root / 'fixture-marker').read_text() == 'calendar-activation-test'
config = root / 'bus.conf'
config.write_text('''<busconfig>
  <type>session</type>
  <listen>unix:path=/tmp/fixture/runtime/bus</listen>
  <policy context="default">
    <allow own="*"/><allow send_destination="*"/><allow receive_sender="*"/>
  </policy>
</busconfig>''')
bus = subprocess.Popen(['/usr/bin/dbus-daemon', '--nofork', '--config-file=' + str(config)])
try:
    deadline = time.monotonic() + 5
    while not (root / 'runtime/bus').exists():
        if bus.poll() is not None or time.monotonic() >= deadline:
            raise RuntimeError('private bus did not become ready')
        time.sleep(0.01)
    result = subprocess.run(sys.argv[1:], timeout=30)
    sys.exit(result.returncode)
finally:
    bus.terminate()
    try:
        bus.wait(timeout=3)
    except subprocess.TimeoutExpired:
        bus.kill()
        bus.wait(timeout=3)
    (root / 'runtime/bus').unlink(missing_ok=True)
