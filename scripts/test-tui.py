"""Exercise the real Alice TUI in a PTY against a local fixture server (Unix)."""
import fcntl
import json
import os
import pty
import select
import shutil
import struct
import subprocess
import tempfile
import termios
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse

account = dict(id="fixture-account", name="Fixture", email="me@example.invalid", provider="imap", imap_host="imap.invalid", imap_port=993, smtp_host="smtp.invalid", smtp_port=587, last_synced=None, sync_error=None)
message = dict(id="fixture-1", account_id=account["id"], from_name="Notifications", from_email="alerts@example.invalid", reply_to=["support@example.invalid"], to=[account["email"]], subject="Fixture email", preview="A short fixture", body="Hello", date="2026-10-11T00:00:00Z", folder="inbox", is_read=False, is_starred=False, category="work", attachments=[], message_id="<fixture@example.invalid>")
state = dict(accounts=[account], messages=[message], delay=0, saved=[], requests=[], patch_event=threading.Event(), loaded=threading.Event())

class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def out(self, data):
        body = json.dumps(data).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        try:
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            pass

    def do_GET(self):
        if self.path.startswith("/v1/health"):
            # The binary replaces a daemon of another build; the fixture stands in for this one.
            self.out(dict(status="ok", version="0.1.0", build=build))
        elif self.path.startswith("/v1/accounts"):
            self.out(state["accounts"])
            state["loaded"].set()
        elif self.path.startswith("/v1/preferences"):
            self.out(dict(theme="light", mail_layout="single", show_avatars=True, density="comfortable", message_format="html", remote_images="ask", thread_order="newest_first", signature="", default_account_id=""))
        else:
            state["requests"].append(self.path)
            query = parse_qs(urlparse(self.path).query)
            offset = int(query.get("offset", [0])[0])
            limit = int(query.get("limit", [100])[0])
            self.out(dict(messages=state["messages"][offset:offset+limit], total=len(state["messages"])))

    def do_PATCH(self):
        state["patch_event"].set()
        time.sleep(state["delay"])
        self.out(message)

    def do_POST(self):
        draft = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        state["saved"].append(draft)
        self.out(dict(message, **draft, folder="drafts"))

server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
server.daemon_threads = True
threading.Thread(target=server.serve_forever, daemon=True).start()
binary = str(Path("target/debug/alice").resolve())
build = subprocess.run([binary, "--version"], capture_output=True, text=True, check=True).stdout.strip().rpartition("(")[2].rstrip(")")
results = {}

class Terminal:
    def __enter__(self):
        self.directory = Path(tempfile.mkdtemp(prefix="alice-tui-test-"))
        (self.directory / "daemon.token").write_text("fixture-token")
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 36, 120, 0, 0))
        state["loaded"].clear()
        self.process = subprocess.Popen([binary], stdin=slave, stdout=slave, stderr=slave, env=dict(os.environ, LC_ALL="C", ALICE_DATA_DIR=str(self.directory), ALICE_PORT=str(server.server_port)))
        os.close(slave)
        self.output = bytearray()
        def drain():
            while self.process.poll() is None:
                try:
                    if select.select([self.master], [], [], .1)[0]:
                        self.output.extend(os.read(self.master, 65536))
                except OSError:
                    break
        threading.Thread(target=drain, daemon=True).start()
        assert state["loaded"].wait(3), "TUI did not load"
        time.sleep(.25)
        return self

    def key(self, value):
        os.write(self.master, value)

    def saved(self):
        for _ in range(150):
            if state["saved"]:
                time.sleep(.25)
                return state["saved"][-1]
            time.sleep(.02)
        raise AssertionError("Draft not saved")

    def __exit__(self, *args):
        if self.process.poll() is None:
            self.process.kill()
        self.process.wait()
        os.close(self.master)
        shutil.rmtree(self.directory)

try:
    with Terminal() as terminal:
        terminal.key(b"c")
        time.sleep(.2)
        terminal.key(b"person@example.invalid\tFixture\tHello\x1b[DX\x13")
        assert terminal.saved()["body"] == "HellXo"
        results["cursor_edit"] = "HellXo"
        terminal.key(b"q")
        terminal.process.wait(timeout=3)
    state["saved"] = []
    with Terminal() as terminal:
        terminal.key(b"r")
        time.sleep(.2)
        terminal.key(b"Reply\x13")
        assert terminal.saved()["to"] == ["support@example.invalid"]
        results["reply_to"] = "support@example.invalid"
        terminal.key(b"q")
        terminal.process.wait(timeout=3)
    state["delay"] = 4
    state["patch_event"].clear()
    with Terminal() as terminal:
        terminal.key(b"\r")
        assert state["patch_event"].wait(3)
        start = time.monotonic()
        terminal.key(b"q")
        terminal.process.wait(timeout=1)
        results["quit_during_request_seconds"] = round(time.monotonic()-start, 2)
    state["saved"] = []
    state["delay"] = 0
    with Terminal() as terminal:
        state["accounts"] = [dict(account, id="new-real", email="real@example.invalid")]
        state["messages"] = []
        state["loaded"].clear()
        assert state["loaded"].wait(3)
        time.sleep(.25)
        terminal.key(b"c")
        time.sleep(.2)
        assert b"real@example.invalid" in terminal.output
        terminal.key(b"person@example.invalid\tFixture\tNew account test\x13")
        assert terminal.saved()["account_id"] == "new-real"
        results["new_account_from"] = "new-real"
        terminal.key(b"q")
        terminal.process.wait(timeout=3)
    state["accounts"] = [account]
    state["messages"] = [dict(message, id=f"page-{i}") for i in range(125)]
    state["requests"] = []
    with Terminal() as terminal:
        terminal.key(b"]")
        for _ in range(100):
            if any("offset=50" in request for request in state["requests"]):
                break
            time.sleep(.02)
        assert any("offset=50" in request for request in state["requests"])
        time.sleep(.25)
        assert "51–100 of 125".encode() in terminal.output
        results["second_page"] = "51–100 of 125"
        terminal.key(b"q")
        terminal.process.wait(timeout=3)
    print(json.dumps(results, indent=2))
finally:
    server.shutdown()
    server.server_close()
