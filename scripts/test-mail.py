#!/usr/bin/env python3
"""Exercise Alice against isolated, certificate-validated Dovecot/Postfix servers.

Requires scripts/mail-fixture.sh in a disposable Linux container. Never targets
real accounts: all recipients and credentials belong to example.test.
"""
import argparse
import email
import imaplib
import json
import os
import socket
import ssl
import subprocess
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from email.message import EmailMessage
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--binary", default="target/debug/alice")
args = parser.parse_args()
binary = str(Path(args.binary).resolve())
address, password = "alice@example.test", "alice-fixture-password"
tls = ssl.create_default_context()
with socket.socket() as probe:
    probe.bind(("127.0.0.1", 0))
    port = probe.getsockname()[1]

with tempfile.TemporaryDirectory(prefix="alice-mail-") as directory:
    env = {**os.environ, "LC_ALL": "C", "ALICE_DATA_DIR": directory, "ALICE_PORT": str(port)}
    daemon = None

    def api(method, path, body=None):
        token = Path(directory, "daemon.token").read_text().strip()
        request = urllib.request.Request(
            f"http://127.0.0.1:{port}{path}", method=method,
            headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"},
            data=None if body is None else json.dumps(body).encode(),
        )
        try:
            with urllib.request.urlopen(request, timeout=90) as response:
                return json.load(response)
        except urllib.error.HTTPError as error:
            raise AssertionError(f"{method} {path}: {error.read().decode()}") from error

    def start():
        global daemon
        daemon = subprocess.Popen([binary, "daemon", "run"], env=env,
                                  stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(100):
            try:
                api("GET", "/v1/health")
                return
            except (OSError, urllib.error.URLError):
                time.sleep(0.1)
        raise AssertionError("Alice daemon did not become ready")

    def cli(*arguments):
        return subprocess.check_output([binary, *arguments], env=env, text=True, timeout=90)

    def sync():
        result = api("POST", "/v1/sync", {})
        assert result["errors"] == [], result

    def messages(folder):
        return json.loads(cli("list", "--folder", folder, "--account", account_id, "--limit", "200", "--json"))["messages"]

    def download(path):
        token = Path(directory, "daemon.token").read_text().strip()
        request = urllib.request.Request(f"http://127.0.0.1:{port}{path}", headers={"Authorization": f"Bearer {token}"})
        with urllib.request.urlopen(request, timeout=90) as response:
            return response.headers, response.read()

    def eventually(check, what, timeout=90):
        deadline = time.time() + timeout
        while time.time() < deadline:
            if check():
                return
            time.sleep(0.5)
        raise AssertionError(f"Timed out waiting for {what}")

    def storage():
        return next(a for a in api("GET", "/v1/storage")["accounts"] if a["account_id"] == account_id)

    def old(days):
        return imaplib.Time2Internaldate(time.time() - days * 86400)

    def flags(mailbox, identity):
        server.select(mailbox)
        status, found = server.uid("search", None, "HEADER", "Message-ID", f'"{identity}"')
        assert status == "OK" and found[0], (mailbox, identity, found)
        status, result = server.uid("fetch", found[0], "(FLAGS)")
        assert status == "OK"
        return b" ".join(item for item in result if isinstance(item, bytes))

    def delivered(identity):
        for _ in range(100):
            server.select("INBOX")
            status, found = server.uid("search", None, "HEADER", "Message-ID", f'"{identity}"')
            if status == "OK" and found[0]:
                assert len(found[0].split()) == 1, "SMTP retry duplicated delivery"
                status, fetched = server.uid("fetch", found[0], "(BODY.PEEK[])")
                assert status == "OK"
                return next(item[1] for item in fetched if isinstance(item, tuple))
            time.sleep(0.1)
        raise AssertionError("SMTP message never reached IMAP inbox")

    server = imaplib.IMAP4_SSL("localhost", 993, ssl_context=tls)
    server.login(address, password)
    marker = str(uuid.uuid4())
    original = EmailMessage()
    original["From"] = "Support <support@example.test>"
    original["To"] = address
    original["Reply-To"] = address
    original["Subject"] = f"Alice protocol test {marker}"
    original["Message-ID"] = f"<{marker}@example.test>"
    original.set_content("A café, a reply, and a safe mailbox move.")
    raw = original.as_bytes(policy=email.policy.SMTP)
    server.append("INBOX", None, None, raw)
    # Independent sent and received copies must both survive import.
    server.append("Gesendet", "(\\Seen)", None, raw)
    # Cross a body batch boundary with independently identifiable contents and flags.
    for index in range(12):
        item = EmailMessage()
        item["From"], item["To"] = address, address
        item["Subject"] = f"Alice batch {marker} {index}"
        item["Message-ID"] = f"<{marker}-{index}@example.test>"
        item.set_content(f"Batch body {index}: café.")
        server.append("INBOX", "(\\Seen)" if index % 2 else None, None,
                      item.as_bytes(policy=email.policy.SMTP))
    # An attachment Alice must fetch on demand: never downloaded during sync.
    payload = os.urandom(300_000)
    with_file = EmailMessage()
    with_file["From"], with_file["To"] = "Billing <billing@example.test>", address
    with_file["Subject"] = f"Alice invoice {marker}"
    with_file["Message-ID"] = f"<{marker}-invoice@example.test>"
    with_file.set_content("Your invoice is attached.")
    with_file.add_attachment(payload, maintype="application", subtype="pdf", filename="Rechnung €.pdf")
    server.append("INBOX", None, None, with_file.as_bytes(policy=email.policy.SMTP))
    # More archived mail than a first sync downloads at once, spread over the last 60 days.
    for index in range(60):
        item = EmailMessage()
        item["From"], item["To"] = address, address
        item["Subject"] = f"Alice backlog {marker} {index}"
        item["Message-ID"] = f"<{marker}-backlog-{index}@example.test>"
        item.set_content(f"Backlog {index}.")
        server.append("Archiv", "(\\Seen)", old(index), item.as_bytes(policy=email.policy.SMTP))
    # Older than the default 90-day window.
    ancient = EmailMessage()
    ancient["From"], ancient["To"] = address, address
    ancient["Subject"] = f"Alice ancient {marker}"
    ancient["Message-ID"] = f"<{marker}-ancient@example.test>"
    ancient.set_content("From two years ago.")
    server.append("INBOX", "(\\Seen)", old(730), ancient.as_bytes(policy=email.policy.SMTP))
    try:
        start()
        account_input = {"provider": "imap", "email": address, "name": "Protocol fixture",
                         "password": password, "imap_host": "localhost", "imap_port": 993,
                         "smtp_host": "localhost", "smtp_port": 587}
        account = api("POST", "/v1/accounts", account_input)["account"]
        account_id = account["id"]
        assert account["folders"]["sent"] == "Gesendet", account
        assert account["folders"]["archive"] == "Archiv", account
        assert account["folders"]["trash"] == "Papierkorb", account
        imported = {m["message_id"]: m for m in messages("inbox")}
        for index in range(12):
            item = imported[f"<{marker}-{index}@example.test>"]
            assert item["body"].strip() == f"Batch body {index}: café."
            assert item["is_read"] == bool(index % 2)
        assert ancient["Message-ID"] not in imported, "mail older than the sync window was downloaded"
        # The first sync brings the newest mail; the rest of the window follows in the background.
        eventually(lambda: storage()["sync"]["total"] > 0 and storage()["sync"]["downloaded"] == storage()["sync"]["total"]
                   and not storage()["sync"]["active"], "backfill of the sync window")
        archived_backlog = {m["message_id"] for m in messages("archive")}
        assert all(f"<{marker}-backlog-{index}@example.test>" in archived_backlog for index in range(60))
        usage = storage()
        assert usage["messages"] >= 75 and usage["bytes"] > 0 and any(f["folder"] == "archive" for f in usage["folders"])
        assert usage["bytes"] < len(payload), "an attachment was stored during sync"

        invoice = next(m for m in messages("inbox") if m["message_id"] == with_file["Message-ID"])
        assert [(a["name"], a["part"]) for a in invoice["attachments"]] == [("Rechnung €.pdf", "2")], invoice["attachments"]
        assert invoice["body"].strip() == "Your invoice is attached."
        headers, content = download(f'/v1/messages/{invoice["id"]}/attachments/0')
        assert content == payload, "attachment bytes differ from the server's"
        assert urllib.parse.unquote(headers["X-Alice-Filename"]) == "Rechnung €.pdf"
        inbox = next(m for m in messages("inbox") if m["message_id"] == original["Message-ID"])
        sent = [m for m in messages("sent") if m["message_id"] == original["Message-ID"]]
        assert len(sent) == 1, "Import dropped Sent copy of a message also in Inbox"
        assert sent[0]["id"] != inbox["id"]
        assert "café" in inbox["body"] and inbox["reply_to"] == [address]
        cli("read", inbox["id"], "--json")
        assert b"\\Seen" in flags("INBOX", original["Message-ID"])
        cli("unread", inbox["id"])
        cli("star", inbox["id"])
        assert b"\\Seen" not in flags("INBOX", original["Message-ID"])
        assert b"\\Flagged" in flags("INBOX", original["Message-ID"])
        cli("star", sent[0]["id"])
        assert b"\\Flagged" in flags("Gesendet", original["Message-ID"])
        cli("archive", inbox["id"])
        assert b"\\Flagged" in flags("Archiv", original["Message-ID"])
        sync()
        archived = next(m for m in messages("archive") if m["message_id"] == original["Message-ID"])
        assert archived["id"] == inbox["id"] and archived["remote_uid"] is not None
        server.select("Archiv")
        server.uid("store", str(archived["remote_uid"]), "-FLAGS.SILENT", "(\\Flagged)")
        sync()
        assert not api("GET", f'/v1/messages/{archived["id"]}')["is_starred"], \
            "Sent copy suppressed remote updates to the archived received copy"
        cli("trash", archived["id"])
        sync()
        assert any(m["id"] == inbox["id"] for m in messages("trash"))
        cli("restore", inbox["id"])
        sync()
        assert any(m["id"] == inbox["id"] for m in messages("inbox"))

        cli("reply", inbox["id"], "--body", "Alice reply test — café.")
        draft = next(m for m in messages("drafts") if m["in_reply_to"] == original["Message-ID"])
        assert draft["to"] == [address]
        cli("send", draft["id"])
        cli("send", draft["id"])
        received = delivered(draft["message_id"])
        parsed = email.message_from_bytes(received, policy=email.policy.default)
        assert parsed["To"] == address and parsed["In-Reply-To"] == original["Message-ID"]
        assert "Alice reply test — café." in parsed.get_content()
        # Model providers which also retain their own SMTP Sent copy.
        server.append("Gesendet", "(\\Seen)", None, received)
        sync()
        receipt = [m for m in messages("sent") if m["message_id"] == draft["message_id"]]
        assert len(receipt) == 1 and receipt[0]["id"] == draft["id"]
        assert receipt[0]["remote_uid"] is not None, "Sent receipt never acquired provider identity"
        assert any(m["message_id"] == draft["message_id"] for m in messages("inbox"))
        cli("star", receipt[0]["id"])
        assert b"\\Flagged" in flags("Gesendet", draft["message_id"])

        # Repeat delivery with implicit TLS on the standard SMTPS port.
        account_input["smtp_port"] = 465
        assert api("POST", "/v1/accounts", account_input)["account"]["id"] == account_id
        cli("compose", "--account", account_id, "--to", address,
            "--subject", f"Alice SMTPS test {marker}", "--body", "Port 465 delivery.", "--send")
        implicit = next(m for m in messages("sent") if m["subject"] == f"Alice SMTPS test {marker}")
        assert "Port 465 delivery." in email.message_from_bytes(
            delivered(implicit["message_id"]), policy=email.policy.default).get_content()

        # External deletion must disappear from the shared cache on the next sync.
        server.select("INBOX")
        _, found = server.uid("search", None, "HEADER", "Message-ID", f'"{original["Message-ID"]}"')
        server.uid("store", found[0], "+FLAGS.SILENT", "(\\Deleted)")
        server.expunge()
        sync()
        assert all(m["id"] != inbox["id"] for m in messages("inbox"))
        assert any(m["id"] == sent[0]["id"] for m in messages("sent"))
        api("POST", "/v1/shutdown", {})
        daemon.wait(timeout=10)
        start()
        sync()  # Reload credentials from a real Secret Service after restart.
        assert any(m["id"] == draft["id"] for m in messages("sent"))

        # Widening the window brings older mail; narrowing it removes it from this device only.
        api("PATCH", "/v1/preferences", {"sync_window": "all"})
        eventually(lambda: any(m["message_id"] == ancient["Message-ID"] for m in messages("inbox")), "older mail after widening the window")
        api("PATCH", "/v1/preferences", {"sync_window": "30_days"})
        eventually(lambda: not any(m["message_id"] == ancient["Message-ID"] for m in messages("inbox"))
                   and f"<{marker}-backlog-45@example.test>" not in {m["message_id"] for m in messages("archive")}, "pruning after narrowing the window")
        assert f"<{marker}-backlog-5@example.test>" in {m["message_id"] for m in messages("archive")}
        server.select("INBOX")
        _, found = server.uid("search", None, "HEADER", "Message-ID", f'"{ancient["Message-ID"]}"')
        assert found[0], "narrowing the window deleted mail on the server"
        print("Mail E2E passed: TLS IMAP, special-use folders, distinct Inbox/Sent copies, "
              "remote flags, MOVE/UID reconciliation, Reply-To, SMTP STARTTLS and SMTPS, "
              "single delivery, Sent receipt merge, external deletion, credential restart, "
              "background backfill, on-demand attachments, and the sync window.")
    finally:
        if daemon is not None and daemon.poll() is None:
            try:
                api("POST", "/v1/shutdown", {})
                daemon.wait(timeout=10)
            except (OSError, subprocess.TimeoutExpired):
                daemon.kill()
                daemon.wait()
        server.logout()
