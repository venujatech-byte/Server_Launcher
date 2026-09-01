#!/usr/bin/env python3
"""Server Launcher.

A Tkinter control panel for dev services: each server can be switched on
and off on its own, with live logs and status lights.

Start with no pre-configured servers - add your own with the "+ Add Server"
button:

  * a name and working directory
  * a start command
  * an optional stop command (runs before stopping, e.g. export data)
  * a port (for status detection)
  * environment variables
  * extra action buttons (sub-commands that run in that folder)
  * open-in-browser links

Custom servers persist to servers.json next to this script, and can be
edited or deleted at any time from their card.

The "Server Overview" tab lists every process currently listening on this
PC (netstat), so you can see and stop servers that the launcher did not
start.

Standard library only - no pip installs needed.
"""

from __future__ import annotations

import json
import os
import queue
import re
import shlex
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import webbrowser
from pathlib import Path

import tkinter as tk
from tkinter import ttk, messagebox, filedialog

# --------------------------------------------------------------------------
# Paths
# --------------------------------------------------------------------------

SCRIPT_DIR = Path(__file__).resolve().parent
SERVERS_JSON = SCRIPT_DIR / "servers.json"


def _tool(*names: str) -> str:
    for name in names:
        found = shutil.which(name)
        if found:
            return found
    return names[0]


def _find_venv_python(cwd) -> str:
    """Return a venv python.exe under cwd, or '' if none found."""
    cwd = Path(cwd)
    if cwd.is_dir():
        for folder in (".venv", "venv", "env", ".virtualenv"):
            cand = cwd / folder / "Scripts" / "python.exe"
            if cand.is_file():
                return str(cand)
    return ""


def split_command(command: str) -> list:
    """Split a command string into args, respecting double-quoted tokens.

    Unlike str.split(), this keeps "python -c \"... code ...\"" intact so
    arguments containing spaces work. Windows backslash paths (C:\\Users)
    are preserved, and a matched pair of surrounding double quotes is
    stripped from a token so quoted arguments are passed cleanly.
    """
    try:
        parts = shlex.split(command, posix=False)
    except ValueError:
        parts = shlex.split(command, posix=True)
    out = []
    for tok in parts:
        if len(tok) >= 2 and tok.startswith('"') and tok.endswith('"'):
            tok = tok[1:-1]
        out.append(tok)
    return out


def resolve_command(args: list, cwd=None) -> list:
    """Return args with the executable resolved to a full path.

    On Windows, bare command names like "npm" need to become the full path
    to npm.CMD before CreateProcess will run them without an extra shell.
    shutil.which handles this (including PATHEXT), so "npm run dev" can be
    typed normally instead of "npm.cmd run dev".

    If the executable is python and a virtual environment exists under
    `cwd`, the venv's python is preferred automatically.
    """
    args = list(args)
    if not args:
        return args
    exe = str(args[0])
    # if it's already an absolute/relative path to an existing file, keep it
    try:
        p = Path(exe)
        if p.is_file() or ("/" in exe or "\\" in exe):
            return args
    except OSError:
        pass
    base = os.path.basename(exe).lower()
    if base in ("python", "python.exe", "python3", "python3.exe", "py"):
        venv = _find_venv_python(cwd) if cwd else ""
        if venv:
            args[0] = venv
            return args
    resolved = shutil.which(exe)
    if resolved:
        args[0] = resolved
    return args


# --------------------------------------------------------------------------
# Custom servers persistence
# --------------------------------------------------------------------------

def load_custom_servers() -> list[dict]:
    if not SERVERS_JSON.exists():
        return []
    try:
        data = json.loads(SERVERS_JSON.read_text("utf-8"))
        servers = data.get("servers", []) if isinstance(data, dict) else data
        return servers if isinstance(servers, list) else []
    except (json.JSONDecodeError, OSError):
        return []


def save_custom_servers(servers: list[dict]) -> None:
    SERVERS_JSON.write_text(
        json.dumps({"servers": servers}, indent=2, ensure_ascii=False),
        encoding="utf-8",
    )


# --------------------------------------------------------------------------
# Look and feel
# --------------------------------------------------------------------------

BG = "#0f1117"
CARD_BG = "#1a1d27"
CARD_BORDER = "#2a2e3a"
TEXT = "#e4e7ee"
MUTED = "#7b8394"
ACCENT = "#6c5ce7"
ACCENT_HOVER = "#7f70f0"
SUCCESS = "#00b894"
DANGER = "#e17055"
WARNING = "#fdcb6e"
LOG_BG = "#111318"
LOG_FG = "#c8cdd8"

DOT = {
    "stopped": "#555d6e",
    "starting": WARNING,
    "running": SUCCESS,
    "stopping": WARNING,
    "error": DANGER,
}
LABEL = {
    "stopped": "Stopped",
    "starting": "Starting...",
    "running": "Running",
    "stopping": "Stopping...",
    "error": "Exited",
}

ANSI = re.compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07]*\x07")

CREATE_NO_WINDOW = 0x08000000
CREATE_NEW_CONSOLE = 0x00000010

EH = "#252830"       # entry bg
EH_HOVER = "#2f3340" # button hover


# --------------------------------------------------------------------------
# Helpers
# --------------------------------------------------------------------------

def port_open(port: int, host: str = "127.0.0.1", timeout: float = 0.35) -> bool:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.settimeout(timeout)
        return sock.connect_ex((host, port)) == 0


def lan_ip() -> str:
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            sock.connect(("8.8.8.8", 80))
            return sock.getsockname()[0]
    except OSError:
        return "127.0.0.1"


# --------------------------------------------------------------------------
# Local server scanning (netstat-based)
# --------------------------------------------------------------------------

NETSTAT_RE = re.compile(
    r"\s*(?P<proto>TCP|UDP)\s+"
    r"(?P<local>[0-9.:\[\]]+)\s+"
    r"(?P<remote>[0-9.:\[\]]+)\s+"
    r"(?P<state>[A-Z_]+)?\s*"
    r"(?P<pid>\d+)"
)

_pid_name_cache = {}


def _run_netstat() -> str:
    try:
        done = subprocess.run(["netstat", "-ano"], capture_output=True,
                              creationflags=CREATE_NO_WINDOW, timeout=20,
                              text=True)
        return done.stdout or ""
    except (OSError, subprocess.SubprocessError):
        return ""


def _tasklist_names() -> dict:
    if _pid_name_cache:
        return _pid_name_cache
    names = {}
    try:
        done = subprocess.run(["tasklist", "/FO", "CSV", "/NH"],
                              capture_output=True, creationflags=CREATE_NO_WINDOW,
                              timeout=30, text=True)
        for line in (done.stdout or "").splitlines():
            try:
                parts = next(iter(__import__("csv").reader([line])))
                if len(parts) >= 2 and parts[1].strip().isdigit():
                    names[int(parts[1].strip())] = parts[0].strip("\"")
            except Exception:
                continue
    except (OSError, subprocess.SubprocessError):
        pass
    _pid_name_cache.update(names)
    return names


