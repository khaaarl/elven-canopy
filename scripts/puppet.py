#!/usr/bin/env python3
"""Puppet CLI — remote control for Elven Canopy game instances.

Launches headless game instances under xvfb-run, communicates via a
JSON-over-TCP RPC protocol (4-byte big-endian length prefix + payload),
and manages session lifecycle.

Stdlib only — no external dependencies.

Usage:
    scripts/puppet.py launch                    # start game, default session "a"
    scripts/puppet.py launch -g b               # second session as "b"
    scripts/puppet.py game-state                # query session "a"
    scripts/puppet.py -g b game-state           # query session "b"
    scripts/puppet.py press-key B               # send key press
    scripts/puppet.py kill                      # kill session "a"
    scripts/puppet.py kill --all                # kill ALL sessions
    scripts/puppet.py list                      # show running sessions
"""

import argparse
import json
import os
import signal
import socket
import struct
import subprocess
import sys
import time

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TMP_DIR = os.path.join(REPO_ROOT, ".tmp")
GODOT_PROJECT = os.path.join(REPO_ROOT, "godot")

# Port range for puppet sessions (pick from this range).
PORT_BASE = 19200
PORT_RANGE = 100

MAX_MESSAGE_SIZE = 1_048_576  # 1 MB


# ---------------------------------------------------------------------------
# Session file management
# ---------------------------------------------------------------------------


def session_path(game_id: str) -> str:
    return os.path.join(TMP_DIR, f"puppet-{game_id}.json")


def read_session(game_id: str) -> dict | None:
    path = session_path(game_id)
    if not os.path.exists(path):
        return None
    with open(path) as f:
        return json.load(f)


def write_session(game_id: str, data: dict) -> None:
    os.makedirs(TMP_DIR, exist_ok=True)
    path = session_path(game_id)
    with open(path, "w") as f:
        json.dump(data, f, indent=2)
        f.write("\n")


def remove_session(game_id: str) -> None:
    path = session_path(game_id)
    if os.path.exists(path):
        os.remove(path)


def list_sessions() -> dict[str, dict]:
    """Return {game_id: session_data} for all session files."""
    result = {}
    if not os.path.isdir(TMP_DIR):
        return result
    for fname in os.listdir(TMP_DIR):
        if fname.startswith("puppet-") and fname.endswith(".json"):
            game_id = fname[len("puppet-"):-len(".json")]
            try:
                with open(os.path.join(TMP_DIR, fname)) as f:
                    result[game_id] = json.load(f)
            except (json.JSONDecodeError, OSError):
                pass
    return result


def is_pid_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
        return True
    except OSError:
        return False


# ---------------------------------------------------------------------------
# TCP protocol
# ---------------------------------------------------------------------------


def send_message(sock: socket.socket, data: dict) -> None:
    payload = json.dumps(data).encode("utf-8")
    header = struct.pack(">I", len(payload))
    sock.sendall(header + payload)


def recv_message(sock: socket.socket, timeout: float = 30.0) -> dict:
    sock.settimeout(timeout)
    # Read 4-byte length prefix.
    header = _recv_exact(sock, 4)
    msg_len = struct.unpack(">I", header)[0]
    if msg_len > MAX_MESSAGE_SIZE:
        raise RuntimeError(f"message too large: {msg_len} bytes")
    payload = _recv_exact(sock, msg_len)
    return json.loads(payload.decode("utf-8"))


def _recv_exact(sock: socket.socket, n: int) -> bytes:
    buf = bytearray()
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise ConnectionError("connection closed")
        buf.extend(chunk)
    return bytes(buf)


def rpc_call(port: int, method: str, args: list | None = None,
             timeout: float = 30.0) -> dict:
    """Send an RPC and return the response dict."""
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.settimeout(timeout)
    try:
        sock.connect(("127.0.0.1", port))
        request = {"method": method}
        if args:
            request["args"] = args
        send_message(sock, request)
        return recv_message(sock, timeout)
    finally:
        sock.close()


# ---------------------------------------------------------------------------
# Godot binary discovery (mirrors build.sh logic)
# ---------------------------------------------------------------------------


def find_godot() -> str:
    for cmd in ("godot-4", "godot4", "godot"):
        for d in os.environ.get("PATH", "").split(os.pathsep):
            full = os.path.join(d, cmd)
            if os.path.isfile(full) and os.access(full, os.X_OK):
                return full
    print("ERROR: Godot binary not found (tried godot-4, godot4, godot)",
          file=sys.stderr)
    sys.exit(1)


# ---------------------------------------------------------------------------
# Commands
# ---------------------------------------------------------------------------


