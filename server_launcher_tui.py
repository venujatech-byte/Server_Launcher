#!/usr/bin/env python3
"""
Server Launcher TUI
===================
A Textual-based terminal control panel for dev services.
Supports React, Next.js, Flask, FastAPI, databases, and any custom server.

Usage:
    python server_launcher_tui.py

Requirements:
    pip install "textual>=0.50.0"

Standard-library process management is identical to server_launcher_linux.py.
Only the UI layer has been replaced with Textual.
"""

from __future__ import annotations

import glob
import json
import os
import queue
import re
import shlex
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

# ── Check textual ─────────────────────────────────────────────────────────────
try:
    from textual import events, on
    from textual.app import App, ComposeResult
    from textual.binding import Binding
    from textual.containers import Container, Horizontal, Vertical, VerticalScroll
    from textual.message import Message
    from textual.reactive import reactive
    from textual.screen import ModalScreen
    from textual.widget import Widget
    from textual.widgets import (
        Button,
        ContentSwitcher,
        DataTable,
        Footer,
        Input,
        Label,
        RichLog,
        Select,
        Static,
        Tab,
        TabbedContent,
        TabPane,
        Tabs,
        TextArea,
    )
    from rich.text import Text
except ImportError:
    print("\n  Textual not found. Install it with:\n")
    print("    pip install 'textual>=0.50.0'\n")
    sys.exit(1)

# ── Platform ──────────────────────────────────────────────────────────────────
IS_WINDOWS = sys.platform.startswith("win")
CREATE_NO_WINDOW  = 0x08000000 if IS_WINDOWS else 0
CREATE_NEW_CONSOLE = 0x00000010 if IS_WINDOWS else 0

# Jiffies per second — used for CPU% calculation
HZ = 100
try:
    HZ = os.sysconf("SC_CLK_TCK")
except (AttributeError, ValueError, OSError):
    pass

# ── Paths ─────────────────────────────────────────────────────────────────────
SCRIPT_DIR  = Path(__file__).resolve().parent
SERVERS_JSON = SCRIPT_DIR / "servers.json"

# ── ANSI strip ────────────────────────────────────────────────────────────────
ANSI = re.compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07]*\x07")

# ── State display maps ────────────────────────────────────────────────────────
LABEL_MAP = {
    "stopped":  "Stopped",
    "starting": "Starting…",
    "running":  "Running",
    "stopping": "Stopping…",
    "error":    "Error",
}
DOT_CHAR = {
    "stopped":  "○",
    "starting": "◔",
    "running":  "●",
    "stopping": "◔",
    "error":    "✕",
}
STATE_COLOR = {
    "stopped":  "#555d6e",
    "starting": "#fdcb6e",
    "running":  "#00b894",
    "stopping": "#fdcb6e",
    "error":    "#e17055",
}


# ══════════════════════════════════════════════════════════════════════════════
# Utility — Environment & Subprocess
# ══════════════════════════════════════════════════════════════════════════════

def _normalize_path_env() -> None:
    """Ensure standard user binary paths are in PATH on POSIX."""
    if IS_WINDOWS:
        return
    home = Path.home()
    additions = [
        home / ".local" / "bin",
        home / "bin",
        home / ".cargo" / "bin",
        home / ".npm-global" / "bin",
    ]
    cur = os.environ.get("PATH", "").split(os.pathsep)
    for p in additions:
        s = str(p)
        if p.is_dir() and s not in cur:
            cur.insert(0, s)
    os.environ["PATH"] = os.pathsep.join(cur)


def _proc_kwargs(hidden: bool = True, session: bool = False) -> dict:
    kw: dict = {}
    if IS_WINDOWS:
        if hidden:
            kw["creationflags"] = CREATE_NO_WINDOW
    else:
        if session:
            kw["start_new_session"] = True
    return kw


def _find_venv_python(cwd) -> str:
    cwd = Path(cwd)
    if cwd.is_dir():
        for folder in (".venv", "venv", "env", ".virtualenv"):
            subfolders = ("bin", "Scripts") if not IS_WINDOWS else ("Scripts", "bin")
            executables = (
                ("python3", "python", "python.exe", "python3.exe")
                if not IS_WINDOWS
                else ("python.exe", "python3.exe", "python", "python3")
            )
            for sub in subfolders:
                for exe in executables:
                    cand = cwd / folder / sub / exe
                    if cand.is_file():
                        return str(cand)
    return ""


def split_command(command: str) -> list:
    try:
        parts = shlex.split(command, posix=not IS_WINDOWS)
    except ValueError:
        parts = shlex.split(command, posix=True)
    out = []
    for tok in parts:
        if len(tok) >= 2 and tok.startswith('"') and tok.endswith('"'):
            tok = tok[1:-1]
        out.append(tok)
    return out


def format_command(args: list) -> str:
    if IS_WINDOWS:
        return subprocess.list2cmdline(args)
    return shlex.join(str(a) for a in args)


def resolve_command(args: list, cwd=None) -> list:
    args = list(args)
    if not args:
        return args
    exe = str(args[0])
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
    if not resolved and not IS_WINDOWS and base == "python":
        resolved = shutil.which("python3")
    if resolved:
        args[0] = resolved
    return args


def _get_project_env(cwd: Path, env_extra: dict) -> dict:
    env = os.environ.copy()
    env.update(env_extra)
    env.setdefault("PYTHONUNBUFFERED", "1")
    cwd_p = Path(cwd)
    for folder in (".venv", "venv", "env", ".virtualenv"):
        sub  = "Scripts" if IS_WINDOWS else "bin"
        vbin = cwd_p / folder / sub
        if vbin.is_dir():
            env["PATH"] = str(vbin) + os.pathsep + env.get("PATH", "")
            env["VIRTUAL_ENV"] = str(cwd_p / folder)
            break
    node_bin = cwd_p / "node_modules" / ".bin"
    if node_bin.is_dir():
        env["PATH"] = str(node_bin) + os.pathsep + env.get("PATH", "")
    return env


# ══════════════════════════════════════════════════════════════════════════════
# Persistence
# ══════════════════════════════════════════════════════════════════════════════

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


# ══════════════════════════════════════════════════════════════════════════════
# Network
# ══════════════════════════════════════════════════════════════════════════════

def port_open(port: int, host: str = "127.0.0.1", timeout: float = 0.35) -> bool:
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            s.settimeout(timeout)
            if s.connect_ex((host, port)) == 0:
                return True
    except OSError:
        pass
    if host in ("127.0.0.1", "localhost"):
        try:
            with socket.socket(socket.AF_INET6, socket.SOCK_STREAM) as s:
                s.settimeout(timeout)
                if s.connect_ex(("::1", port)) == 0:
                    return True
        except OSError:
            pass
    return False


def lan_ip() -> str:
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as s:
            s.connect(("8.8.8.8", 80))
            return s.getsockname()[0]
    except OSError:
        return "127.0.0.1"


# ══════════════════════════════════════════════════════════════════════════════
# Process Management
# ══════════════════════════════════════════════════════════════════════════════

def _get_child_pids_linux(pid: int) -> list[int]:
    children: list[int] = []
    for path in glob.glob(f"/proc/{pid}/task/*/children"):
        try:
            with open(path, "r", encoding="ascii") as f:
                for c in f.read().split():
                    if c.isdigit():
                        cid = int(c)
                        children.extend(_get_child_pids_linux(cid))
                        children.append(cid)
        except OSError:
            pass
    return children


def kill_process_tree(pid: int) -> None:
    if pid <= 0:
        return
    if IS_WINDOWS:
        try:
            subprocess.run(
                ["taskkill", "/PID", str(pid), "/T", "/F"],
                capture_output=True, creationflags=CREATE_NO_WINDOW, timeout=10,
            )
        except (OSError, subprocess.SubprocessError):
            pass
        return
    # POSIX — graceful SIGTERM then forced SIGKILL
    pgid = None
    try:
        cand = os.getpgid(pid)
        if cand == pid:
            pgid = cand
    except OSError:
        pass
    children = _get_child_pids_linux(pid)
    for sig in (signal.SIGTERM, signal.SIGKILL):
        if pgid is not None:
            try: os.killpg(pgid, sig)
            except OSError: pass
        for cpid in children:
            try: os.kill(cpid, sig)
            except OSError: pass
        try: os.kill(pid, sig)
        except OSError: pass
        if shutil.which("pkill"):
            try:
                subprocess.run(
                    ["pkill", "-" + sig.name, "-P", str(pid)],
                    capture_output=True, timeout=2,
                )
            except Exception:
                pass
        if sig == signal.SIGTERM:
            time.sleep(0.15)


# ══════════════════════════════════════════════════════════════════════════════
# Server Scanning (ss / lsof / netstat)
# ══════════════════════════════════════════════════════════════════════════════

def _linux_pid_name(pid: int) -> str:
    if pid <= 0:
        return "system"
    try:
        name = Path(f"/proc/{pid}/comm").read_text("utf-8", errors="replace").strip()
        if name:
            return name
    except OSError:
        pass
    try:
        raw = Path(f"/proc/{pid}/cmdline").read_bytes().split(b"\0")
        if raw and raw[0]:
            return Path(os.fsdecode(raw[0])).name
    except OSError:
        pass
    return f"pid {pid}"


def scan_listening_servers() -> list[dict]:
    if not IS_WINDOWS:
        return _scan_linux()
    return _scan_windows()