def scan_listening_servers() -> list[dict]:
    """Return listening TCP ports on this PC with pid and process name.

    Each entry: {proto, port, pid, name}
    """
    out = _run_netstat()
    names = _tasklist_names()
    found = {}
    for line in out.splitlines():
        m = NETSTAT_RE.match(line)
        if not m:
            continue
        proto = m.group("proto")
        if proto != "TCP":
            continue
        local = m.group("local")
        state = (m.group("state") or "").upper()
        if state != "LISTENING":
            continue
        try:
            port = int(local.rsplit(":", 1)[1])
            pid = int(m.group("pid"))
        except ValueError:
            continue
        key = (pid, port)
        if key not in found:
            found[key] = {"proto": proto, "port": port, "pid": pid,
                          "name": names.get(pid, "pid %d" % pid)}
    return sorted(found.values(), key=lambda d: d["port"])


def taskkill(pid: int) -> None:
    subprocess.run(["taskkill", "/PID", str(pid), "/T", "/F"],
                   capture_output=True, creationflags=CREATE_NO_WINDOW)


# --------------------------------------------------------------------------
# Service
# --------------------------------------------------------------------------

class Service:
    def __init__(self, app, key, name, subtitle, args, cwd, port,
                 env=None, links=(), external=False, missing=None,
                 actions=(), custom=False, custom_actions=(), stop_command=""):
        self.app = app
        self.key = key
        self.name = name
        self.subtitle = subtitle
        self.args = list(args)
        self.cwd = Path(cwd)
        self.port = port
        self.env_extra = dict(env or {})
        self.links = list(links)
        self.external_default = external
        self.missing = missing
        self.actions = list(actions)
        self.custom = custom
        self.custom_actions = list(custom_actions)  # (label, command)
        self.stop_command = stop_command            # runs before killing

        self.proc = None
        self.state = "stopped"
        self.stopping = False
        self.external_var = None

    @property
    def alive(self) -> bool:
        return self.proc is not None and self.proc.poll() is None

    def set_state(self, state: str) -> None:
        self.state = state
        self.app.post(("state", self.key))

    def log(self, line: str) -> None:
        self.app.post(("log", self.key, line))

    def run_sub_command(self, label: str, command: str) -> None:
        """Run a custom sub-command in this service's working directory."""
        self.log("[launcher] running: " + label)
        self.log("[launcher] > " + command)
        args = resolve_command(split_command(command), self.cwd)
        env = os.environ.copy()
        env.update(self.env_extra)
        try:
            proc = subprocess.Popen(
                args, cwd=str(self.cwd), env=env,
                stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                stdin=subprocess.DEVNULL, creationflags=CREATE_NO_WINDOW,
            )
            threading.Thread(target=self._pump_foreign, args=(proc,),
                             daemon=True).start()
        except OSError as exc:
            self.log("[launcher] could not run: " + str(exc))

    def _pump_foreign(self, proc) -> None:
        code = stream(proc, self.log)
        self.log("[launcher] sub-command exited with code %d" % code)

    def start(self) -> None:
        if self.alive:
            return
        if self.missing:
            self.log("[launcher] cannot start: " + self.missing)
            self.set_state("error")
            return
        if not self.cwd.exists():
            self.log("[launcher] folder not found: " + str(self.cwd))
            self.set_state("error")
            return
        if self.port and port_open(self.port):
            self.log("[launcher] warning: port %d is already in use" % self.port)

        external = bool(self.external_var and self.external_var.get())
        env = os.environ.copy()
        env.update(self.env_extra)

        self.stopping = False
        self._started_at = None
        self.set_state("starting")
        self.log("[launcher] " + str(self.cwd))
        self.log("[launcher] > " + subprocess.list2cmdline(self.args))

        run_args = resolve_command(self.args, self.cwd)

        try:
            if external:
                script = write_batch(self.key, "Server - " + self.name,
                                     self.cwd, self.env_extra, run_args)
                self.proc = subprocess.Popen(
                    ["cmd", "/k", script],
                    cwd=str(self.cwd),
                    env=env,
                    creationflags=CREATE_NEW_CONSOLE,
                )
                self.log("[launcher] running in its own console window")
                threading.Thread(target=self._wait_only, daemon=True).start()
            else:
                self.proc = subprocess.Popen(
                    run_args,
                    cwd=str(self.cwd),
                    env=env,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.STDOUT,
                    stdin=subprocess.DEVNULL,
                    creationflags=CREATE_NO_WINDOW,
                )
                threading.Thread(target=self._pump, daemon=True).start()
        except OSError as exc:
            self.proc = None
            self.log("[launcher] failed to start: " + str(exc))
            self.set_state("error")

    def stop(self) -> None:
        if not self.alive:
            self.proc = None
            self.stopping = False
            self.set_state("stopped")
            return
        self.stopping = True
        self.set_state("stopping")
        if self.stop_command and self.stop_command.strip():
            threading.Thread(target=self._stop_worker, daemon=True).start()
        else:
            self._terminate()

    def _stop_worker(self) -> None:
        """Run the configured stop command, then kill the process tree."""
        self.log("[launcher] running stop command: " + self.stop_command)
        args = resolve_command(split_command(self.stop_command), self.cwd)
        env = os.environ.copy()
        env.update(self.env_extra)
        try:
            proc = subprocess.Popen(
                args, cwd=str(self.cwd), env=env,
                stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                stdin=subprocess.DEVNULL, creationflags=CREATE_NO_WINDOW,
            )
            code = stream(proc, self.log)
            self.log("[launcher] stop command exited with code %d" % code)
        except OSError as exc:
            self.log("[launcher] stop command could not run: " + str(exc))
        finally:
            self._terminate()

    def _terminate(self) -> None:
        if not self.alive:
            return
        pid = self.proc.pid
        self.log("[launcher] stopping pid %d and its child processes..." % pid)
        threading.Thread(target=self._kill_tree, args=(pid,), daemon=True).start()

    def _kill_tree(self, pid: int) -> None:
        subprocess.run(
            ["taskkill", "/PID", str(pid), "/T", "/F"],
            capture_output=True,
            creationflags=CREATE_NO_WINDOW,
        )

    def _pump(self) -> None:
        self._finish(stream(self.proc, self.log))

    def _wait_only(self) -> None:
        self._finish(self.proc.wait())

    def _finish(self, code: int) -> None:
        self.log("[launcher] process exited with code %d" % code)
        self.proc = None
        self.set_state("stopped" if self.stopping or code == 0 else "error")
        self.stopping = False


def write_batch(key: str, title: str, cwd: Path, env_extra: dict,
                args: list) -> str:
    lines = ["@echo off", "title " + title, 'cd /d "%s"' % cwd]
    for name, value in env_extra.items():
        lines.append('set "%s=%s"' % (name, value))
    lines.append(subprocess.list2cmdline(args))
    path = Path(tempfile.gettempdir()) / ("launcher_%s.cmd" % key)
    path.write_text("\r\n".join(lines) + "\r\n", encoding="utf-8")
    return str(path)


def stream(proc, log) -> int:
    for raw in iter(proc.stdout.readline, b""):
        text = raw.decode("utf-8", "replace").replace("\r", "")
        for line in text.splitlines():
            log(ANSI.sub("", line))
    return proc.wait()


# --------------------------------------------------------------------------
# Add / Edit Server Dialog
# --------------------------------------------------------------------------