def pick_free_port() -> int:
    """Find an unused port in our range."""
    used_ports = set()
    for sess in list_sessions().values():
        used_ports.add(sess.get("port", 0))
    for port in range(PORT_BASE, PORT_BASE + PORT_RANGE):
        if port in used_ports:
            continue
        # Check if port is actually free.
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            try:
                s.bind(("127.0.0.1", port))
                return port
            except OSError:
                continue
    print("ERROR: no free port in range %d-%d" % (PORT_BASE, PORT_BASE + PORT_RANGE),
          file=sys.stderr)
    sys.exit(1)


def cmd_launch(args: argparse.Namespace) -> None:
    game_id = args.game

    # Check for existing session.
    existing = read_session(game_id)
    if existing and is_pid_alive(existing.get("pid", 0)):
        print(f"Session '{game_id}' already running (PID {existing['pid']}, "
              f"port {existing['port']})")
        return

    # Clean up stale session file.
    remove_session(game_id)

    port = pick_free_port()
    godot = find_godot()

    env = os.environ.copy()
    env["PUPPET_SERVER"] = str(port)
    # Short timeout for testing — 5 minutes.
    env.setdefault("PUPPET_TIMEOUT_SECS", "300")

    if args.visible:
        cmd = [godot, "--path", GODOT_PROJECT]
    else:
        cmd = ["xvfb-run", "-a", godot, "--path", GODOT_PROJECT, "--headless"]

    # Launch in background, logging to .tmp/.
    log_path = os.path.join(TMP_DIR, f"puppet-{game_id}.log")
    log_file = open(log_path, "w")  # noqa: SIM115 — can't use with-block, need fd open for Popen
    proc = subprocess.Popen(
        cmd,
        env=env,
        stdout=log_file,
        stderr=subprocess.STDOUT,
        start_new_session=True,
    )
    log_file.close()  # Child inherited the fd; parent no longer needs it.

    write_session(game_id, {
        "port": port,
        "pid": proc.pid,
        "started": time.time(),
        "godot": godot,
        "log": log_path,
    })

    print(f"Launching session '{game_id}' (PID {proc.pid}, port {port}, log {log_path})...")

    # Poll until the server responds.
    deadline = time.time() + 60
    while time.time() < deadline:
        # Check if process died.
        if proc.poll() is not None:
            remove_session(game_id)
            print(f"ERROR: Godot process exited with code {proc.returncode}",
                  file=sys.stderr)
            sys.exit(1)
        try:
            resp = rpc_call(port, "ping", timeout=2.0)
            if resp.get("ok"):
                print(f"Session '{game_id}' ready (port {port})")
                return
        except (ConnectionError, ConnectionRefusedError, OSError, TimeoutError):
            time.sleep(0.5)

    # Timeout — kill the process.
    print("ERROR: timed out waiting for puppet server to respond",
          file=sys.stderr)
    _kill_process(proc.pid)
    remove_session(game_id)
    sys.exit(1)


def cmd_kill(args: argparse.Namespace) -> None:
    if args.all:
        sessions = list_sessions()
        if not sessions:
            print("No puppet sessions found.")
            return
        for gid in sorted(sessions):
            _kill_session(gid, sessions[gid])
        return

    game_id = args.game
    sess = read_session(game_id)
    if not sess:
        print(f"No session '{game_id}' found.")
        return
    _kill_session(game_id, sess)


def _kill_session(game_id: str, sess: dict) -> None:
    pid = sess.get("pid", 0)
    port = sess.get("port", 0)

    if not is_pid_alive(pid):
        print(f"Session '{game_id}' (PID {pid}): already dead, cleaning up")
        remove_session(game_id)
        return

    # Try graceful quit via RPC first.
    if port:
        try:
            rpc_call(port, "quit", timeout=3.0)
            # Wait for process to exit.
            for _ in range(20):
                if not is_pid_alive(pid):
                    print(f"Session '{game_id}' (PID {pid}): quit gracefully")
                    remove_session(game_id)
                    return
                time.sleep(0.25)
        except (ConnectionError, ConnectionRefusedError, OSError, TimeoutError):
            pass

    # Graceful quit failed — SIGTERM.
    _kill_process(pid)
    remove_session(game_id)
    print(f"Session '{game_id}' (PID {pid}): killed")


