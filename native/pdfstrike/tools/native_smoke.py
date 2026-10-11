"""Read-only test of PDFStrike's opt-in loopback UI control channel."""

import base64
import json
import socket
import sys
from pathlib import Path


def request(stream, method, params, ident):
    stream.write(json.dumps({"jsonrpc": "2.0", "id": ident, "method": method, "params": params}).encode() + b"\n")
    stream.flush()
    reply = json.loads(stream.readline())
    if "error" in reply:
        raise RuntimeError(f"{method}: {reply['error']}")
    return reply["result"]


def main():
    control = json.loads(Path(sys.argv[1]).read_text())
    output = Path(sys.argv[2])
    output.mkdir(parents=True, exist_ok=True)
    with socket.create_connection(("127.0.0.1", control["port"]), timeout=30) as sock:
        sock.settimeout(30)
        with sock.makefile("rwb") as stream:
            request(stream, "auth", {"token": control["token"]}, 1)
            state = request(stream, "ui.state", {}, 2)
            (output / "native-state.json").write_text(json.dumps(state, indent=2))
            shot = request(stream, "ui.screenshot", {}, 3)
            (output / "native-window.png").write_bytes(base64.b64decode(shot["png_base64"]))
            print(json.dumps({"documents": len(state.get("documents", [])), "active": state.get("active"), "size": [shot["width"], shot["height"]]}))


if __name__ == "__main__":
    main()