class ServerDialog(tk.Toplevel):
    """Dialog for adding a new server or editing an existing one."""

    def __init__(self, parent, existing_keys: set, edit=None):
        super().__init__(parent)
        self.result = None
        self.existing_keys = existing_keys
        self.edit = edit          # dict being edited, or None for add
        self.action_rows = []     # (label_var, cmd_var)
        self.link_rows = []       # (label_var, url_var)

        self.title("Edit Server" if edit else "Add Server")
        self.geometry("560x640")
        self.minsize(540, 600)
        self.configure(bg=BG)
        self.transient(parent)
        self.grab_set()

        self._build_ui()
        self.protocol("WM_DELETE_WINDOW", self._cancel)

        self.update_idletasks()
        px = parent.winfo_x() + (parent.winfo_width() - 560) // 2
        py = parent.winfo_y() + 30
        self.geometry("+%d+%d" % (px, max(30, py)))

    # -- helpers ----------------------------------------------------------

    def _lbl(self, parent, text, pad=6, size=10):
        tk.Label(parent, text=text, bg=CARD_BG, fg=TEXT,
                 font=("Segoe UI", size)).pack(anchor="w", pady=(pad, 2))

    def _hint(self, parent, text):
        tk.Label(parent, text=text, bg=CARD_BG, fg=MUTED,
                 font=("Segoe UI", 8)).pack(anchor="w")

    def _entry(self, parent, var, browse=None):
        row = tk.Frame(parent, bg=CARD_BG)
        row.pack(fill="x", pady=(0, 4))
        ent = tk.Entry(row, textvariable=var, bg=EH, fg=TEXT,
                       insertbackground=TEXT, font=("Consolas", 10),
                       relief="flat", highlightthickness=1,
                       highlightbackground=CARD_BORDER, highlightcolor=ACCENT)
        ent.pack(side="left", fill="x", expand=True, ipady=4)
        if browse:
            tk.Button(row, text="Browse", bg=EH, fg=TEXT,
                      activebackground=EH_HOVER, activeforeground=TEXT,
                      font=("Segoe UI", 9), relief="flat",
                      command=browse).pack(side="right", padx=(6, 0))
        return ent

    # -- ui ---------------------------------------------------------------

    def _build_ui(self):
        outer = tk.Frame(self, bg=BG)
        outer.pack(fill="both", expand=True, padx=12, pady=12)

        card = tk.Frame(outer, bg=CARD_BG)
        card.pack(fill="both", expand=True)

        # scrollable body
        wrap = tk.Frame(card, bg=CARD_BG)
        wrap.pack(fill="both", expand=True, padx=20, pady=(14, 0))
        canvas = tk.Canvas(wrap, bg=CARD_BG, highlightthickness=0)
        vbar = ttk.Scrollbar(wrap, orient="vertical", command=canvas.yview)
        inner = tk.Frame(canvas, bg=CARD_BG)
        inner_id = canvas.create_window((0, 0), window=inner, anchor="nw")
        canvas.configure(yscrollcommand=vbar.set)
        canvas.pack(side="left", fill="both", expand=True)
        vbar.pack(side="right", fill="y")

        def _scroll_configure(event):
            canvas.configure(scrollregion=canvas.bbox("all"))

        def _canvas_configure(event):
            canvas.itemconfigure(inner_id, width=event.width)

        inner.bind("<Configure>", _scroll_configure)
        canvas.bind("<Configure>", _canvas_configure)

        def _on_mousewheel(event):
            canvas.yview_scroll(int(-event.delta / 120), "units")
        canvas.bind_all("<MouseWheel>", _on_mousewheel)

        title = "Edit server" if self.edit else "Add a new server"
        tk.Label(inner, text=title, bg=CARD_BG, fg=TEXT,
                 font=("Segoe UI Semibold", 15)).pack(anchor="w")
        tk.Label(inner, text="Saved to servers.json - no code changes needed.",
                 bg=CARD_BG, fg=MUTED, font=("Segoe UI", 9)).pack(anchor="w",
                                                                  pady=(2, 12))

        f = inner

        # Name
        self._lbl(f, "Server name *")
        self.name_var = tk.StringVar(value=(self.edit or {}).get("name", ""))
        self._entry(f, self.name_var)

        # Working directory
        self._lbl(f, "Working directory (path) *")
        self.cwd_var = tk.StringVar(value=(self.edit or {}).get("cwd", ""))
        self._entry(f, self.cwd_var, browse=self._browse_dir)

        # Start command
        self._lbl(f, "Start command *")
        self._hint(f, "e.g.  npm run dev   |   python app.py   |   node server.js")
        self.cmd_var = tk.StringVar(value=(self.edit or {}).get("command", ""))
        self._entry(f, self.cmd_var)

        # Stop command (optional)
        self._lbl(f, "Stop command  (optional - runs before stopping)")
        self._hint(f, "e.g.  firebase emulators:export ./data   - leave empty for a "
                      "plain kill")
        self.stopcmd_var = tk.StringVar(
            value=(self.edit or {}).get("stop_command", ""))
        self._entry(f, self.stopcmd_var)

        # Port
        self._lbl(f, "Port  (0 = none, used to detect running state)")
        self.port_var = tk.StringVar(
            value=str((self.edit or {}).get("port", 0)))
        self._entry(f, self.port_var)

        # Env vars
        self._lbl(f, "Environment variables (KEY=VALUE, one per line)")
        env_initial = "\n".join("%s=%s" % (k, v)
                                for k, v in (self.edit or {})
                                .get("env", {}).items())
        self.env_text = tk.Text(f, height=3, bg=EH, fg=TEXT,
                                insertbackground=TEXT, font=("Consolas", 9),
                                relief="flat", highlightthickness=1,
                                highlightbackground=CARD_BORDER,
                                highlightcolor=ACCENT, wrap="word")
        self.env_text.insert("1.0", env_initial)
        self.env_text.pack(fill="x", pady=(0, 8))

        # Extra action buttons (sub commands)
        self._lbl(f, "Extra action buttons (sub-commands)", pad=10)
        self._hint(f, "Run inside the working directory.  e.g. label: 'Export "
                      "data', command: 'node export.js'")
        self.actions_box = tk.Frame(f, bg=CARD_BG)
        self.actions_box.pack(fill="x")
        add_act = (self.edit or {}).get("actions", [])
        for item in add_act:
            self._add_action_row(item.get("label", ""), item.get("command", ""))
        tk.Button(f, text="+ Add action button", bg=EH, fg=ACCENT,
                  activebackground=EH_HOVER, activeforeground=ACCENT,
                  font=("Segoe UI", 9), relief="flat",
                  command=self._add_action_row).pack(anchor="w", pady=(2, 6))

        # Links (open in browser)
        self._lbl(f, "Open links (label : url)", pad=6)
        self._hint(f, "One per row.  e.g. label: 'Dashboard', "
                      "url: http://127.0.0.1:3000")
        self.links_box = tk.Frame(f, bg=CARD_BG)
        self.links_box.pack(fill="x")
        add_links = (self.edit or {}).get("links", [])
        for item in add_links:
            self._add_link_row(item.get("label", ""), item.get("url", ""))
        tk.Button(f, text="+ Add link", bg=EH, fg=ACCENT,
                  activebackground=EH_HOVER, activeforeground=ACCENT,
                  font=("Segoe UI", 9), relief="flat",
                  command=self._add_link_row).pack(anchor="w", pady=(2, 6))

        # Buttons
        btn_row = tk.Frame(card, bg=CARD_BG)
        btn_row.pack(fill="x", padx=20, pady=14)
        tk.Button(btn_row, text="Cancel", bg=CARD_BG, fg=MUTED,
                  activebackground="#2a2e3a", activeforeground=TEXT,
                  font=("Segoe UI", 10), relief="flat", width=10,
                  command=self._cancel).pack(side="right")
        tk.Button(btn_row, text="Save", bg=ACCENT, fg="#ffffff",
                  activebackground=ACCENT_HOVER, activeforeground="#ffffff",
                  font=("Segoe UI Semibold", 10), relief="flat", width=10,
                  command=self._save).pack(side="right", padx=(0, 8))

        self.focus_force()
        self.bind("<Escape>", lambda e: self._cancel())

    def _field_row(self, parent, label_var, cmd_var):
        row = tk.Frame(parent, bg=CARD_BG)
        row.pack(fill="x", pady=2)
        tk.Label(row, text="label:", bg=CARD_BG, fg=MUTED,
                 font=("Segoe UI", 8)).pack(side="left")
        lbl_ent = tk.Entry(row, textvariable=label_var, bg=EH, fg=TEXT,
                           insertbackground=TEXT, font=("Consolas", 9),
                           relief="flat", highlightthickness=1,
                           highlightbackground=CARD_BORDER, highlightcolor=ACCENT)
        lbl_ent.pack(side="left", fill="x", expand=False, ipady=3, padx=(2, 8))
        lbl_ent.configure(width=12)
        cmd_ent = tk.Entry(row, textvariable=cmd_var, bg=EH, fg=TEXT,
                           insertbackground=TEXT, font=("Consolas", 9),
                           relief="flat", highlightthickness=1,
                           highlightbackground=CARD_BORDER, highlightcolor=ACCENT)
        cmd_ent.pack(side="left", fill="x", expand=True, ipady=3)
        tk.Button(row, text="x", bg="#24171c", fg=DANGER,
                  activebackground="#331d23", activeforeground=DANGER,
                  font=("Segoe UI", 9), relief="flat", width=2,
                  command=row.destroy).pack(side="left", padx=(6, 0))

    def _add_action_row(self, label="", command=""):
        lv = tk.StringVar(value=label)
        cv = tk.StringVar(value=command)
        self._field_row(self.actions_box, lv, cv)
        self.action_rows.append((lv, cv))

    def _add_link_row(self, label="", url=""):
        lv = tk.StringVar(value=label)
        uv = tk.StringVar(value=url)
        row = tk.Frame(self.links_box, bg=CARD_BG)
        row.pack(fill="x", pady=2)
        tk.Label(row, text="label:", bg=CARD_BG, fg=MUTED,
                 font=("Segoe UI", 8)).pack(side="left")
        lbl_ent = tk.Entry(row, textvariable=lv, bg=EH, fg=TEXT,
                           insertbackground=TEXT, font=("Consolas", 9),
                           relief="flat", highlightthickness=1,
                           highlightbackground=CARD_BORDER, highlightcolor=ACCENT,
                           width=12)
        lbl_ent.pack(side="left", fill="x", expand=False, ipady=3, padx=(2, 8))
        url_ent = tk.Entry(row, textvariable=uv, bg=EH, fg=TEXT,
                           insertbackground=TEXT, font=("Consolas", 9),
                           relief="flat", highlightthickness=1,
                           highlightbackground=CARD_BORDER, highlightcolor=ACCENT)
        url_ent.pack(side="left", fill="x", expand=True, ipady=3)
        tk.Button(row, text="x", bg="#24171c", fg=DANGER,
                  activebackground="#331d23", activeforeground=DANGER,
                  font=("Segoe UI", 9), relief="flat", width=2,
                  command=row.destroy).pack(side="left", padx=(6, 0))
        self.link_rows.append((lv, uv))

    def _browse_dir(self):
        d = filedialog.askdirectory(parent=self, title="Select working directory")
        if d:
            self.cwd_var.set(d)

    def _cancel(self):
        self.result = None
        self.grab_release()
        self.destroy()

    def _save(self):
        name = self.name_var.get().strip()
        cwd = self.cwd_var.get().strip()
        cmd = self.cmd_var.get().strip()

        if not name:
            messagebox.showwarning("Server", "Server name is required.", parent=self)
            return
        if not cwd:
            messagebox.showwarning("Server", "Working directory is required.",
                                   parent=self)
            return
        if not cmd:
            messagebox.showwarning("Server", "Start command is required.",
                                   parent=self)
            return
        if not Path(cwd).is_dir():
            messagebox.showwarning("Server",
                                   "Directory does not exist:\n%s" % cwd,
                                   parent=self)
            return

        key = self.edit.get("key") if self.edit else None
        if not key:
            key = re.sub(r"[^a-z0-9]+", "_", name.lower()).strip("_")
            if not key:
                key = "custom_%d" % int(time.time())
            base_key = key
            i = 2
            while key in self.existing_keys:
                key = "%s_%d" % (base_key, i)
                i += 1

        try:
            port = int(self.port_var.get().strip() or "0")
        except ValueError:
            port = 0

        env = {}
        for line in self.env_text.get("1.0", "end").strip().splitlines():
            line = line.strip()
            if "=" in line:
                k, v = line.split("=", 1)
                env[k.strip()] = v.strip()

        actions = []
        for lv, cv in self.action_rows:
            label = lv.get().strip()
            command = cv.get().strip()
            if label and command:
                actions.append({"label": label, "command": command})

        links = []
        for lv, uv in self.link_rows:
            label = lv.get().strip()
            url = uv.get().strip()
            if label and url:
                links.append({"label": label, "url": url})

        self.result = {
            "name": name,
            "key": key,
            "cwd": cwd,
            "command": cmd,
            "stop_command": self.stopcmd_var.get().strip(),
            "port": port,
            "env": env,
            "actions": actions,
            "links": links,
        }
        self.grab_release()
        self.destroy()