def _kill_process(pid: int) -> None:
    """Send SIGTERM, wait briefly, then SIGKILL if needed."""
    if not is_pid_alive(pid):
        return
    try:
        # Kill the process group (since we used start_new_session=True).
        os.killpg(pid, signal.SIGTERM)
    except OSError:
        try:
            os.kill(pid, signal.SIGTERM)
        except OSError:
            return
    # Wait up to 5 seconds for it to die.
    for _ in range(50):
        if not is_pid_alive(pid):
            return
        time.sleep(0.1)
    # Still alive — SIGKILL.
    try:
        os.killpg(pid, signal.SIGKILL)
    except OSError:
        try:
            os.kill(pid, signal.SIGKILL)
        except OSError:
            pass


def cmd_list(args: argparse.Namespace) -> None:
    sessions = list_sessions()
    if not sessions:
        print("No puppet sessions.")
        return
    for gid in sorted(sessions):
        sess = sessions[gid]
        pid = sess.get("pid", 0)
        port = sess.get("port", 0)
        alive = is_pid_alive(pid)
        status = "running" if alive else "DEAD"
        started = sess.get("started", 0)
        age = ""
        if started:
            elapsed = time.time() - started
            age = f", age {int(elapsed)}s"
        print(f"  {gid}: PID {pid}, port {port}, {status}{age}")
        if not alive:
            remove_session(gid)


def cmd_rpc(args: argparse.Namespace) -> None:
    """Generic RPC command handler."""
    game_id = args.game
    sess = read_session(game_id)
    if not sess:
        print(f"ERROR: no session '{game_id}' (use 'launch' first)",
              file=sys.stderr)
        sys.exit(1)

    port = sess["port"]
    pid = sess.get("pid", 0)
    if not is_pid_alive(pid):
        print(f"ERROR: session '{game_id}' process (PID {pid}) is dead",
              file=sys.stderr)
        remove_session(game_id)
        sys.exit(1)

    method = args.rpc_method
    rpc_args = args.rpc_args if hasattr(args, "rpc_args") and args.rpc_args else []

    try:
        resp = rpc_call(port, method, rpc_args if rpc_args else None)
    except (ConnectionError, ConnectionRefusedError, OSError) as e:
        print(f"ERROR: connection failed: {e}", file=sys.stderr)
        sys.exit(1)

    if "error" in resp:
        print(f"ERROR: {resp['error']}", file=sys.stderr)
        sys.exit(1)

    result = resp.get("result")
    if isinstance(result, (dict, list)):
        print(json.dumps(result, indent=2))
    elif isinstance(result, bool):
        print("OK" if result else "false")
    else:
        print(result)


# ---------------------------------------------------------------------------
# Argument parsing
# ---------------------------------------------------------------------------


# RPC methods and their expected positional args for help text.
RPC_METHODS = {
    "game-state": [],
    "list-panels": [],
    "is-panel-visible": ["panel_name"],
    "read-panel-text": ["node_name"],
    "find-text": ["panel_name", "substring"],
    "collect-text": ["panel_name"],
    "tree-info": [],
    "list-structures": [],
    "click-at-world-pos": ["x,y,z"],
    "press-key": ["key_name"],
    "press-button": ["button_text"],
    "press-button-near": ["label_text", "button_text"],
    "step-ticks": ["count"],
    "set-sim-speed": ["speed"],
    "move-camera-to": ["x,y,z"],
    "quit": [],
    "ping": [],
}


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="puppet.py",
        description="Remote control for Elven Canopy game instances.",
    )
    parser.add_argument(
        "-g", "--game", default="a",
        help="Game session ID (default: 'a')",
    )

    sub = parser.add_subparsers(dest="command")

    # launch
    launch_p = sub.add_parser("launch", help="Launch a new game instance")
    launch_p.add_argument("--visible", action="store_true",
                           help="Run with a visible window (no xvfb/headless)")

    # kill
    kill_p = sub.add_parser("kill", help="Kill a game instance")
    kill_p.add_argument("--all", action="store_true",
                        help="Kill ALL sessions")

    # list
    sub.add_parser("list", help="List running sessions")

    # RPC methods as subcommands
    for method, expected_args in RPC_METHODS.items():
        p = sub.add_parser(method, help=f"RPC: {method}")
        p.add_argument("rpc_args", nargs="*", metavar="ARG",
                        help=" ".join(expected_args) if expected_args else argparse.SUPPRESS)

    return parser


def main() -> None:
    parser = build_parser()
    args = parser.parse_args()

    if not args.command:
        parser.print_help()
        sys.exit(1)

    if args.command == "launch":
        cmd_launch(args)
    elif args.command == "kill":
        cmd_kill(args)
    elif args.command == "list":
        cmd_list(args)
    elif args.command in RPC_METHODS:
        args.rpc_method = args.command
        cmd_rpc(args)
    else:
        parser.print_help()
        sys.exit(1)


if __name__ == "__main__":
    main()
