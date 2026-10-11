"""Exercise one PDFStrike project command through the real native UI."""

import base64
import json
import socket
import sys
import zipfile
from pathlib import Path

from native_smoke import request


def manifest(path):
    with zipfile.ZipFile(path) as archive:
        return json.loads(archive.read("manifest.json"))


def main():
    control = json.loads(Path(sys.argv[1]).read_text())
    project = Path(sys.argv[2])
    output = Path(sys.argv[3])
    output.mkdir(parents=True, exist_ok=True)
    before = manifest(project)
    with socket.create_connection(("127.0.0.1", control["port"]), timeout=30) as sock:
        sock.settimeout(30)
        with sock.makefile("rwb") as stream:
            request(stream, "auth", {"token": control["token"]}, 1)
            inspected = request(stream, "ui.inspect", {"query": "Rotate right", "limit": 20}, 2)
            matches = [w for w in inspected["widgets"] if w.get("label") == "Rotate right" and w.get("clickable")]
            if len(matches) != 1:
                raise AssertionError(f"expected one clickable Rotate right control, found {len(matches)}")
            request(stream, "ui.click", {"id": matches[0]["id"]}, 3)
            state = request(stream, "ui.state", {}, 4)
            shot = request(stream, "ui.screenshot", {}, 5)
            (output / "edited-window.png").write_bytes(base64.b64decode(shot["png_base64"]))
    after = manifest(project)
    assert after["source_sha256"] == before["source_sha256"]
    assert after["cursor"] == before["cursor"] + 1
    assert after["edits"][-1] == {"Rotate": {"page": 0, "degrees": 90}}
    assert state["mode"] == "Read"
    assert not state["active"]["page_errors"]
    print(json.dumps({"edits_before": before["cursor"], "edits_after": after["cursor"], "mode": state["mode"], "pages": state["documents"][0]["pages"]}))


if __name__ == "__main__":
    main()