def _scan_linux() -> list[dict]:
    found: dict = {}
    if shutil.which("ss"):
        try:
            out = subprocess.run(["ss", "-tlpn"], capture_output=True, text=True, timeout=5).stdout
            for line in out.splitlines()[1:]:
                parts = line.strip().split()
                if len(parts) >= 4 and parts[0] == "LISTEN":
                    port_str = parts[3].rsplit(":", 1)[-1]
                    if port_str.isdigit():
                        port = int(port_str)
                        m = re.search(r'users:\(\("([^"]+)",pid=(\d+)', line)
                        pid = int(m.group(2)) if m else 0
                        found[(pid, port)] = {
                            "proto": "TCP", "port": port, "pid": pid,
                            "name": _linux_pid_name(pid) if pid else "system/other",
                        }
        except Exception:
            pass
    if shutil.which("lsof"):
        try:
            out = subprocess.run(
                ["lsof", "-iTCP", "-sTCP:LISTEN", "-n", "-P"],
                capture_output=True, text=True, timeout=5,
            ).stdout
            for line in out.splitlines()[1:]:
                parts = line.strip().split()
                if len(parts) >= 9:
                    try:
                        pid = int(parts[1])
                    except ValueError:
                        continue
                    port_str = parts[8].rsplit(":", 1)[-1]
                    if port_str.isdigit():
                        port = int(port_str)
                        found[(pid, port)] = {
                            "proto": "TCP", "port": port, "pid": pid,
                            "name": _linux_pid_name(pid),
                        }
                        found.pop((0, port), None)
        except Exception:
            pass
    return sorted(found.values(), key=lambda d: d["port"])


def _scan_windows() -> list[dict]:
    found: dict = {}
    _RE = re.compile(
        r"\s*(?P<proto>TCP|UDP)\s+(?P<local>[0-9.:\[\]]+)\s+"
        r"(?P<remote>[0-9.:\[\]]+)\s+(?P<state>[A-Z_]+)?\s*(?P<pid>\d+)"
    )
    try:
        raw = subprocess.run(
            ["netstat", "-ano"], capture_output=True,
            creationflags=CREATE_NO_WINDOW, timeout=20, text=True,
        ).stdout or ""
    except (OSError, subprocess.SubprocessError):
        raw = ""
    names: dict[int, str] = {}
    try:
        import csv
        out2 = subprocess.run(
            ["tasklist", "/FO", "CSV", "/NH"],
            capture_output=True, creationflags=CREATE_NO_WINDOW, timeout=30, text=True,
        ).stdout or ""
        for line in out2.splitlines():
            try:
                parts = next(iter(csv.reader([line])))
                if len(parts) >= 2 and parts[1].strip().isdigit():
                    names[int(parts[1].strip())] = parts[0].strip('"')
            except Exception:
                pass
    except (OSError, subprocess.SubprocessError):
        pass
    for line in raw.splitlines():
        m = _RE.match(line)
        if not m or m.group("proto") != "TCP":
            continue
        if (m.group("state") or "").upper() != "LISTENING":
            continue
        try:
            port = int(m.group("local").rsplit(":", 1)[1])
            pid  = int(m.group("pid"))
        except ValueError:
            continue
        found.setdefault((pid, port), {
            "proto": "TCP", "port": port, "pid": pid,
            "name": names.get(pid, f"pid {pid}"),
        })
    return sorted(found.values(), key=lambda d: d["port"])


# ══════════════════════════════════════════════════════════════════════════════
# External Terminal Support
# ══════════════════════════════════════════════════════════════════════════════

def write_shell_script(key: str, title: str, cwd: Path, env_extra: dict, args: list) -> str:
    lines = ["#!/usr/bin/env bash"]
    for k, v in env_extra.items():
        lines.append('export %s="%s"' % (k, str(v).replace('"', '\\"')))
    lines += [
        'cd "%s"' % cwd,
        'echo -ne "\\033]0;%s\\007"' % title,
        " ".join(shlex.quote(str(a)) for a in args),
        "EXIT_CODE=$?",
        'echo ""',
        'echo "[launcher] Process exited with code $EXIT_CODE."',
        'read -n 1 -s -r -p "Press any key to close..."',
    ]
    path = Path(tempfile.gettempdir()) / f"launcher_{key}.sh"
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    try:
        path.chmod(0o755)
    except OSError:
        pass
    return str(path)


def _get_external_terminal_command(script: str, title: str, cwd: Path) -> list[str] | None:
    for term, args in [
        ("xdg-terminal-exec", [script]),
        ("ptyxis",            ["--working-directory", str(cwd), "--", script]),
        ("gnome-terminal",    ["--", script]),
        ("konsole",           ["-e", script]),
        ("xfce4-terminal",    ["-e", script]),
        ("x-terminal-emulator", ["-e", script]),
        ("xterm",             ["-T", title, "-e", script]),
        ("alacritty",         ["-e", script]),
        ("kitty",             [script]),
        ("foot",              [script]),
        ("mate-terminal",     ["-e", script]),
        ("lxterminal",        ["-e", script]),
        ("terminator",        ["-x", script]),
    ]:
        if shutil.which(term):
            return [term] + args
    return None


def stream(proc, log) -> int:
    for raw in iter(proc.stdout.readline, b""):
        text = raw.decode("utf-8", "replace").replace("\r", "")
        for line in text.splitlines():
            log(ANSI.sub("", line))
    return proc.wait()


# ══════════════════════════════════════════════════════════════════════════════
# CPU / RAM Monitoring  (Linux /proc — Windows: skipped gracefully)
# ══════════════════════════════════════════════════════════════════════════════

def _proc_ticks(pid: int) -> int:
    """Return utime+stime jiffies for one process."""
    try:
        stat = Path(f"/proc/{pid}/stat").read_text(encoding="ascii", errors="replace")
        idx = stat.rfind(")")
        if idx == -1:
            return 0
        fields = stat[idx + 2:].split()
        return int(fields[11]) + int(fields[12])      # utime + stime
    except (OSError, IndexError, ValueError):
        return 0


def _proc_rss_kb(pid: int) -> int:
    """Return RSS in kB for one process."""
    try:
        for line in Path(f"/proc/{pid}/status").read_text("utf-8", errors="replace").splitlines():
            if line.startswith("VmRSS:"):
                return int(line.split()[1])
    except (OSError, ValueError):
        pass
    return 0


def _tree_stats(root_pid: int) -> tuple[int, int]:
    """Return (total_ticks, total_rss_kb) for root_pid + all children."""
    all_pids = [root_pid] + _get_child_pids_linux(root_pid)
    return (
        sum(_proc_ticks(p) for p in all_pids),
        sum(_proc_rss_kb(p)  for p in all_pids),
    )


def _fmt_uptime(secs: int) -> str:
    if secs < 60:
        return f"{secs}s"
    if secs < 3600:
        return f"{secs // 60}m {secs % 60}s"
    h = secs // 3600
    m = (secs % 3600) // 60
    return f"{h}h {m}m"


# ══════════════════════════════════════════════════════════════════════════════
# Service
# ══════════════════════════════════════════════════════════════════════════════

