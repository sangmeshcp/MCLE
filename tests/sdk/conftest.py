import json
import os
import subprocess
import tempfile
import time
import urllib.request

import pytest

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


def _binary():
    path = os.environ.get("MCLE_BIN") or os.path.join(ROOT, "target", "debug", "mcle")
    if not os.path.exists(path):
        subprocess.run(["cargo", "build", "-p", "mcle-cli"], cwd=ROOT, check=True)
    return path


@pytest.fixture(scope="session")
def mcle():
    """Starts one emulator on a random port; yields its base URL."""
    with tempfile.TemporaryDirectory() as d:
        port_file = os.path.join(d, "port.json")
        proc = subprocess.Popen([_binary(), "start", "--port", "0", "--port-file", port_file, "--seed", "7"])
        deadline = time.time() + 10
        while not os.path.exists(port_file) or os.path.getsize(port_file) == 0:
            if time.time() > deadline or proc.poll() is not None:
                proc.kill()
                raise RuntimeError("mcle did not start")
            time.sleep(0.02)
        with open(port_file) as f:
            url = f"http://localhost:{json.load(f)['http']}"
        yield url
        proc.terminate()
        proc.wait(timeout=5)


@pytest.fixture
def sandbox(mcle, request):
    """Isolates each test in its own sandbox; yields (url, sandbox_id)."""
    sid = request.node.name.replace("[", "-").replace("]", "").lower()
    req = urllib.request.Request(f"{mcle}/_mcle/sandboxes", data=json.dumps({"id": sid}).encode(), method="POST")
    body = json.load(urllib.request.urlopen(req))
    return mcle, body
