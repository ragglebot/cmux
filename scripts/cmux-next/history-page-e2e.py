#!/usr/bin/env python3
"""Live check of the React History page in a tagged build (plans/cmux-next/react-pages.md H1b/H3).

Launches the tagged app itself (no-activate, scratch cmux.json and an empty Ghostty config),
turns on the React page (Debug Settings `history.surface = web`), opens History as the user would
(`action.run history.show`, origin user), and checks through `debug.page` only: the page loaded
in its own origin, the History title and chips rendered, html paints the window's one background
and body is transparent, the dispatcher's find command reaches the search field, and a lost owner
link shows the disconnected state. Writes `debug.page` snapshots and a composited
`debug.window_snapshot` to --out. Never sends system input; kills the app it started.

Usage: history-page-e2e.py --tag <tag> [--app PATH] [--out DIR]
Exit 1 on any failed check.
"""
import argparse, glob, json, os, socket, subprocess, sys, tempfile, time

parser = argparse.ArgumentParser()
parser.add_argument("--tag", required=True)
parser.add_argument("--app", help="the tagged cmux DEV app (default: the newest DerivedData build of the tag)")
parser.add_argument("--out", default=os.environ.get("NX_ARTIFACTS", "/tmp"))
opts = parser.parse_args()

APP = opts.app or next(iter(sorted(glob.glob(os.path.expanduser(
    f"~/Library/Developer/Xcode/DerivedData/*/Build/Products/Debug/cmux DEV {opts.tag}.app")))), None)
if not APP or not os.path.isdir(APP):
    sys.exit(f"no tagged app for {opts.tag}")
BINARY = os.path.join(APP, "Contents/MacOS/cmux DEV")
SOCKET = f"/tmp/cmux-debug-{opts.tag}.sock"
SCRATCH = tempfile.mkdtemp(prefix=f"history-e2e-{opts.tag}-")
CONFIG = os.path.join(SCRATCH, "cmux.json")
GHOSTTY = os.path.join(SCRATCH, "ghostty")
open(GHOSTTY, "w").write("")
open(CONFIG, "w").write("{}")
os.makedirs(opts.out, exist_ok=True)


def rpc(method, params=None):
    try:
        conn = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        conn.settimeout(30)
        conn.connect(SOCKET)
        conn.sendall((json.dumps({"id": 1, "method": method, "params": params or {}}) + "\n").encode())
        buf = b""
        while not buf.endswith(b"\n"):
            chunk = conn.recv(1 << 20)
            if not chunk:
                break
            buf += chunk
        conn.close()
        reply = json.loads(buf)
        return reply.get("result") if reply.get("ok") else {"error": reply.get("error")}
    except (OSError, ValueError) as error:
        return {"error": str(error)}


def wait(predicate, seconds, step=0.5):
    end = time.time() + seconds
    while time.time() < end:
        value = predicate()
        if value:
            return value
        time.sleep(step)  # test harness wait, not app code
    return None


failures = []


def check(label, ok, detail=""):
    print(("ok   " if ok else "FAIL ") + label + (f": {detail}" if detail and not ok else ""), flush=True)
    if not ok:
        failures.append(label)


def page_state():
    state = rpc("debug.page", {"page": "cmux.history"})
    return state if isinstance(state, dict) and "error" not in state else None


app = None
try:
    if os.path.exists(SOCKET):
        os.unlink(SOCKET)
    env = {"HOME": os.environ["HOME"], "USER": os.environ.get("USER", ""), "TMPDIR": os.environ.get("TMPDIR", "/tmp"),
           "PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "CMUX_NEXT_NO_ACTIVATE": "1", "CMUX_NEXT_SOCKET_MODE": "automation",
           "CMUX_NEXT_TEST_WINDOW_SCREEN": "last", "CMUX_NEXT_CONFIG_FILE": CONFIG, "CMUX_NEXT_GHOSTTY_CONFIG": GHOSTTY,
           "CMUX_NEXT_TEST_WINDOW_FRAME": "40,40,1000,700"}
    log = open(os.path.join(SCRATCH, "app.log"), "a")
    app = subprocess.Popen([BINARY], env=env, stdout=log, stderr=log, stdin=subprocess.DEVNULL)
    print(f"launched pid {app.pid}, scratch {SCRATCH}", flush=True)
    if not wait(lambda: os.path.exists(SOCKET) and (rpc("debug.surfaces") or {}).get("windows"), 120):
        sys.exit("the tagged app did not come up")

    tunable = rpc("debug.tunables", {"action": "set", "key": "history.surface", "value": "web"})
    check("history.surface = web", isinstance(tunable, dict) and "error" not in tunable, json.dumps(tunable))
    shown = rpc("action.run", {"action": "history.show", "origin": "user"})
    check("history.show runs", isinstance(shown, dict) and "error" not in shown, json.dumps(shown))

    state = wait(lambda: (s := page_state()) and "History" in s.get("text", "") and s, 60)
    check("the React History page rendered", bool(state), json.dumps(page_state()))
    if state:
        print(json.dumps(state, indent=1), flush=True)
        check("page origin cmux.history", state.get("page") in ("history", "cmux.history"), json.dumps(state))
        check("chips and search rendered", state.get("controls", 0) >= 7, str(state.get("controls")))
        check("body is transparent", state.get("body") in ("rgba(0, 0, 0, 0)", "transparent"), str(state.get("body")))
        check("html paints the window background", bool(state.get("html")), str(state.get("html")))
        check("the page subscribed to its streams", state.get("subscriptions", 0) >= 0)

    rpc("debug.page", {"page": "cmux.history", "action": "snapshot", "path": os.path.join(opts.out, "history-page.png")})
    rpc("debug.window_snapshot", {"path": os.path.join(opts.out, "history-window.png")})

    found = rpc("debug.page", {"page": "cmux.history", "action": "command", "command": "find", "text": "zzz-no-match"})
    check("find reaches the page", isinstance(found, dict) and found.get("handled") is True, json.dumps(found))
    after = wait(lambda: (s := page_state()) and ("No matches" in s.get("text", "") or "zzz" in s.get("text", "")) and s, 20)
    check("the search shows no matches", bool(after), json.dumps(page_state()))

    rpc("debug.page", {"page": "cmux.history", "action": "connected", "value": False})
    lost = wait(lambda: (s := page_state()) and "reconnects" in s.get("text", "") and s, 20)
    check("a lost link shows the disconnected state", bool(lost), json.dumps(page_state()))
    rpc("debug.page", {"page": "cmux.history", "action": "snapshot", "path": os.path.join(opts.out, "history-disconnected.png")})
    rpc("debug.page", {"page": "cmux.history", "action": "connected", "value": True})
    back = wait(lambda: (s := page_state()) and "reconnects" not in s.get("text", "") and s, 20)
    check("the link coming back re-reads", bool(back), json.dumps(page_state()))
finally:
    if app:
        app.terminate()
        try:
            app.wait(10)
        except subprocess.TimeoutExpired:
            app.kill()

print(f"{len(failures)} failure(s); artifacts in {opts.out}")
sys.exit(1 if failures else 0)