# --------------------------------------------------------------------------
# Application
# --------------------------------------------------------------------------

class LauncherApp:
    def __init__(self, root: tk.Tk):
        self.root = root
        self.events = queue.Queue()
        self.services: dict[str, Service] = {}
        self.cards: dict[str, dict] = {}
        self.logs: dict[str, tk.Text] = {}

        self._build_custom_services()
        self._build_ui()

        self.root.after(120, self._drain)
        self.root.after(1500, self._poll_ports)
        self.root.protocol("WM_DELETE_WINDOW", self._on_close)

    # -- config -----------------------------------------------------------

    def _make_custom_service(self, entry: dict) -> Service:
        key = entry.get("key", "")
        name = entry.get("name", key)
        cwd = entry.get("cwd", ".")
        command = entry.get("command", "")
        port = entry.get("port", 0)
        env = entry.get("env", {})
        actions_raw = entry.get("actions", [])
        links_raw = entry.get("links", [])

        args = split_command(command) if command else ["cmd", "/k"]

        links = [(a.get("label", ""), a.get("url", ""))
                 for a in links_raw if a.get("url")]

        custom_actions = [(a.get("label", ""), a.get("command", ""))
                          for a in actions_raw
                          if a.get("label") and a.get("command")]

        svc = Service(
            self, key, name,
            "%s  (port %s)" % (command, port) if port else command,
            args, cwd, port, env=env, links=links, custom=True,
            custom_actions=custom_actions,
            stop_command=entry.get("stop_command", ""),
        )
        return svc

    def _attach_action_handlers(self, svc: Service) -> None:
        final = []
        for label, command in svc.custom_actions:
            final.append((label, lambda s=svc, c=command: s.run_sub_command(label, c)))
        svc.actions = final

    def _build_custom_services(self) -> None:
        for entry in load_custom_servers():
            svc = self._make_custom_service(entry)
            if svc.custom_actions:
                self._attach_action_handlers(svc)
            self.services[svc.key] = svc

    def _persist_custom(self) -> None:
        servers = []
        for svc in self.services.values():
            if not svc.custom:
                continue
            servers.append({
                "key": svc.key,
                "name": svc.name,
                "cwd": str(svc.cwd),
                "command": subprocess.list2cmdline(svc.args),
                "stop_command": svc.stop_command,
                "port": svc.port,
                "env": svc.env_extra,
                "actions": [{"label": l, "command": c}
                            for l, c in svc.custom_actions],
                "links": [{"label": l, "url": u} for l, u in svc.links],
            })
        save_custom_servers(servers)

    # -- ui ---------------------------------------------------------------

    def _build_ui(self) -> None:
        root = self.root
        root.title("Server Launcher")
        root.geometry("1220x780")
        root.minsize(1020, 660)
        root.configure(bg=BG)

        style = ttk.Style()
        try:
            style.theme_use("clam")
        except tk.TclError:
            pass

        style.configure("TFrame", background=BG)
        style.configure("TCheckbutton", background=CARD_BG, foreground=MUTED,
                        font=("Segoe UI", 9))
        style.configure("TNotebook", background=BG, borderwidth=0)
        style.configure("TNotebook.Tab", background=BG, foreground=MUTED,
                        font=("Segoe UI", 9), padding=(14, 6))
        style.map("TNotebook.Tab",
                  background=[("selected", CARD_BG)],
                  foreground=[("selected", TEXT)])

        header = tk.Frame(root, bg=BG)
        header.pack(fill="x", padx=20, pady=(16, 6))

        left_hdr = tk.Frame(header, bg=BG)
        left_hdr.pack(side="left", fill="x", expand=True)
        tk.Label(left_hdr, text="Server Launcher", bg=BG, fg=TEXT,
                 font=("Segoe UI Semibold", 18)).pack(anchor="w")
        tk.Label(left_hdr,
                 text="this pc: %s    servers: %d"
                      % (lan_ip(), len(self.services)),
                 bg=BG, fg=MUTED, font=("Segoe UI", 9)).pack(anchor="w",
                                                              pady=(2, 0))

        right_hdr = tk.Frame(header, bg=BG)
        right_hdr.pack(side="right")
        tk.Button(right_hdr, text="+ Add Server", bg=ACCENT, fg="#ffffff",
                  activebackground=ACCENT_HOVER, activeforeground="#ffffff",
                  font=("Segoe UI Semibold", 10), relief="flat", padx=14,
                  pady=4, command=self._add_server).pack(side="right", padx=(0, 8))
        tk.Button(right_hdr, text="Stop all", bg=EH, fg=TEXT,
                  activebackground=EH_HOVER, activeforeground=TEXT,
                  font=("Segoe UI", 10), relief="flat", padx=12, pady=4,
                  command=self.stop_all).pack(side="right", padx=(0, 6))
        tk.Button(right_hdr, text="Start all", bg=SUCCESS, fg="#ffffff",
                  activebackground="#00d2a0", activeforeground="#ffffff",
                  font=("Segoe UI Semibold", 10), relief="flat", padx=12, pady=4,
                  command=self.start_all).pack(side="right")

        sep = tk.Frame(root, bg=CARD_BORDER, height=1)
        sep.pack(fill="x", padx=20, pady=(8, 0))

        body = tk.Frame(root, bg=BG)
        body.pack(fill="both", expand=True, padx=20, pady=(12, 16))
        body.columnconfigure(0, minsize=400)
        body.columnconfigure(1, weight=1)
        body.rowconfigure(0, weight=1)

        # Left column - SCROLLABLE card list
        left = tk.Frame(body, bg=BG)
        left.grid(row=0, column=0, sticky="nsew", padx=(0, 16))

        self.card_canvas = tk.Canvas(left, bg=BG, highlightthickness=0)
        self.card_scroll = ttk.Scrollbar(left, orient="vertical",
                                         command=self.card_canvas.yview)
        self._cards_frame = tk.Frame(self.card_canvas, bg=BG)
        self._cards_window = self.card_canvas.create_window(
            (0, 0), window=self._cards_frame, anchor="nw")
        self.card_canvas.configure(yscrollcommand=self.card_scroll.set)
        self.card_canvas.pack(side="left", fill="both", expand=True)
        self.card_scroll.pack(side="right", fill="y")

        def _on_card_configure(event):
            self.card_canvas.configure(scrollregion=self.card_canvas.bbox("all"))

        def _on_canvas_configure(event):
            self.card_canvas.itemconfigure(self._cards_window,
                                           width=event.width)

        self._cards_frame.bind("<Configure>", _on_card_configure)
        self.card_canvas.bind("<Configure>", _on_canvas_configure)

        def _on_wheel(event):
            self.card_canvas.yview_scroll(int(-event.delta / 120), "units")
        self.card_canvas.bind("<Enter>",
                              lambda e: self.card_canvas.bind_all(
                                  "<MouseWheel>", _on_wheel))
        self.card_canvas.bind("<Leave>",
                              lambda e: self.card_canvas.unbind_all(
                                  "<MouseWheel>"))

        for svc in self.services.values():
            self._build_card(self._cards_frame, svc)

        self.notebook = ttk.Notebook(body)
        self.notebook.grid(row=0, column=1, sticky="nsew")
        self._build_overview_tab()
        for svc in self.services.values():
            self._build_log_tab(svc)

    def _admin_button(self, parent, text, fg, command):
        tk.Button(parent, text=text, bg=EH, fg=fg,
                  activebackground=EH_HOVER, activeforeground=fg,
                  font=("Segoe UI", 9), relief="flat", padx=9, pady=2,
                  command=command).pack(side="right", padx=(6, 0))

    # -- overview / all running servers on this pc ------------------------

    def _build_overview_tab(self) -> None:
        frame = tk.Frame(self.notebook, bg=BG)
        self.notebook.add(frame, text="  Server Overview  ")

        top = tk.Frame(frame, bg=BG)
        top.pack(fill="x", padx=16, pady=(14, 8))
        tk.Label(top, text="Servers running on this PC",
                 bg=BG, fg=TEXT, font=("Segoe UI Semibold", 15)).pack(side="left")
        self.overview_time_lbl = tk.Label(top, text="", bg=BG, fg=MUTED,
                                          font=("Segoe UI", 9))
        self.overview_time_lbl.pack(side="left", padx=(14, 0))
        tk.Button(top, text="Refresh", bg=EH, fg=ACCENT,
                  activebackground=EH_HOVER, activeforeground=ACCENT,
                  font=("Segoe UI Semibold", 10), relief="flat", padx=14, pady=4,
                  command=self._refresh_overview).pack(side="right")

        bar = tk.Frame(frame, bg=CARD_BORDER, height=1)
        bar.pack(fill="x", padx=16)

        # scrollable body holding both sections
        body = tk.Frame(frame, bg=BG)
        body.pack(fill="both", expand=True, padx=16, pady=(12, 16))

        self.ov_canvas = tk.Canvas(body, bg=BG, highlightthickness=0)
        self.ov_scroll = ttk.Scrollbar(body, orient="vertical",
                                       command=self.ov_canvas.yview)
        self._ov_inner = tk.Frame(self.ov_canvas, bg=BG)
        self._ov_window = self.ov_canvas.create_window(
            (0, 0), window=self._ov_inner, anchor="nw")
        self.ov_canvas.configure(yscrollcommand=self.ov_scroll.set)
        self.ov_canvas.pack(side="left", fill="both", expand=True)
        self.ov_scroll.pack(side="right", fill="y")

        def _on_ov_inner_configure(event):
            self.ov_canvas.configure(scrollregion=self.ov_canvas.bbox("all"))

        def _on_ov_canvas_configure(event):
            self.ov_canvas.itemconfigure(self._ov_window, width=event.width)

        self._ov_inner.bind("<Configure>", _on_ov_inner_configure)
        self.ov_canvas.bind("<Configure>", _on_ov_canvas_configure)

        def _on_ov_wheel(event):
            self.ov_canvas.yview_scroll(int(-event.delta / 120), "units")

        self.ov_canvas.bind("<Enter>",
                            lambda e: self.ov_canvas.bind_all(
                                "<MouseWheel>", _on_ov_wheel))
        self.ov_canvas.bind("<Leave>",
                            lambda e: self.ov_canvas.unbind_all(
                                "<MouseWheel>"))

        # managed servers section
        tk.Label(self._ov_inner, text="Launcher-managed servers", bg=BG, fg=TEXT,
                 font=("Segoe UI Semibold", 11)).pack(anchor="w", padx=2,
                                                      pady=(0, 4))
        self._managed_box = tk.Frame(self._ov_inner, bg=BG)
        self._managed_box.pack(fill="x")

        # external servers section
        tk.Label(self._ov_inner, text="Other processes listening on this PC",
                 bg=BG, fg=TEXT, font=("Segoe UI Semibold", 11)).pack(
            anchor="w", padx=2, pady=(14, 4))
        self._external_box = tk.Frame(self._ov_inner, bg=BG)
        self._external_box.pack(fill="x")

        # snapshots used to avoid rebuilding unchanged rows
        self._managed_snapshot = None
        self._external_snapshot = None
        self._scanning = False

        self._refresh_overview(force=True)
        self.root.after(5000, self._auto_refresh_overview)

    def _auto_refresh_overview(self) -> None:
        self._refresh_overview(force=False)
        self.root.after(5000, self._auto_refresh_overview)

    def _managed_sig(self) -> tuple:
        return tuple((svc.key,
                      "running" if svc.state in ("running", "starting")
                      else "stopped")
                     for svc in self.services.values())

    def _rebuild_managed_overview(self) -> None:
        sig = self._managed_sig()
        if sig == self._managed_snapshot:
            return
        self._managed_snapshot = sig
        for w in self._managed_box.winfo_children():
            w.destroy()
        for svc in self.services.values():
            display_state = svc.state if svc.state in ("running", "starting") \
                else "stopped"
            self._overview_row(self._managed_box, svc.name,
                               svc.subtitle, display_state, svc)

    def _overview_row(self, parent, name, detail, state, svc=None, pid=None,
                      port=None) -> None:
        if state in ("running", "starting"):
            dot_color = DOT[state]
            state_text = LABEL[state]
        else:
            dot_color = DOT["stopped"]
            state_text = LABEL["stopped"]

        outer = tk.Frame(parent, bg=CARD_BORDER, bd=0, padx=1, pady=1)
        outer.pack(fill="x", pady=(0, 8))
        row = tk.Frame(outer, bg=CARD_BG)
        row.pack(fill="x", padx=1, pady=1)
        inner = tk.Frame(row, bg=CARD_BG)
        inner.pack(fill="x", padx=12, pady=8)

        dot = tk.Canvas(inner, width=12, height=12, bg=CARD_BG,
                        highlightthickness=0)
        oval = dot.create_oval(2, 2, 10, 10, fill=dot_color, outline="")
        dot.pack(side="left")

        text_col = tk.Frame(inner, bg=CARD_BG)
        text_col.pack(side="left", padx=(10, 0), fill="x", expand=True)
        tk.Label(text_col, text=name, bg=CARD_BG, fg=TEXT, anchor="w",
                 font=("Segoe UI Semibold", 11)).pack(anchor="w")
        tk.Label(text_col, text=detail, bg=CARD_BG, fg=MUTED, anchor="w",
                 font=("Segoe UI", 9)).pack(anchor="w")

        if pid and port:
            tk.Label(inner, text="pid %d  :%s" % (pid, port), bg=CARD_BG,
                     fg=MUTED, font=("Consolas", 9)).pack(side="left",
                                                          padx=(12, 0))

        state_lbl = tk.Label(inner, text=state_text, bg=CARD_BG, fg=TEXT,
                             font=("Segoe UI", 9), width=10, anchor="e")
        state_lbl.pack(side="left", padx=(12, 0))

        btn_frame = tk.Frame(inner, bg=CARD_BG)
        btn_frame.pack(side="right")

        is_running = state in ("running", "starting")

        if svc is not None:
            def do_start(s=svc):
                s.start()
            def do_stop(s=svc):
                s.stop()
            if is_running:
                tk.Button(btn_frame, text="Stop", bg=DANGER, fg="#ffffff",
                          activebackground="#d04030", activeforeground="#ffffff",
                          font=("Segoe UI Semibold", 9), relief="flat", padx=10,
                          pady=3, command=do_stop).pack(side="right")
            else:
                tk.Button(btn_frame, text="Start", bg=SUCCESS, fg="#ffffff",
                          activebackground="#00d2a0", activeforeground="#ffffff",
                          font=("Segoe UI Semibold", 9), relief="flat", padx=10,
                          pady=3, command=do_start).pack(side="right")
        elif pid:
            def do_kill(p=pid):
                self._kill_external(p)
            tk.Button(btn_frame, text="Stop", bg=DANGER, fg="#ffffff",
                      activebackground="#d04030", activeforeground="#ffffff",
                      font=("Segoe UI Semibold", 9), relief="flat", padx=10,
                      pady=3, command=do_kill).pack(side="right")

        return row

    def _kill_external(self, pid: int) -> None:
        if not messagebox.askyesno(
                "Stop process", "Kill process %d?  It will be terminated." % pid):
            return
        taskkill(pid)
        self._refresh_overview(force=True)

    def _external_sig(self, listeners: list, managed_pids: set) -> tuple:
        sig = []
        for entry in listeners:
            if entry["pid"] in managed_pids:
                continue
            sig.append((entry["pid"], entry["port"],
                        entry["name"] or "pid"))
        return tuple(sig)

    def _refresh_overview(self, force: bool = False) -> None:
        now = time.strftime("%H:%M:%S")
        self.overview_time_lbl.configure(text="last scan: %s" % now)

        # managed section is cheap to update - do it inline
        sig = self._managed_sig()
        if force or sig != self._managed_snapshot:
            self._rebuild_managed_overview()

        # external scan is slow - run on a background thread so the UI
        # never blocks on netstat/tasklist
        if getattr(self, "_scanning", False):
            return
        self._scanning = True
        threading.Thread(target=self._scan_worker, daemon=True).start()

    def _scan_worker(self) -> None:
        try:
            listeners = scan_listening_servers()
            managed_pids = set()
            for svc in self.services.values():
                if svc.alive:
                    try:
                        managed_pids.add(svc.proc.pid)
                    except Exception:
                        pass
            ext_sig = self._external_sig(listeners, managed_pids)
            changed = ext_sig != self._external_snapshot
            if changed:
                self._external_snapshot = ext_sig
                try:
                    self.root.after(0, lambda: self._apply_external(
                        listeners, managed_pids))
                except (RuntimeError, tk.TclError):
                    pass
        finally:
            self._scanning = False

    def _apply_external(self, listeners: list, managed_pids: set) -> None:
        for w in self._external_box.winfo_children():
            w.destroy()
        count = 0
        for entry in listeners:
            pid = entry["pid"]
            if pid in managed_pids:
                continue
            count += 1
            proto = entry["proto"]
            port = entry["port"]
            name = entry["name"] or ("pid %d" % pid)
            detail = "%s :%s  (%s)" % (name, port, proto)
            self._overview_row(self._external_box, name, detail,
                               "running", pid=pid, port=port)
        if count == 0:
            tk.Label(self._external_box,
                     text="No unmanaged processes found listening on a "
                          "port.",
                     bg=BG, fg=MUTED, font=("Segoe UI", 9)).pack(anchor="w",
                                                                 pady=(2, 6))

    # -- card building ----------------------------------------------------

    def _build_card(self, parent: tk.Frame, svc: Service) -> None:
        outer = tk.Frame(parent, bg=CARD_BORDER, bd=0, padx=1, pady=1)
        outer.pack(fill="x", pady=(0, 10))

        card = tk.Frame(outer, bg=CARD_BG)
        card.pack(fill="x", padx=1, pady=1)
        card_inner = tk.Frame(card, bg=CARD_BG)
        card_inner.pack(fill="x", padx=14, pady=12)
        card_inner.columnconfigure(1, weight=1)

        dot = tk.Canvas(card_inner, width=14, height=14, bg=CARD_BG,
                        highlightthickness=0)
        oval = dot.create_oval(2, 2, 12, 12, fill=DOT["stopped"], outline="")
        dot.grid(row=0, column=0, sticky="nw", padx=(0, 10))

        name_row = tk.Frame(card_inner, bg=CARD_BG)
        name_row.grid(row=0, column=1, sticky="ew")
        name_row.columnconfigure(0, weight=1)

        name_holder = tk.Frame(name_row, bg=CARD_BG)
        name_holder.grid(row=0, column=0, sticky="w")
        tk.Label(name_holder, text=svc.name, bg=CARD_BG, fg=TEXT,
                 font=("Segoe UI Semibold", 11),
                 anchor="w").pack(side="left")

        state_lbl = tk.Label(name_row, text=LABEL["stopped"], bg=CARD_BG,
                             fg=MUTED, font=("Segoe UI", 9), anchor="e")
        state_lbl.grid(row=0, column=1, sticky="e")

        tk.Label(card_inner, text=svc.subtitle, bg=CARD_BG, fg=MUTED,
                 font=("Segoe UI", 9), wraplength=340, justify="left", anchor="w",
                 ).grid(row=1, column=0, columnspan=2, sticky="w",
                        pady=(4, 10))

        controls = tk.Frame(card_inner, bg=CARD_BG)
        controls.grid(row=2, column=0, columnspan=2, sticky="ew")

        toggle = tk.Button(controls, text="Start", bg=SUCCESS, fg="#ffffff",
                           activebackground="#00d2a0", activeforeground="#ffffff",
                           font=("Segoe UI Semibold", 10), relief="flat",
                           padx=16, pady=4,
                           command=lambda s=svc: self.toggle(s))
        toggle.pack(side="left")

        svc.external_var = tk.BooleanVar(value=svc.external_default)
        ttk.Checkbutton(controls, text="own console",
                        variable=svc.external_var).pack(side="left", padx=(12, 0))

        # admin buttons on the right: edit + delete (custom only)
        if svc.custom:
            self._admin_button(controls, "Delete", DANGER,
                               lambda s=svc: self._delete_server(s))
            self._admin_button(controls, "Edit", ACCENT,
                               lambda s=svc: self._edit_server(s))

        row = 3

        action_buttons = []
        if svc.actions:
            actions = tk.Frame(card_inner, bg=CARD_BG)
            actions.grid(row=row, column=0, columnspan=2, sticky="w", pady=(8, 0))
            row += 1
            for text, callback in svc.actions:
                if callback is None:
                    continue
                button = tk.Button(actions, text=text, bg=EH, fg=ACCENT,
                                   activebackground=EH_HOVER,
                                   activeforeground=ACCENT,
                                   font=("Segoe UI", 9), relief="flat", padx=9,
                                   pady=3, command=callback)
                button.pack(side="left", padx=(0, 6))
                action_buttons.append(button)

        # custom sub-command buttons
        for label, command in svc.custom_actions:
            if not label or not command:
                continue
            cb = (lambda s=svc, c=command: s.run_sub_command(label, c))
            button = tk.Button(actions, text=label, bg=EH, fg=ACCENT,
                               activebackground=EH_HOVER, activeforeground=ACCENT,
                               font=("Segoe UI", 9), relief="flat", padx=9,
                               pady=3, command=cb)
            button.pack(side="left", padx=(0, 6))
            action_buttons.append(button)

        if svc.links:
            links = tk.Frame(card_inner, bg=CARD_BG)
            links.grid(row=row, column=0, columnspan=2, sticky="w", pady=(8, 0))
            row += 1
            for text, url in svc.links:
                b = tk.Label(links, text=text, bg=CARD_BG, fg=ACCENT,
                             font=("Segoe UI", 9, "underline"), cursor="hand2")
                b.pack(side="left", padx=(0, 12))
                b.bind("<Button-1>", lambda e, u=url: webbrowser.open(u))

        if svc.missing:
            tk.Label(card_inner, text=svc.missing, bg=CARD_BG, fg=DANGER,
                     font=("Segoe UI", 9), wraplength=340, justify="left",
                     anchor="w").grid(row=row, column=0, columnspan=2,
                                      sticky="w", pady=(6, 0))
            toggle.configure(state="disabled")
            for button in action_buttons:
                button.configure(state="disabled")

        self.cards[svc.key] = {
            "dot": dot, "oval": oval, "state": state_lbl,
            "toggle": toggle, "actions": action_buttons, "outer": outer,
        }

    def _build_log_tab(self, svc: Service) -> None:
        frame = tk.Frame(self.notebook, bg=BG)
        self.notebook.add(frame, text="  %s  " % svc.name)
        frame.rowconfigure(0, weight=1)
        frame.columnconfigure(0, weight=1)

        text = tk.Text(frame, bg=LOG_BG, fg=LOG_FG, insertbackground=LOG_FG,
                       font=("Consolas", 9), wrap="none", relief="flat",
                       padx=10, pady=8, state="disabled",
                       highlightthickness=0, bd=0)
        text.grid(row=0, column=0, sticky="nsew")
        yscroll = ttk.Scrollbar(frame, orient="vertical", command=text.yview)
        yscroll.grid(row=0, column=1, sticky="ns")
        xscroll = ttk.Scrollbar(frame, orient="horizontal", command=text.xview)
        xscroll.grid(row=1, column=0, sticky="ew")
        text.configure(yscrollcommand=yscroll.set, xscrollcommand=xscroll.set)
        text.tag_configure("launcher", foreground="#7c8aff")

        bar = tk.Frame(frame, bg=BG)
        bar.grid(row=2, column=0, columnspan=2, sticky="ew", pady=(6, 0))
        tk.Button(bar, text="Clear", bg=EH, fg=MUTED,
                  activebackground=EH_HOVER, activeforeground=TEXT,
                  font=("Segoe UI", 9), relief="flat", padx=8,
                  command=lambda k=svc.key: self.clear_log(k)).pack(side="right")

        self.logs[svc.key] = text

    # -- events -----------------------------------------------------------

    def post(self, event: tuple) -> None:
        self.events.put(event)

    def _drain(self) -> None:
        try:
            while True:
                event = self.events.get_nowait()
                if event[0] == "log":
                    self._append(event[1], event[2])
                elif event[0] == "state":
                    self._refresh(event[1])
                elif event[0] == "actions":
                    self._set_actions(event[1], event[2])
        except queue.Empty:
            pass
        self.root.after(120, self._drain)

    def _append(self, key: str, line: str) -> None:
        widget = self.logs[key]
        at_end = widget.yview()[1] > 0.999
        widget.configure(state="normal")
        widget.insert("end", line + "\n",
                      "launcher" if line.startswith("[launcher]") else "")
        if int(widget.index("end-1c").split(".")[0]) > 2500:
            widget.delete("1.0", "500.0")
        widget.configure(state="disabled")
        if at_end:
            widget.see("end")

    def clear_log(self, key: str) -> None:
        widget = self.logs[key]
        widget.configure(state="normal")
        widget.delete("1.0", "end")
        widget.configure(state="disabled")

    def _refresh(self, key: str) -> None:
        svc = self.services[key]
        card = self.cards[key]
        card["dot"].itemconfigure(card["oval"], fill=DOT[svc.state])
        card["state"].configure(text=LABEL[svc.state])
        if not svc.missing:
            card["toggle"].configure(
                text="Stop" if svc.state in ("running", "starting") else "Start",
                bg=DANGER if svc.state in ("running", "starting") else SUCCESS,
                activebackground="#d04030" if svc.state in ("running", "starting")
                else "#00d2a0",
            )
            if svc.state == "stopping":
                card["toggle"].configure(state="disabled")
            else:
                card["toggle"].configure(state="normal")
        if hasattr(self, "_managed_box"):
            self._rebuild_managed_overview()

    def _set_actions(self, key: str, enabled: bool) -> None:
        svc = self.services[key]
        if svc.missing:
            return
        for button in self.cards[key]["actions"]:
            button.configure(state="normal" if enabled else "disabled")

    def _poll_ports(self) -> None:
        for svc in self.services.values():
            if svc.state != "starting" or not svc.alive:
                continue
            # port configured and open -> clearly running
            if svc.port and port_open(svc.port):
                svc.set_state("running")
                continue
            # no port (or port not confirmed): if the process has stayed
            # alive for a grace period, treat it as running
            try:
                started = getattr(svc, "_started_at", None)
                if started is None:
                    svc._started_at = time.time()
                    started = svc._started_at
                if not svc.port or time.time() - started >= 6.0:
                    svc.set_state("running")
            except Exception:
                pass
        self.root.after(1500, self._poll_ports)

    # -- add / edit / delete servers --------------------------------------

    def _add_server(self) -> None:
        existing = set(self.services.keys())
        dlg = ServerDialog(self.root, existing)
        self.root.wait_window(dlg)

        if dlg.result is None:
            return

        r = dlg.result
        svc = self._make_custom_service(r)
        self._attach_action_handlers(svc)
        self.services[svc.key] = svc

        self._build_card(self._cards_frame, svc)
        self._build_log_tab(svc)
        self.notebook.select(len(self.notebook.tabs()) - 1)

        self.card_canvas.update_idletasks()
        self.card_canvas.configure(scrollregion=self.card_canvas.bbox("all"))

        self._persist_custom()
        self._refresh_overview()
        self._update_header_count()

    def _edit_server(self, svc: Service) -> None:
        if svc.alive:
            messagebox.showwarning("Edit Server",
                                   "Stop the server before editing it.")
            return
        existing = set(self.services.keys())
        if svc.key in existing:
            existing.discard(svc.key)  # allow keeping its own key

        entry = {
            "key": svc.key,
            "name": svc.name,
            "cwd": str(svc.cwd),
            "command": subprocess.list2cmdline(svc.args),
            "stop_command": svc.stop_command,
            "port": svc.port,
            "env": svc.env_extra,
            "actions": [{"label": l, "command": c} for l, c in svc.custom_actions],
            "links": [{"label": l, "url": u} for l, u in svc.links],
        }
        dlg = ServerDialog(self.root, existing, edit=entry)
        self.root.wait_window(dlg)

        if dlg.result is None:
            return

        r = dlg.result
        old_key = svc.key
        new_key = r["key"]

        new_svc = self._make_custom_service(r)
        self._attach_action_handlers(new_svc)
        new_svc.key = new_key
        self.services.pop(old_key, None)
        self.services[new_key] = new_svc

        # destroy old card + tab
        self.cards[old_key]["outer"].destroy()
        del self.cards[old_key]
        for i, tab_id in enumerate(self.notebook.tabs()):
            if self.notebook.tab(tab_id, "text").strip() == svc.name:
                self.notebook.forget(i)
        if old_key in self.logs:
            del self.logs[old_key]

        # build new card + tab
        self._build_card(self._cards_frame, new_svc)
        self._build_log_tab(new_svc)

        self.card_canvas.update_idletasks()
        self.card_canvas.configure(scrollregion=self.card_canvas.bbox("all"))

        self._persist_custom()
        self._refresh_overview()
        self._update_header_count()

    def _delete_server(self, svc: Service) -> None:
        if svc.alive:
            messagebox.showwarning("Delete Server",
                                   "Stop the server before deleting it.")
            return
        if not messagebox.askyesno("Delete Server",
                                   "Delete \"%s\"?  This cannot be undone."
                                   % svc.name):
            return

        self.cards[svc.key]["outer"].destroy()
        del self.cards[svc.key]
        for i, tab_id in enumerate(self.notebook.tabs()):
            if self.notebook.tab(tab_id, "text").strip() == svc.name:
                self.notebook.forget(i)
                break
        if svc.key in self.logs:
            del self.logs[svc.key]

        del self.services[svc.key]
        self._persist_custom()
        self._refresh_overview()
        self._update_header_count()

    # -- actions ----------------------------------------------------------

    def _update_header_count(self) -> None:
        # refresh the "servers: N" line in the header if present
        pass

    def toggle(self, svc: Service) -> None:
        if svc.alive:
            svc.stop()
        else:
            svc.start()

    def start_all(self) -> None:
        if not self.services:
            messagebox.showinfo("Server Launcher",
                                "No servers defined yet.\n\n"
                                "Click \"+ Add Server\" to create one.")
            return
        threading.Thread(target=self._start_all_worker, daemon=True).start()

    def _start_all_worker(self) -> None:
        for key in list(self.services.keys()):
            svc = self.services.get(key)
            if svc is None or svc.alive or svc.missing:
                continue
            svc.start()
            time.sleep(1.0)

    def stop_all(self) -> None:
        for svc in self.services.values():
            if svc.alive:
                svc.stop()

    def _on_close(self) -> None:
        running = [s.name for s in self.services.values() if s.alive]
        if running:
            answer = messagebox.askyesnocancel(
                "Server Launcher",
                "These servers are still running:\n\n  "
                + "\n  ".join(running)
                + "\n\nStop them before closing?",
            )
            if answer is None:
                return
            if answer:
                self.stop_all()
                deadline = time.time() + 90
                while time.time() < deadline and \
                        any(s.alive for s in self.services.values()):
                    self.root.update()
                    time.sleep(0.15)
        self.root.destroy()


def main() -> int:
    root = tk.Tk()
    LauncherApp(root)
    root.mainloop()
    return 0


if __name__ == "__main__":
    sys.exit(main())