class Service:
    def __init__(
        self, app, key, name, subtitle, args, cwd, port,
        env=None, links=(), external=False, missing=None,
        actions=(), custom=False, custom_actions=(), stop_command="",
        group="General",
    ):
        self.app            = app
        self.key            = key
        self.name           = name
        self.subtitle       = subtitle
        self.args           = list(args)
        self.cwd            = Path(cwd)
        self.port           = port
        self.env_extra      = dict(env or {})
        self.links          = list(links)
        self.external_default = external
        self.missing        = missing
        self.actions        = list(actions)
        self.custom         = custom
        self.custom_actions = list(custom_actions)   # [(label, command), …]
        self.stop_command   = stop_command
        self.group          = (group or "General").strip() or "General"

        self.proc:     subprocess.Popen | None = None
        self.sub_proc: subprocess.Popen | None = None
        self.state     = "stopped"
        self.stopping  = False
        self._started_at: float | None = None

        # Stats — updated by App's background poller
        self._prev_ticks:     int   = 0
        self._prev_tick_time: float = 0.0
        self.cpu_pct:  float = 0.0
        self.ram_mb:   float = 0.0
        self.uptime_secs: int = 0

    @property
    def alive(self) -> bool:
        return self.proc is not None and self.proc.poll() is None

    def set_state(self, state: str) -> None:
        self.state = state
        if self.app:
            self.app.post(("state", self.key))

    def log(self, line: str) -> None:
        if self.app:
            self.app.post(("log", self.key, line))

    # ── sub-commands ──────────────────────────────────────────────────────────

    def run_sub_command(self, label: str, command: str) -> None:
        if self.sub_proc and self.sub_proc.poll() is None:
            self.log("[launcher] a command is already running (Ctrl+C to stop it)")
            return
        if not self.cwd.exists():
            self.log("[launcher] folder not found: " + str(self.cwd))
            return
        self.log(f"[launcher] {self.cwd}")
        self.log("[launcher] > " + command)
        env = _get_project_env(self.cwd, self.env_extra)
        if IS_WINDOWS:
            run_args = ["cmd", "/c", command]
        else:
            shell = os.environ.get("SHELL", "/bin/bash")
            if not shutil.which(shell):
                shell = "/bin/sh"
            run_args = [shell, "-c", command]
        try:
            self.sub_proc = subprocess.Popen(
                run_args, cwd=str(self.cwd), env=env,
                stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                stdin=subprocess.PIPE, **_proc_kwargs(hidden=True, session=True),
            )
            threading.Thread(
                target=self._pump_foreign, args=(self.sub_proc,), daemon=True
            ).start()
        except OSError as exc:
            self.sub_proc = None
            self.log("[launcher] could not run: " + str(exc))

    def _pump_foreign(self, proc) -> None:
        try:
            code = stream(proc, self.log)
            self.log(f"[launcher] command exited with code {code}")
        finally:
            if self.sub_proc is proc:
                self.sub_proc = None

    # ── stdin / interrupt ──────────────────────────────────────────────────────

    def send_input(self, text: str) -> bool:
        stripped = text.strip()
        # Shell escape prefix
        if stripped.startswith(("!", "/")):
            cmd = stripped[1:].strip()
            if cmd:
                self.run_sub_command("command", cmd)
                return True
        # Pipe to main proc stdin
        if self.alive and self.proc and self.proc.stdin and not self.proc.stdin.closed:
            try:
                raw = text if text.endswith("\n") else text + "\n"
                self.proc.stdin.write(raw.encode("utf-8"))
                self.proc.stdin.flush()
                self.log("> " + stripped)
                return True
            except (BrokenPipeError, OSError):
                pass
        # Pipe to sub_proc stdin
        if self.sub_proc and self.sub_proc.poll() is None:
            if self.sub_proc.stdin and not self.sub_proc.stdin.closed:
                try:
                    raw = text if text.endswith("\n") else text + "\n"
                    self.sub_proc.stdin.write(raw.encode("utf-8"))
                    self.sub_proc.stdin.flush()
                    self.log("> " + stripped)
                    return True
                except (BrokenPipeError, OSError):
                    pass
        # Nothing running — run as command in cwd
        self.run_sub_command("command", stripped)
        return True

    def send_interrupt(self) -> None:
        target = None
        if self.sub_proc and self.sub_proc.poll() is None:
            target = self.sub_proc
        elif self.alive and self.proc:
            target = self.proc
        if target is None:
            self.log("[launcher] nothing running to interrupt")
            return
        self.log("[launcher] sending Ctrl+C (SIGINT)…")
        if IS_WINDOWS:
            try:
                target.send_signal(signal.CTRL_C_EVENT)
            except Exception:
                pass
        else:
            try:
                os.killpg(os.getpgid(target.pid), signal.SIGINT)
            except OSError:
                try:
                    target.send_signal(signal.SIGINT)
                except OSError:
                    pass

    # ── start / stop ──────────────────────────────────────────────────────────

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
            self.log(f"[launcher] warning: port {self.port} already in use")
        env = _get_project_env(self.cwd, self.env_extra)
        self.stopping = False
        self._started_at = None
        self.set_state("starting")
        self.log(f"[launcher] {self.cwd}")
        self.log("[launcher] > " + format_command(self.args))
        run_args = resolve_command(self.args, self.cwd)
        try:
            self.proc = subprocess.Popen(
                run_args, cwd=str(self.cwd), env=env,
                stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                stdin=subprocess.PIPE, **_proc_kwargs(hidden=True, session=True),
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
        self.log("[launcher] running stop command: " + self.stop_command)
        args = resolve_command(split_command(self.stop_command), self.cwd)
        env = os.environ.copy()
        env.update(self.env_extra)
        try:
            proc = subprocess.Popen(
                args, cwd=str(self.cwd), env=env,
                stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                stdin=subprocess.DEVNULL, **_proc_kwargs(hidden=True, session=True),
            )
            code = stream(proc, self.log)
            self.log(f"[launcher] stop command exited with code {code}")
        except OSError as exc:
            self.log("[launcher] stop command failed: " + str(exc))
        finally:
            self._terminate()

    def _terminate(self) -> None:
        if not self.alive:
            return
        pid = self.proc.pid
        if self.proc and self.proc.stdin:
            try:
                self.proc.stdin.close()
            except OSError:
                pass
        self.log(f"[launcher] stopping pid {pid}…")
        threading.Thread(target=kill_process_tree, args=(pid,), daemon=True).start()

    def _pump(self) -> None:
        self._finish(stream(self.proc, self.log))

    def _finish(self, code: int) -> None:
        self.log(f"[launcher] process exited with code {code}")
        self.proc = None
        self.cpu_pct = 0.0
        self.ram_mb  = 0.0
        self._prev_ticks = 0
        self._prev_tick_time = 0.0
        self.uptime_secs = 0
        self.set_state("stopped" if self.stopping or code == 0 else "error")
        self.stopping = False

    # ── resource monitoring ───────────────────────────────────────────────────

    def sample_stats(self) -> None:
        """Read CPU/RAM from /proc. Call from a background thread every ~2 s."""
        if not self.alive or self.proc is None:
            self.cpu_pct = 0.0
            self.ram_mb  = 0.0
            return
        try:
            pid = self.proc.pid
            now = time.monotonic()
            ticks, rss_kb = _tree_stats(pid)
            if self._prev_tick_time > 0:
                elapsed = now - self._prev_tick_time
                if elapsed > 0:
                    self.cpu_pct = max(0.0, ((ticks - self._prev_ticks) / (elapsed * HZ)) * 100.0)
            self._prev_ticks     = ticks
            self._prev_tick_time = now
            self.ram_mb = rss_kb / 1024.0
            if self._started_at is not None:
                self.uptime_secs = int(time.time() - self._started_at)
        except Exception:
            pass


# ══════════════════════════════════════════════════════════════════════════════
# UI Widgets
# ══════════════════════════════════════════════════════════════════════════════

class GroupHeader(Widget):
    """Section header for a group of servers with group start/stop buttons."""

    class StartGroup(Message):
        def __init__(self, group: str) -> None:
            self.group = group
            super().__init__()

    class StopGroup(Message):
        def __init__(self, group: str) -> None:
            self.group = group
            super().__init__()

    DEFAULT_CSS = ""

    def __init__(self, group_name: str, count: int, **kwargs) -> None:
        super().__init__(**kwargs)
        self.group_name = group_name
        self.count = count

    def compose(self) -> ComposeResult:
        with Horizontal(classes="grp-header-box"):
            yield Static(f"🏷  {self.group_name.upper()} ({self.count})", classes="grp-title")
            yield Button("▶ Start", id="btn-grp-start", classes="btn-grp-start")
            yield Button("⏹ Stop",  id="btn-grp-stop",  classes="btn-grp-stop")

    @on(Button.Pressed, "#btn-grp-start")
    def _on_start(self, event: Button.Pressed) -> None:
        event.stop()
        self.post_message(self.StartGroup(self.group_name))

    @on(Button.Pressed, "#btn-grp-stop")
    def _on_stop(self, event: Button.Pressed) -> None:
        event.stop()
        self.post_message(self.StopGroup(self.group_name))


class ServerCard(Widget):
    """One card per service in the left panel."""

    # Custom messages so the App can handle edit/delete
    class ToggleServer(Message):
        def __init__(self, svc: Service) -> None:
            self.svc = svc
            super().__init__()

    class EditServer(Message):
        def __init__(self, svc: Service) -> None:
            self.svc = svc
            super().__init__()

    class DeleteServer(Message):
        def __init__(self, svc: Service) -> None:
            self.svc = svc
            super().__init__()

    class SelectServer(Message):
        def __init__(self, svc: Service) -> None:
            self.svc = svc
            super().__init__()

    class RunAction(Message):
        def __init__(self, svc: Service, label: str, command: str) -> None:
            self.svc     = svc
            self.label   = label
            self.command = command
            super().__init__()

    DEFAULT_CSS = ""

    def __init__(self, svc: Service, **kwargs) -> None:
        super().__init__(**kwargs)
        self.svc = svc

    def compose(self) -> ComposeResult:
        svc = self.svc
        state = svc.state
        with Vertical(classes="card-inner"):
            # Row 1: dot  name  group  port  state
            with Horizontal(classes="card-row1"):
                yield Static(DOT_CHAR.get(state, "○"),
                             id="dot", classes=f"dot dot-{state}")
                yield Static(svc.name, classes="svc-name")
                if svc.group:
                    yield Static(svc.group, classes=f"grp-tag grp-{svc.group.lower()}")
                if svc.port:
                    yield Static(f":{svc.port}", classes="port-badge")
                yield Static(
                    LABEL_MAP.get(state, state),
                    id="state-lbl", classes=f"state-lbl status-{state}",
                )
            # Row 2: stats (cpu / ram / uptime)
            yield Static("", id="stats-lbl", classes="stats-lbl")
            # Row 3: controls
            with Horizontal(classes="card-controls"):
                is_running = state in ("running", "starting")
                yield Button(
                    "Stop" if is_running else "Start",
                    id="toggle-btn",
                    classes="btn-stop" if is_running else "btn-start",
                )
                if svc.custom:
                    yield Button("Edit",   id="edit-btn",   classes="btn-edit")
                    yield Button("Delete", id="delete-btn", classes="btn-delete")
            # Row 4: custom action buttons
            if svc.custom_actions:
                with Horizontal(classes="action-row"):
                    for idx, (lbl, _cmd) in enumerate(svc.custom_actions):
                        yield Button(lbl, id=f"action-{idx}", classes="btn-action")

    @on(Button.Pressed, "#toggle-btn")
    def _toggle(self, event: Button.Pressed) -> None:
        event.stop()
        self.post_message(self.ToggleServer(self.svc))

    @on(Button.Pressed, "#edit-btn")
    def _edit(self, event: Button.Pressed) -> None:
        event.stop()
        self.post_message(self.EditServer(self.svc))

    @on(Button.Pressed, "#delete-btn")
    def _delete(self, event: Button.Pressed) -> None:
        event.stop()
        self.post_message(self.DeleteServer(self.svc))

    @on(Button.Pressed)
    def _action_btn(self, event: Button.Pressed) -> None:
        bid = str(event.button.id or "")
        if bid.startswith("action-"):
            idx = int(bid[7:])
            if idx < len(self.svc.custom_actions):
                lbl, cmd = self.svc.custom_actions[idx]
                self.post_message(self.RunAction(self.svc, lbl, cmd))
                event.stop()

    def on_click(self) -> None:
        self.post_message(self.SelectServer(self.svc))

    # ── update helpers ────────────────────────────────────────────────────────

    def refresh_state(self) -> None:
        """Called from App when service state changes."""
        svc   = self.svc
        state = svc.state
        is_running = state in ("running", "starting")
        try:
            dot = self.query_one("#dot", Static)
            dot.update(DOT_CHAR.get(state, "○"))
            dot.remove_class("dot-stopped", "dot-starting", "dot-running",
                             "dot-stopping", "dot-error")
            dot.add_class(f"dot-{state}")

            lbl = self.query_one("#state-lbl", Static)
            lbl.update(LABEL_MAP.get(state, state))
            lbl.remove_class("status-stopped", "status-starting", "status-running",
                             "status-stopping", "status-error")
            lbl.add_class(f"status-{state}")

            btn = self.query_one("#toggle-btn", Button)
            btn.label   = "Stop" if is_running else "Start"
            btn.disabled = (state == "stopping")
            btn.remove_class("btn-start", "btn-stop")
            btn.add_class("btn-stop" if is_running else "btn-start")
        except Exception:
            pass

    def refresh_stats(self) -> None:
        """Called from App when CPU/RAM poll finishes."""
        svc = self.svc
        try:
            lbl = self.query_one("#stats-lbl", Static)
            if svc.alive:
                uptime = _fmt_uptime(svc.uptime_secs) if svc.uptime_secs else "0s"
                lbl.update(
                    f"CPU {svc.cpu_pct:4.1f}%   RAM {svc.ram_mb:4.0f} MB   Running for {uptime}"
                )
            else:
                lbl.update("")
        except Exception:
            pass


# ─────────────────────────────────────────────────────────────────────────────

class ServerLogPane(Widget):
    """Log view + interactive input bar for one service."""

    DEFAULT_CSS = ""

    def __init__(self, svc: Service, **kwargs) -> None:
        super().__init__(**kwargs)
        self.svc      = svc
        self._history: list[str] = []
        self._hist_idx = 0

    def compose(self) -> ComposeResult:
        yield RichLog(
            id="log", markup=False, highlight=False,
            wrap=False, auto_scroll=True,
        )
        with Horizontal(id="input-bar"):
            yield Static("❯", classes="prompt-char")
            yield Input(
                placeholder="Run command or send input…",
                id="cmd-input",
            )
            yield Button("Send",   id="btn-send",   classes="btn-send")
            yield Button("^C",     id="btn-ctrlc",  classes="btn-ctrlc")
            yield Button("Clear",  id="btn-clear",  classes="btn-clear")

    def append_log(self, line: str) -> None:
        try:
            rlog = self.query_one("#log", RichLog)
            if line.startswith("[launcher]"):
                rlog.write(Text(line, style="bold #7c8aff"))
            elif line.startswith("> "):
                rlog.write(Text(line, style="bold #00d2a0"))
            elif any(kw in line.lower() for kw in ("error:", "traceback", "exception")):
                rlog.write(Text(line, style="#e17055"))
            elif any(kw in line.lower() for kw in ("warning:", "warn:")):
                rlog.write(Text(line, style="#fdcb6e"))
            else:
                rlog.write(line)
        except Exception:
            pass

    def _do_send(self) -> None:
        inp = self.query_one("#cmd-input", Input)
        text = inp.value.strip()
        if not text:
            return
        self.svc.send_input(text)
        if not self._history or self._history[-1] != text:
            self._history.append(text)
        self._hist_idx = len(self._history)
        inp.value = ""

    @on(Button.Pressed, "#btn-send")
    def _send(self, event: Button.Pressed) -> None:
        event.stop()
        self._do_send()

    @on(Button.Pressed, "#btn-ctrlc")
    def _ctrlc(self, event: Button.Pressed) -> None:
        event.stop()
        self.svc.send_interrupt()

    @on(Button.Pressed, "#btn-clear")
    def _clear(self, event: Button.Pressed) -> None:
        event.stop()
        try:
            self.query_one("#log", RichLog).clear()
        except Exception:
            pass

    @on(Input.Submitted, "#cmd-input")
    def _submitted(self) -> None:
        self._do_send()

    def on_key(self, event: events.Key) -> None:
        try:
            inp = self.query_one("#cmd-input", Input)
        except Exception:
            return
        if not inp.has_focus:
            return
        if event.key == "up":
            if self._hist_idx > 0:
                self._hist_idx -= 1
                inp.value = self._history[self._hist_idx]
                inp.cursor_position = len(inp.value)
            event.stop()
        elif event.key == "down":
            if self._hist_idx < len(self._history) - 1:
                self._hist_idx += 1
                inp.value = self._history[self._hist_idx]
            else:
                self._hist_idx = len(self._history)
                inp.value = ""
            inp.cursor_position = len(inp.value)
            event.stop()


# ─────────────────────────────────────────────────────────────────────────────

class OverviewPane(Widget):
    """Summary table of managed servers + external listeners."""

    DEFAULT_CSS = ""

    def compose(self) -> ComposeResult:
        with VerticalScroll():
            yield Static("  Managed Servers", classes="ov-section-title")
            yield DataTable(id="managed-tbl", cursor_type="row")
            yield Static("  Other Listeners on This PC", classes="ov-section-title")
            yield DataTable(id="external-tbl", cursor_type="row")
            yield Button("⟳  Refresh", id="btn-refresh-ov", classes="btn-edit")

    def on_mount(self) -> None:
        mt = self.query_one("#managed-tbl", DataTable)
        mt.add_columns("", "Name", "Port", "Command", "CPU", "RAM", "Uptime")
        et = self.query_one("#external-tbl", DataTable)
        et.add_columns("Port", "PID", "Process")
        self.refresh_data()

    @on(Button.Pressed, "#btn-refresh-ov")
    def _refresh(self, event: Button.Pressed) -> None:
        event.stop()
        self.refresh_data()

    def refresh_data(self) -> None:
        app = self.app  # type: ServerLauncherApp  # noqa: F821
        mt = self.query_one("#managed-tbl", DataTable)
        mt.clear()
        for svc in app.services.values():
            state = svc.state
            dot   = Text(DOT_CHAR.get(state, "○"), style=STATE_COLOR.get(state, "white"))
            name  = Text(svc.name,  style="bold white")
            port  = f":{svc.port}" if svc.port else "—"
            cmd   = format_command(svc.args)[:28]
            cpu   = f"{svc.cpu_pct:.1f}%" if svc.alive else "—"
            ram   = f"{svc.ram_mb:.0f} MB" if svc.alive else "—"
            up    = _fmt_uptime(svc.uptime_secs) if svc.alive and svc.uptime_secs else "—"
            mt.add_row(dot, name, port, cmd, cpu, ram, up, key=svc.key)

        threading.Thread(target=self._bg_external_scan, daemon=True).start()

    def _bg_external_scan(self) -> None:
        try:
            listeners = scan_listening_servers()
            app = self.app  # type: ServerLauncherApp  # noqa: F821
            managed_pids = {
                svc.proc.pid
                for svc in app.services.values()
                if svc.alive and svc.proc
            }
            external = [
                e for e in listeners
                if not e["pid"] or e["pid"] not in managed_pids
            ]
            self.app.call_from_thread(self._apply_external, external)
        except Exception:
            pass

    def _apply_external(self, entries: list[dict]) -> None:
        try:
            et = self.query_one("#external-tbl", DataTable)
            et.clear()
            for e in entries:
                et.add_row(f":{e['port']}", str(e["pid"] or "—"), e["name"])
            if not entries:
                et.add_row("No unmanaged listeners found", "", "")
        except Exception:
            pass


# ══════════════════════════════════════════════════════════════════════════════
# Modal Screens
# ══════════════════════════════════════════════════════════════════════════════

class ConfirmModal(ModalScreen[bool]):
    """Simple yes/no confirmation dialog."""

    BINDINGS = [Binding("escape", "dismiss_no", "Cancel")]

    def __init__(self, message: str, **kwargs) -> None:
        super().__init__(**kwargs)
        self._message = message

    def compose(self) -> ComposeResult:
        with Container(id="confirm-container"):
            yield Static(self._message, id="confirm-msg")
            with Horizontal(id="confirm-btns"):
                yield Button("Cancel", id="btn-no",  classes="btn-cancel")
                yield Button("Delete", id="btn-yes", classes="btn-delete")

    @on(Button.Pressed, "#btn-no")
    def _no(self, event: Button.Pressed) -> None:
        event.stop()
        self.dismiss(False)

    @on(Button.Pressed, "#btn-yes")
    def _yes(self, event: Button.Pressed) -> None:
        event.stop()
        self.dismiss(True)

    def action_dismiss_no(self) -> None:
        self.dismiss(False)


# ─────────────────────────────────────────────────────────────────────────────

SERVER_TEMPLATES = [
    {
        "name": "React (Vite)",
        "command": "npm run dev",
        "port": 5173,
        "group": "Frontend",
        "actions": [{"label": "Build", "command": "npm run build"}, {"label": "Install", "command": "npm install"}],
        "links": [{"label": "Localhost", "url": "http://localhost:5173"}],
    },
    {
        "name": "Next.js",
        "command": "npm run dev",
        "port": 3000,
        "group": "Frontend",
        "actions": [{"label": "Build", "command": "npm run build"}, {"label": "Install", "command": "npm install"}],
        "links": [{"label": "Localhost", "url": "http://localhost:3000"}],
    },
    {
        "name": "FastAPI (Uvicorn)",
        "command": "uvicorn main:app --reload --port 8000",
        "port": 8000,
        "group": "Backend",
        "actions": [{"label": "Install reqs", "command": "pip install -r requirements.txt"}],
        "links": [{"label": "Swagger Docs", "url": "http://localhost:8000/docs"}],
    },
    {
        "name": "Flask",
        "command": "flask run --port 5000 --debug",
        "port": 5000,
        "group": "Backend",
        "env": {"FLASK_ENV": "development", "FLASK_DEBUG": "1"},
        "actions": [{"label": "Install reqs", "command": "pip install -r requirements.txt"}],
        "links": [{"label": "Localhost", "url": "http://localhost:5000"}],
    },
    {
        "name": "Django",
        "command": "python manage.py runserver 0.0.0.0:8000",
        "port": 8000,
        "group": "Backend",
        "actions": [{"label": "Migrate", "command": "python manage.py migrate"}],
        "links": [{"label": "Admin Panel", "url": "http://localhost:8000/admin"}],
    },
    {
        "name": "Node / Express",
        "command": "node server.js",
        "port": 3000,
        "group": "Backend",
        "actions": [{"label": "Install", "command": "npm install"}],
        "links": [{"label": "Localhost", "url": "http://localhost:3000"}],
    },
    {
        "name": "PostgreSQL",
        "command": "postgres -D ./data",
        "stop_command": "pg_ctl stop -D ./data",
        "port": 5432,
        "group": "DB",
    },
    {
        "name": "Redis",
        "command": "redis-server",
        "stop_command": "redis-cli shutdown",
        "port": 6379,
        "group": "DB",
    },
    {
        "name": "MongoDB",
        "command": "mongod --dbpath ./data/db",
        "port": 27017,
        "group": "DB",
    },
]


class AddServerModal(ModalScreen[dict | None]):
    """Add or edit a server configuration."""

    BINDINGS = [Binding("escape", "cancel", "Cancel")]

    def __init__(
        self, existing_keys: set[str], edit: dict | None = None, **kwargs
    ) -> None:
        super().__init__(**kwargs)
        self._existing = existing_keys
        self._edit     = edit or {}

    def compose(self) -> ComposeResult:
        e = self._edit
        with Container(id="modal-overlay"):
            with VerticalScroll(id="modal-box"):
                yield Static(
                    "Edit Server" if e else "Add New Server",
                    classes="modal-title",
                )
                yield Static(
                    "Saved to servers.json – no code changes needed.",
                    classes="modal-subtitle",
                )

                if not e:
                    yield Static("⚡ Pick a Template (Optional)", classes="field-label")
                    tpl_options = [(t["name"], f"tpl_{idx}") for idx, t in enumerate(SERVER_TEMPLATES)]
                    yield Select(
                        tpl_options,
                        prompt="-- Select a template (React, Next.js, Flask, etc.) --",
                        id="sel-template"
                    )

                yield Static("Server name *", classes="field-label")
                yield Input(value=e.get("name", ""),
                            placeholder="My Server", id="inp-name")

                yield Static("Group  (Frontend / Backend / DB)", classes="field-label")
                yield Static(
                    "Tag servers for group start/stop. e.g. Frontend, Backend, DB",
                    classes="field-hint",
                )
                yield Input(value=e.get("group", "General"),
                            placeholder="Frontend / Backend / DB", id="inp-group")

                yield Static("Working directory *", classes="field-label")
                yield Input(value=e.get("cwd", ""),
                            placeholder="/path/to/project", id="inp-cwd")

                yield Static("Start command *", classes="field-label")
                yield Static(
                    "e.g.  npm run dev  |  python app.py  |  uvicorn main:app --reload",
                    classes="field-hint",
                )
                yield Input(value=e.get("command", ""),
                            placeholder="npm run dev", id="inp-cmd")

                yield Static("Stop command  (optional)", classes="field-label")
                yield Static(
                    "Leave empty for a plain kill.  e.g. firebase emulators:export ./data",
                    classes="field-hint",
                )
                yield Input(value=e.get("stop_command", ""),
                            placeholder="", id="inp-stopcmd")

                yield Static("Port  (0 = none)", classes="field-label")
                yield Input(value=str(e.get("port", 0)),
                            placeholder="3000", id="inp-port")

                yield Static(
                    "Environment variables  (KEY=VALUE, one per line)",
                    classes="field-label",
                )
                env_txt = "\n".join(
                    f"{k}={v}" for k, v in e.get("env", {}).items()
                )
                yield TextArea(env_txt, id="inp-env")

                yield Static(
                    "Action buttons  (Label: command, one per line)",
                    classes="field-label",
                )
                yield Static(
                    "e.g.  Build: npm run build",
                    classes="field-hint",
                )
                act_txt = "\n".join(
                    f"{a.get('label','')}: {a.get('command','')}"
                    for a in e.get("actions", [])
                )
                yield TextArea(act_txt, id="inp-actions")

                yield Static(
                    "Open links  (Label: url, one per line)",
                    classes="field-label",
                )
                yield Static(
                    "e.g.  App: http://localhost:3000",
                    classes="field-hint",
                )
                link_txt = "\n".join(
                    f"{l.get('label','')}: {l.get('url','')}"
                    for l in e.get("links", [])
                )
                yield TextArea(link_txt, id="inp-links")

                with Horizontal(classes="modal-btns"):
                    yield Button("Cancel", id="btn-cancel", classes="btn-cancel")
                    yield Button("Save",   id="btn-save",   classes="btn-save")

    @on(Select.Changed, "#sel-template")
    def _template_changed(self, event: Select.Changed) -> None:
        if event.value is None or event.value is Select.BLANK:
            return
        val_str = str(event.value)
        if not val_str.startswith("tpl_"):
            return
        try:
            tpl_idx = int(val_str[4:])
            if 0 <= tpl_idx < len(SERVER_TEMPLATES):
                t = SERVER_TEMPLATES[tpl_idx]
                name_inp = self.query_one("#inp-name", Input)
                if not name_inp.value.strip():
                    name_inp.value = t.get("name", "")
                self.query_one("#inp-cmd", Input).value = t.get("command", "")
                self.query_one("#inp-stopcmd", Input).value = t.get("stop_command", "")
                self.query_one("#inp-port", Input).value = str(t.get("port", 0))

                if "group" in t:
                    self.query_one("#inp-group", Input).value = t.get("group", "General")

                if "env" in t:
                    env_lines = "\n".join(f"{k}={v}" for k, v in t["env"].items())
                    self.query_one("#inp-env", TextArea).text = env_lines

                if "actions" in t:
                    act_lines = "\n".join(f"{a['label']}: {a['command']}" for a in t["actions"])
                    self.query_one("#inp-actions", TextArea).text = act_lines

                if "links" in t:
                    link_lines = "\n".join(f"{l['label']}: {l['url']}" for l in t["links"])
                    self.query_one("#inp-links", TextArea).text = link_lines
        except Exception:
            pass

    @on(Button.Pressed, "#btn-cancel")
    def _cancel_btn(self, event: Button.Pressed) -> None:
        event.stop()
        self.dismiss(None)

    def action_cancel(self) -> None:
        self.dismiss(None)

    @on(Button.Pressed, "#btn-save")
    def _save_btn(self, event: Button.Pressed) -> None:
        event.stop()
        name      = self.query_one("#inp-name",    Input).value.strip()
        group     = self.query_one("#inp-group",   Input).value.strip() or "General"
        cwd       = self.query_one("#inp-cwd",     Input).value.strip()
        cmd       = self.query_one("#inp-cmd",     Input).value.strip()
        stop_cmd  = self.query_one("#inp-stopcmd", Input).value.strip()
        port_str  = self.query_one("#inp-port",    Input).value.strip()
        env_text  = self.query_one("#inp-env",     TextArea).text
        act_text  = self.query_one("#inp-actions", TextArea).text
        link_text = self.query_one("#inp-links",   TextArea).text

        if not name:
            self.notify("Server name is required.", severity="error"); return
        if not cwd:
            self.notify("Working directory is required.", severity="error"); return
        if not cmd:
            self.notify("Start command is required.", severity="error"); return
        if not Path(cwd).is_dir():
            self.notify(f"Directory not found:\n{cwd}", severity="error"); return

        try:
            port = int(port_str or "0")
        except ValueError:
            port = 0

        env: dict[str, str] = {}
        for line in env_text.strip().splitlines():
            line = line.strip()
            if "=" in line:
                k, v = line.split("=", 1)
                env[k.strip()] = v.strip()

        actions: list[dict] = []
        for line in act_text.strip().splitlines():
            line = line.strip()
            if ":" in line:
                lbl, rest = line.split(":", 1)
                if lbl.strip() and rest.strip():
                    actions.append({"label": lbl.strip(), "command": rest.strip()})

        links: list[dict] = []
        for line in link_text.strip().splitlines():
            line = line.strip()
            if ":" in line:
                lbl, rest = line.split(":", 1)
                if lbl.strip() and rest.strip():
                    links.append({"label": lbl.strip(), "url": rest.strip()})

        # Generate / reuse key
        key = self._edit.get("key") if self._edit else None
        if not key:
            key = re.sub(r"[^a-z0-9]+", "_", name.lower()).strip("_") or "server"
            base, i = key, 2
            while key in self._existing:
                key = f"{base}_{i}"; i += 1

        self.dismiss({
            "key": key, "name": name, "cwd": cwd,
            "command": cmd, "stop_command": stop_cmd,
            "port": port, "group": group,
            "env": env, "actions": actions, "links": links,
        })


# ─────────────────────────────────────────────────────────────────────────────

class CommandPaletteModal(ModalScreen[tuple | None]):
    """Ctrl+P quick-launch palette."""

    BINDINGS = [Binding("escape", "cancel", "Close")]

    def __init__(self, services: dict, **kwargs) -> None:
        super().__init__(**kwargs)
        self._services = services
        self._items: list[tuple] = []   # (display_text, action, svc_key, *extra)

    def compose(self) -> ComposeResult:
        with Container(id="pal-overlay"):
            with Vertical(id="pal-box"):
                yield Static("⚡  Quick Launch", classes="pal-title")
                yield Input(placeholder="Search servers and actions…", id="pal-input")
                yield VerticalScroll(id="pal-results")

    def on_mount(self) -> None:
        self._populate("")
        self.query_one("#pal-input", Input).focus()

    @on(Input.Changed, "#pal-input")
    def _filter(self, event: Input.Changed) -> None:
        self._populate(event.value)

    def _populate(self, q: str) -> None:
        q = q.lower()
        items: list[tuple] = []
        for svc in self._services.values():
            grp_name = svc.group or "General"
            match = not q or q in svc.name.lower() or q in grp_name.lower()
            if match:
                grp_tag = f" [{grp_name}]" if svc.group else ""
                if svc.alive:
                    items.append(("⏹  Stop  " + svc.name + grp_tag, "stop", svc.key))
                else:
                    items.append(("▶  Start " + svc.name + grp_tag, "start", svc.key))
            for lbl, cmd in svc.custom_actions:
                if not q or q in lbl.lower() or q in svc.name.lower() or q in grp_name.lower():
                    items.append((f"⚡  {lbl}  [{svc.name}]", "action", svc.key, cmd, lbl))

        # Group-level actions
        groups_seen = set()
        for svc in self._services.values():
            grp = getattr(svc, "group", "General") or "General"
            if grp in groups_seen:
                continue
            groups_seen.add(grp)
            if not q or q in grp.lower() or "group" in q:
                items.append((f"▶  Start Group: {grp}", "start_group", grp))
                items.append((f"⏹  Stop Group: {grp}", "stop_group", grp))

        self._items = items
        results = self.query_one("#pal-results", VerticalScroll)
        results.remove_children()
        for idx, item in enumerate(items):
            btn = Button(item[0], id=f"pal-{idx}", classes="pal-item")
            results.mount(btn)

    @on(Button.Pressed)
    def _selected(self, event: Button.Pressed) -> None:
        bid = str(event.button.id or "")
        if bid.startswith("pal-"):
            idx = int(bid[4:])
            if idx < len(self._items):
                event.stop()
                self.dismiss(self._items[idx])

    @on(Input.Submitted, "#pal-input")
    def _submitted(self) -> None:
        if self._items:
            self.dismiss(self._items[0])

    def on_key(self, event: events.Key) -> None:
        inp = self.query_one("#pal-input", Input)
        if not inp.has_focus:
            return
        if event.key == "down":
            buttons = list(self.query(".pal-item"))
            if buttons:
                buttons[0].focus()
                event.stop()
        elif event.key == "escape":
            self.dismiss(None)
            event.stop()

    def action_cancel(self) -> None:
        self.dismiss(None)


# ══════════════════════════════════════════════════════════════════════════════
# Main App
# ══════════════════════════════════════════════════════════════════════════════

class ServerLauncherApp(App):
    TITLE    = "Server Launcher"
    CSS_PATH = None       # inline CSS below

    BINDINGS = [
        Binding("ctrl+p", "command_palette",  "Quick Launch"),
        Binding("ctrl+a", "start_all",         "Start All"),
        Binding("ctrl+x", "stop_all",          "Stop All"),
        Binding("ctrl+r", "refresh_overview",  "Refresh Overview"),
        Binding("ctrl+q", "quit",              "Quit"),
    ]

    CSS = """
/* ── Base ──────────────────────────────────────────────────────────────── */
Screen { background: #0f1117; }

/* ── Header ─────────────────────────────────────────────────────────────── */
#header-bar {
    dock: top;
    height: 1;
    background: #1a1d27;
    color: #7b8394;
    padding: 0 2;
    text-align: center;
    border-bottom: solid #2a2e3a;
}

/* ── Body ───────────────────────────────────────────────────────────────── */
#body { height: 1fr; }

/* ── Left Panel ─────────────────────────────────────────────────────────── */
#left-panel {
    width: 44;
    min-width: 38;
    background: #0f1117;
    border-right: solid #2a2e3a;
}
#server-scroll { height: 1fr; padding: 1 1 0 1; }
#add-server-btn {
    dock: bottom;
    margin: 1;
    background: #6c5ce7;
    color: white;
    border: none;
    width: 1fr;
    text-style: bold;
}
#add-server-btn:hover { background: #7f70f0; }

/* ── Group Header ────────────────────────────────────────────────────────── */
GroupHeader {
    height: auto;
    margin-top: 1;
    margin-bottom: 0;
    padding: 0;
}
.grp-header-box {
    height: 1;
    width: 1fr;
    background: #141722;
    padding: 0 1;
    align: left middle;
}
.grp-title {
    width: 1fr;
    color: #8c7ae6;
    text-style: bold;
}
.btn-grp-start {
    background: #008f68;
    color: white;
    border: none;
    min-width: 7;
    height: 1;
    text-style: bold;
    padding: 0 1;
}
.btn-grp-start:hover { background: #00b894; }
.btn-grp-stop {
    background: #e17055;
    color: white;
    border: none;
    min-width: 6;
    height: 1;
    margin-left: 1;
    text-style: bold;
    padding: 0 1;
}
.btn-grp-stop:hover { background: #d04030; }

.grp-tag {
    background: #252838;
    color: #a4b0be;
    text-style: bold;
    width: auto;
    padding: 0 1;
    margin-right: 1;
}
.grp-frontend { color: #00cec9; background: #132f38; }
.grp-backend  { color: #a29bfe; background: #2a2040; }
.grp-db       { color: #fdcb6e; background: #3a2818; }

/* ── Right Panel ─────────────────────────────────────────────────────────── */
#right-panel { width: 1fr; }
#main-tabs { dock: top; }
#content-switcher { height: 1fr; }

/* ── Server Card ─────────────────────────────────────────────────────────── */
ServerCard {
    height: auto;
    background: #1a1d27;
    border: solid #2a2e3a;
    margin: 0 0 1 0;
    padding: 1 1;
}
ServerCard:hover { border: solid #6c5ce7; }

.card-inner { height: auto; }

.card-row1 {
    height: 1;
    width: 1fr;
    align: left middle;
}
.dot { width: 2; }
.svc-name { color: #e4e7ee; text-style: bold; width: 1fr; padding-left: 1; }
.port-badge {
    background: #252838;
    color: #6c5ce7;
    text-style: bold;
    width: auto;
    padding: 0 1;
    margin-right: 1;
}
.state-lbl { width: auto; text-align: right; }
.stats-lbl { color: #a0abbd; height: 1; margin: 0 0 0 3; }

.card-controls { height: 1; margin-top: 1; }
.action-row { height: 1; margin-top: 1; }

/* ── Dot colors ─────────────────────────────────────────────────────────── */
.dot-stopped  { color: #555d6e; }
.dot-running  { color: #00b894; }
.dot-starting { color: #fdcb6e; }
.dot-stopping { color: #fdcb6e; }
.dot-error    { color: #e17055; }

/* ── Status text colors ──────────────────────────────────────────────────── */
.status-stopped  { color: #555d6e; }
.status-running  { color: #00b894; }
.status-starting { color: #fdcb6e; }
.status-stopping { color: #fdcb6e; }
.status-error    { color: #e17055; }

/* ── Buttons ─────────────────────────────────────────────────────────────── */
.btn-start  { background: #00b894; color: white; border: none; min-width: 7; height: 1; }
.btn-start:hover { background: #00d2a0; }
.btn-stop   { background: #e17055; color: white; border: none; min-width: 7; height: 1; }
.btn-stop:hover  { background: #d04030; }
.btn-edit   { background: #252830; color: #6c5ce7; border: none; min-width: 5; height: 1; margin-left: 1; }
.btn-edit:hover  { background: #2f3340; }
.btn-delete { background: #252830; color: #e17055; border: none; min-width: 7; height: 1; margin-left: 1; }
.btn-delete:hover { background: #2f3340; }
.btn-action { background: #252830; color: #6c5ce7; border: none; height: 1; margin-right: 1; }
.btn-action:hover { background: #2f3340; }
.btn-send   { background: #6c5ce7; color: white; border: none; min-width: 6; height: 1; margin-left: 1; }
.btn-send:hover { background: #7f70f0; }
.btn-ctrlc  { background: #252830; color: #fdcb6e; border: none; min-width: 4; height: 1; margin-left: 1; }
.btn-ctrlc:hover { background: #2f3340; }
.btn-clear  { background: #252830; color: #7b8394; border: none; min-width: 6; height: 1; margin-left: 1; }
.btn-clear:hover { background: #2f3340; }
.btn-cancel { background: #252830; color: #7b8394; border: none; min-width: 10; }
.btn-cancel:hover { background: #2f3340; }
.btn-save   { background: #6c5ce7; color: white; border: none; min-width: 10; text-style: bold; }
.btn-save:hover { background: #7f70f0; }

/* ── Log Pane ────────────────────────────────────────────────────────────── */
ServerLogPane { height: 1fr; }
#log {
    height: 1fr;
    background: #111318;
    color: #c8cdd8;
    padding: 0 1;
    scrollbar-background: #1a1d27;
    scrollbar-color: #2a2e3a;
}
#input-bar {
    height: 3;
    background: #1a1d27;
    border-top: solid #2a2e3a;
    padding: 0 1;
    align: left middle;
}
.prompt-char {
    color: #6c5ce7;
    text-style: bold;
    width: auto;
    margin-right: 1;
}
#cmd-input {
    background: #252830;
    color: #e4e7ee;
    border: solid #2a2e3a;
    height: 1;
    width: 1fr;
}
#cmd-input:focus { border: solid #6c5ce7; }

/* ── Overview Pane ───────────────────────────────────────────────────────── */
OverviewPane { height: 1fr; }
.ov-section-title {
    color: #e4e7ee;
    text-style: bold;
    padding: 1 0 0 0;
    margin-bottom: 1;
}
DataTable {
    background: #1a1d27;
    color: #c8cdd8;
    margin-bottom: 1;
    border: solid #2a2e3a;
}
DataTable > .datatable--header { background: #252830; color: #7b8394; }
DataTable > .datatable--cursor { background: #6c5ce7; color: white; }

/* ── Confirm Modal ───────────────────────────────────────────────────────── */
ConfirmModal { align: center middle; }
#confirm-container {
    background: #1a1d27;
    border: solid #e17055;
    width: 50;
    height: auto;
    padding: 2 3;
    align: center middle;
}
#confirm-msg  { color: #e4e7ee; text-align: center; margin-bottom: 2; }
#confirm-btns { align: center middle; height: auto; }

/* ── Add/Edit Server Modal ───────────────────────────────────────────────── */
AddServerModal { align: center middle; }
#modal-overlay {
    background: rgba(0, 0, 0, 0.85);
    width: 100%;
    height: 100%;
    align: center middle;
}
#modal-box {
    background: #1a1d27;
    border: solid #6c5ce7;
    width: 72;
    height: auto;
    max-height: 90vh;
    padding: 2 3;
}
.modal-title   { text-style: bold; color: #e4e7ee; text-align: center; margin-bottom: 0; }
.modal-subtitle { color: #7b8394; text-align: center; margin-bottom: 1; }
.field-label   { color: #e4e7ee; margin-top: 1; }
.field-hint    { color: #7b8394; }
.modal-btns    { margin-top: 2; align: right middle; height: 3; }

Input {
    background: #252830;
    border: solid #2a2e3a;
    color: #e4e7ee;
}
Input:focus { border: solid #6c5ce7; }

TextArea {
    background: #252830;
    border: solid #2a2e3a;
    color: #e4e7ee;
    height: 5;
}
TextArea:focus { border: solid #6c5ce7; }

/* ── Command Palette ─────────────────────────────────────────────────────── */
CommandPaletteModal { align: center top; }
#pal-overlay {
    background: rgba(0, 0, 0, 0.8);
    width: 100%;
    height: 100%;
    align: center top;
}
#pal-box {
    background: #1a1d27;
    border: solid #6c5ce7;
    width: 62;
    height: auto;
    max-height: 70vh;
    margin-top: 3;
    padding: 1 2;
}
.pal-title { color: #e4e7ee; text-style: bold; text-align: center; margin-bottom: 1; }
#pal-input {
    background: #252830;
    border: solid #6c5ce7;
    color: #e4e7ee;
    margin-bottom: 1;
}
#pal-results { height: auto; max-height: 55vh; }
.pal-item {
    background: #1a1d27;
    color: #e4e7ee;
    border: none;
    height: 1;
    width: 1fr;
    text-align: left;
}
.pal-item:hover { background: #6c5ce7; color: white; }
"""

    def __init__(self) -> None:
        super().__init__()
        self.events: queue.Queue  = queue.Queue()
        self.services: dict[str, Service] = {}
        self._load_services()

    # ── Service Management ────────────────────────────────────────────────────

    def _load_services(self) -> None:
        for entry in load_custom_servers():
            svc = self._make_service(entry)
            self.services[svc.key] = svc

    def _make_service(self, entry: dict) -> Service:
        key     = entry.get("key", "")
        name    = entry.get("name", key)
        cwd     = entry.get("cwd", ".")
        command = entry.get("command", "")
        port    = entry.get("port", 0)
        group   = entry.get("group", "General")
        env     = entry.get("env", {})
        args    = split_command(command) if command else (
            [os.environ.get("SHELL", "bash")] if not IS_WINDOWS else ["cmd"]
        )
        links   = [(a.get("label",""), a.get("url",""))
                   for a in entry.get("links", []) if a.get("url")]
        custom_actions = [
            (a.get("label",""), a.get("command",""))
            for a in entry.get("actions", [])
            if a.get("label") and a.get("command")
        ]
        return Service(
            self, key, name,
            f"{command}  (:{port})" if port else command,
            args, cwd, port,
            env=env, links=links, custom=True,
            custom_actions=custom_actions,
            stop_command=entry.get("stop_command", ""),
            group=group,
        )

    def _persist(self) -> None:
        save_custom_servers([
            {
                "key":          svc.key,
                "name":         svc.name,
                "cwd":          str(svc.cwd),
                "command":      format_command(svc.args),
                "stop_command": svc.stop_command,
                "port":         svc.port,
                "group":        svc.group,
                "env":          svc.env_extra,
                "actions":      [{"label": l, "command": c} for l, c in svc.custom_actions],
                "links":        [{"label": l, "url":     u} for l, u in svc.links],
            }
            for svc in self.services.values()
            if svc.custom
        ])

    # ── Compose ───────────────────────────────────────────────────────────────

    def compose(self) -> ComposeResult:
        yield Static(self._header_text(), id="header-bar")
        with Horizontal(id="body"):
            # ── Left: server list
            with Vertical(id="left-panel"):
                with VerticalScroll(id="server-scroll"):
                    grouped: dict[str, list[Service]] = {}
                    for svc in self.services.values():
                        grouped.setdefault(svc.group, []).append(svc)
                    for grp_name in sorted(grouped.keys()):
                        svcs = grouped[grp_name]
                        slug = re.sub(r"[^a-zA-Z0-9]+", "_", grp_name.lower())
                        yield GroupHeader(grp_name, len(svcs), id=f"grp-hdr-{slug}")
                        for svc in svcs:
                            yield ServerCard(svc, id=f"card-{svc.key}")
                yield Button("＋  Add Server", id="add-server-btn")
            # ── Right: tabs + content
            with Vertical(id="right-panel"):
                tabs_list = [Tab("Overview", id="tab-overview")]
                for svc in self.services.values():
                    tabs_list.append(Tab(svc.name, id=f"tab-{svc.key}"))
                yield Tabs(*tabs_list, id="main-tabs")
                with ContentSwitcher(initial="pane-overview", id="content-switcher"):
                    yield OverviewPane(id="pane-overview")
                    for svc in self.services.values():
                        yield ServerLogPane(svc, id=f"pane-{svc.key}")
        yield Footer()

    # ── Lifecycle ─────────────────────────────────────────────────────────────

    def on_mount(self) -> None:
        self.set_interval(0.10, self._drain_events)
        self.set_interval(1.50, self._poll_ports)
        self.set_interval(1.00, self._poll_stats)

    # ── Event Queue (bridge between threads and Textual) ──────────────────────

    def post(self, event: tuple) -> None:
        self.events.put(event)

    def _drain_events(self) -> None:
        try:
            while True:
                ev = self.events.get_nowait()
                if ev[0] == "log":
                    self._handle_log(ev[1], ev[2])
                elif ev[0] == "state":
                    self._handle_state(ev[1])
        except queue.Empty:
            pass

    def _handle_log(self, key: str, line: str) -> None:
        try:
            self.query_one(f"#pane-{key}", ServerLogPane).append_log(line)
        except Exception:
            pass

    def _handle_state(self, key: str) -> None:
        svc = self.services.get(key)
        if not svc:
            return
        if svc.state == "starting" and svc._started_at is None:
            svc._started_at = time.time()
        try:
            self.query_one(f"#card-{key}", ServerCard).refresh_state()
        except Exception:
            pass
        self.query_one("#header-bar", Static).update(self._header_text())

    def _poll_ports(self) -> None:
        for svc in self.services.values():
            if svc.state != "starting" or not svc.alive:
                continue
            if svc.port and port_open(svc.port):
                svc.set_state("running")
                continue
            if svc._started_at is None:
                svc._started_at = time.time()
            elif not svc.port or time.time() - svc._started_at >= 6.0:
                svc.set_state("running")

    def _poll_stats(self) -> None:
        def _worker() -> None:
            for svc in list(self.services.values()):
                svc.sample_stats()
            self.call_from_thread(self._apply_stats)

        threading.Thread(target=_worker, daemon=True).start()

    def _apply_stats(self) -> None:
        for svc in self.services.values():
            try:
                self.query_one(f"#card-{svc.key}", ServerCard).refresh_stats()
            except Exception:
                pass
        # Also refresh overview table if it's visible
        try:
            cs = self.query_one("#content-switcher", ContentSwitcher)
            if cs.current == "pane-overview":
                self.query_one("#pane-overview", OverviewPane).refresh_data()
        except Exception:
            pass

    # ── Header ────────────────────────────────────────────────────────────────

    def _header_text(self) -> str:
        running = sum(1 for s in self.services.values() if s.alive)
        total   = len(self.services)
        ip      = lan_ip()
        return (
            f"⚡ Server Launcher  │  {ip}  │  "
            f"{running}/{total} running  │  "
            "Ctrl+P: Quick Launch  │  Ctrl+A: Start All  │  Ctrl+X: Stop All  │  Ctrl+Q: Quit"
        )

    # ── Tab switching ─────────────────────────────────────────────────────────

    @on(Tabs.TabActivated)
    def _tab_activated(self, event: Tabs.TabActivated) -> None:
        if event.tab is None:
            return
        tab_id = str(event.tab.id or "")
        pane_id = "pane-overview" if tab_id == "tab-overview" else f"pane-{tab_id[4:]}"
        try:
            self.query_one("#content-switcher", ContentSwitcher).current = pane_id
        except Exception:
            pass

    def _switch_to(self, key: str) -> None:
        """Switch tabs to a service by key (or 'overview')."""
        pane_id = "pane-overview" if key == "overview" else f"pane-{key}"
        tab_id  = "tab-overview"  if key == "overview" else f"tab-{key}"
        try:
            self.query_one("#content-switcher", ContentSwitcher).current = pane_id
            self.query_one("#main-tabs", Tabs).active = tab_id
        except Exception:
            pass

    # ── Card messages ─────────────────────────────────────────────────────────

    @on(ServerCard.ToggleServer)
    def _toggle_server(self, event: ServerCard.ToggleServer) -> None:
        svc = event.svc
        if svc.alive:
            svc.stop()
        else:
            svc.start()

    @on(ServerCard.SelectServer)
    def _select_server(self, event: ServerCard.SelectServer) -> None:
        self._switch_to(event.svc.key)

    @on(ServerCard.RunAction)
    def _run_action(self, event: ServerCard.RunAction) -> None:
        event.svc.run_sub_command(event.label, event.command)
        self._switch_to(event.svc.key)

    @on(ServerCard.EditServer)
    async def _edit_server_msg(self, event: ServerCard.EditServer) -> None:
        await self._edit_server(event.svc)

    @on(ServerCard.DeleteServer)
    @on(GroupHeader.StartGroup)
    def _handle_start_group(self, event: GroupHeader.StartGroup) -> None:
        self.action_start_group(event.group)

    @on(GroupHeader.StopGroup)
    def _handle_stop_group(self, event: GroupHeader.StopGroup) -> None:
        self.action_stop_group(event.group)

    async def _delete_server_msg(self, event: ServerCard.DeleteServer) -> None:
        await self._delete_server(event.svc)

    async def _refresh_card_list(self) -> None:
        scroll = self.query_one("#server-scroll", VerticalScroll)
        await scroll.remove_children()
        grouped: dict[str, list[Service]] = {}
        for svc in self.services.values():
            grouped.setdefault(svc.group, []).append(svc)
        for grp_name in sorted(grouped.keys()):
            svcs = grouped[grp_name]
            slug = re.sub(r"[^a-zA-Z0-9]+", "_", grp_name.lower())
            await scroll.mount(GroupHeader(grp_name, len(svcs), id=f"grp-hdr-{slug}"))
            for svc in svcs:
                await scroll.mount(ServerCard(svc, id=f"card-{svc.key}"))

    # ── Add Server ────────────────────────────────────────────────────────────

    @on(Button.Pressed, "#add-server-btn")
    async def _add_server_btn(self, event: Button.Pressed) -> None:
        event.stop()
        existing = set(self.services.keys())

        async def _callback(result: dict | None) -> None:
            if result:
                await self._apply_add(result)

        await self.push_screen(AddServerModal(existing), _callback)

    async def _apply_add(self, r: dict) -> None:
        svc = self._make_service(r)
        self.services[svc.key] = svc

        await self._refresh_card_list()

        tabs = self.query_one("#main-tabs", Tabs)
        await tabs.add_tab(Tab(svc.name, id=f"tab-{svc.key}"))

        switcher = self.query_one("#content-switcher", ContentSwitcher)
        await switcher.mount(ServerLogPane(svc, id=f"pane-{svc.key}"))

        self._switch_to(svc.key)
        self._persist()
        self.query_one("#header-bar", Static).update(self._header_text())
        self.notify(f"Added  {svc.name}")

    # ── Edit Server ───────────────────────────────────────────────────────────

    async def _edit_server(self, svc: Service) -> None:
        if svc.alive:
            self.notify("Stop the server before editing it.", severity="warning")
            return
        existing = set(self.services.keys()) - {svc.key}
        entry = {
            "key": svc.key, "name": svc.name, "cwd": str(svc.cwd),
            "command": format_command(svc.args), "stop_command": svc.stop_command,
            "port": svc.port, "group": svc.group, "env": svc.env_extra,
            "actions": [{"label": l, "command": c} for l, c in svc.custom_actions],
            "links":   [{"label": l, "url":     u} for l, u in svc.links],
        }

        async def _callback(result: dict | None) -> None:
            if result:
                await self._apply_edit(svc, result)

        await self.push_screen(AddServerModal(existing, edit=entry), _callback)

    async def _apply_edit(self, old: Service, r: dict) -> None:
        new = self._make_service(r)
        old_key = old.key

        self.services.pop(old_key, None)
        self.services[new.key] = new

        await self._refresh_card_list()

        # Replace tab
        tabs = self.query_one("#main-tabs", Tabs)
        try: await tabs.remove_tab(f"tab-{old_key}")
        except Exception: pass
        await tabs.add_tab(Tab(new.name, id=f"tab-{new.key}"))

        # Replace log pane
        try: await self.query_one(f"#pane-{old_key}", ServerLogPane).remove()
        except Exception: pass
        switcher = self.query_one("#content-switcher", ContentSwitcher)
        await switcher.mount(ServerLogPane(new, id=f"pane-{new.key}"))

        self._switch_to(new.key)
        self._persist()
        self.notify(f"Saved  {new.name}")

    # ── Delete Server ─────────────────────────────────────────────────────────

    async def _delete_server(self, svc: Service) -> None:
        if svc.alive:
            self.notify("Stop the server before deleting it.", severity="warning")
            return

        async def _callback(confirmed: bool) -> None:
            if confirmed:
                await self._apply_delete(svc)

        await self.push_screen(
            ConfirmModal(f'Delete "{svc.name}"?\nThis cannot be undone.'),
            _callback,
        )

    async def _apply_delete(self, svc: Service) -> None:
        key = svc.key
        self.services.pop(key, None)
        await self._refresh_card_list()
        try: await self.query_one("#main-tabs", Tabs).remove_tab(f"tab-{key}")
        except Exception: pass
        try: await self.query_one(f"#pane-{key}", ServerLogPane).remove()
        except Exception: pass
        self._switch_to("overview")
        self._persist()
        self.notify(f"Deleted  {svc.name}")

    # ── Actions (key bindings) ────────────────────────────────────────────────

    def action_command_palette(self) -> None:
        def _callback(result: tuple | None) -> None:
            if not result:
                return
            action = result[1]
            key    = result[2]
            if action == "start_group":
                self.action_start_group(key)
                return
            elif action == "stop_group":
                self.action_stop_group(key)
                return
            svc    = self.services.get(key)
            if not svc:
                return
            if action == "start":
                svc.start(); self._switch_to(key)
            elif action == "stop":
                svc.stop()
            elif action == "action" and len(result) >= 5:
                svc.run_sub_command(result[4], result[3])
                self._switch_to(key)

        self.push_screen(CommandPaletteModal(self.services), _callback)

    def action_start_group(self, group: str) -> None:
        def _worker() -> None:
            for svc in list(self.services.values()):
                if svc.group == group and not svc.alive and not svc.missing:
                    svc.start()
                    time.sleep(0.5)
        threading.Thread(target=_worker, daemon=True).start()
        self.notify(f"Starting group: {group}…")

    def action_stop_group(self, group: str) -> None:
        for svc in self.services.values():
            if svc.group == group and svc.alive:
                svc.stop()
        self.notify(f"Stopping group: {group}…")

    def action_start_all(self) -> None:
        def _worker() -> None:
            for svc in list(self.services.values()):
                if not svc.alive and not svc.missing:
                    svc.start()
                    time.sleep(0.5)
        threading.Thread(target=_worker, daemon=True).start()
        self.notify("Starting all servers…")

    def action_stop_all(self) -> None:
        for svc in self.services.values():
            if svc.alive:
                svc.stop()
        self.notify("Stopping all servers…")

    def action_refresh_overview(self) -> None:
        try:
            self.query_one("#pane-overview", OverviewPane).refresh_data()
        except Exception:
            pass

    # ── Close ─────────────────────────────────────────────────────────────────

    async def action_quit(self) -> None:
        running = [s.name for s in self.services.values() if s.alive]
        if running:
            async def _callback(stop: bool) -> None:
                if stop:
                    for svc in self.services.values():
                        if svc.alive:
                            svc.stop()
                    await self.sleep(1.5)
                self.exit()
            await self.push_screen(
                ConfirmModal("Running servers:\n• " + "\n• ".join(running) + "\n\nStop them before closing?"),
                _callback,
            )
        else:
            self.exit()


# ══════════════════════════════════════════════════════════════════════════════
# Entry Point
# ══════════════════════════════════════════════════════════════════════════════

def main() -> int:
    _normalize_path_env()
    app = ServerLauncherApp()
    app.run()
    return 0


if __name__ == "__main__":
    sys.exit(main())
