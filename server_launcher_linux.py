#!/usr/bin/env python3
"""Server Launcher (Linux & Cross-Platform).

A Tkinter control panel for dev services: each server can be switched on
and off on its own, with live logs and status lights.

Optimized for Linux (systemd / POSIX process groups, ss/lsof network inspection,
desktop terminal emulator launching, and native font/scrolling support) while
maintaining full compatibility.

Standard library only - no pip installs needed.
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
import webbrowser
from pathlib import Path

import tkinter as tk
from tkinter import ttk, messagebox, filedialog

IS_WINDOWS = sys.platform.startswith("win")
CREATE_NO_WINDOW = 0x08000000 if IS_WINDOWS else 0
CREATE_NEW_CONSOLE = 0x00000010 if IS_WINDOWS else 0


# --------------------------------------------------------------------------
# Environment & Subprocess Helpers
# --------------------------------------------------------------------------

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
    cur_path = os.environ.get("PATH", "").split(os.pathsep)
    for p in additions:
        p_str = str(p)
        if p.is_dir() and p_str not in cur_path:
            cur_path.insert(0, p_str)
    os.environ["PATH"] = os.pathsep.join(cur_path)


def _proc_kwargs(hidden: bool = True, session: bool = False) -> dict:
    """Return platform-appropriate keyword args for subprocess.Popen / run."""
    kw = {}
    if IS_WINDOWS:
        if hidden:
            kw["creationflags"] = CREATE_NO_WINDOW
    else:
        if session:
            kw["start_new_session"] = True
    return kw


# --------------------------------------------------------------------------
# Fonts & Look-and-feel Helpers
# --------------------------------------------------------------------------

class Fonts:
    UI = "Segoe UI"
    UI_BOLD = "Segoe UI Semibold"
    MONO = "Consolas"

    @classmethod
    def init(cls, root: tk.Tk) -> None:
        if IS_WINDOWS:
            cls.UI = "Segoe UI"
            cls.UI_BOLD = "Segoe UI Semibold"
            cls.MONO = "Consolas"
            return
        try:
            import tkinter.font as tkfont
            families = set(tkfont.families(root))
        except Exception:
            families = set()

        for cand in ("Cantarell", "Liberation Sans", "Noto Sans", "DejaVu Sans", "Ubuntu"):
            if cand in families:
                cls.UI = cand
                cls.UI_BOLD = cand
                break
        else:
            cls.UI = "TkDefaultFont"
            cls.UI_BOLD = "TkDefaultFont"

        for cand in ("Liberation Mono", "DejaVu Sans Mono", "Noto Sans Mono", "monospace"):
            if cand in families:
                cls.MONO = cand
                break
        else:
            cls.MONO = "TkFixedFont"


def font_ui(size: int = 10, bold: bool = False) -> tuple:
    if IS_WINDOWS:
        return (Fonts.UI_BOLD if bold else Fonts.UI, size)
    return (Fonts.UI, size, "bold" if bold else "normal")


def font_mono(size: int = 9, bold: bool = False) -> tuple:
    return (Fonts.MONO, size, "bold" if bold else "normal")


def _bind_canvas_scroll(canvas: tk.Canvas) -> None:
    """Enable smooth mousewheel scrolling across Windows, Linux, and macOS."""
    def _on_wheel(event):
        if event.num == 4:
            canvas.yview_scroll(-1, "units")
        elif event.num == 5:
            canvas.yview_scroll(1, "units")
        elif getattr(event, "delta", 0):
            canvas.yview_scroll(int(-event.delta / 120), "units")

    def _enter(event):
        for seq in ("<MouseWheel>", "<Button-4>", "<Button-5>"):
            canvas.bind_all(seq, _on_wheel)

    def _leave(event):
        for seq in ("<MouseWheel>", "<Button-4>", "<Button-5>"):
            canvas.unbind_all(seq)

    canvas.bind("<Enter>", _enter)
    canvas.bind("<Leave>", _leave)


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
    """Return a venv python binary under cwd, or '' if none found."""
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
    """Split a command string into args, respecting double-quoted tokens."""
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
    """Format an argument list into a command line string."""
    if IS_WINDOWS:
        return subprocess.list2cmdline(args)
    return shlex.join(str(a) for a in args)


def resolve_command(args: list, cwd=None) -> list:
    """Return args with the executable resolved to a full path."""
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
    if not resolved and not IS_WINDOWS and base == "python":
        resolved = shutil.which("python3")
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

EH = "#252830"       # entry bg
EH_HOVER = "#2f3340" # button hover


# --------------------------------------------------------------------------
# Network Helpers
# --------------------------------------------------------------------------

def port_open(port: int, host: str = "127.0.0.1", timeout: float = 0.35) -> bool:
    """Check if TCP port is accepting connections on IPv4 or IPv6 localhost."""
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            sock.settimeout(timeout)
            if sock.connect_ex((host, port)) == 0:
                return True
    except OSError:
        pass
    if host in ("127.0.0.1", "localhost"):
        try:
            with socket.socket(socket.AF_INET6, socket.SOCK_STREAM) as sock:
                sock.settimeout(timeout)
                if sock.connect_ex(("::1", port)) == 0:
                    return True
        except OSError:
            pass
    return False


def lan_ip() -> str:
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            sock.connect(("8.8.8.8", 80))
            return sock.getsockname()[0]
    except OSError:
        return "127.0.0.1"


# --------------------------------------------------------------------------
# Process Termination (Cross-Platform)
# --------------------------------------------------------------------------

def _get_child_pids_linux(pid: int) -> list[int]:
    """Recursively collect child PIDs via /proc filesystem."""
    children = []
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


HZ = 100
try:
    HZ = os.sysconf("SC_CLK_TCK")
except (AttributeError, ValueError, OSError):
    pass


def _proc_ticks(pid: int) -> int:
    """Return utime+stime jiffies for one process."""
    try:
        stat = Path(f"/proc/{pid}/stat").read_text(encoding="ascii", errors="replace")
        idx = stat.rfind(")")
        if idx == -1:
            return 0
        fields = stat[idx + 2:].split()
        return int(fields[11]) + int(fields[12])
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
        sum(_proc_rss_kb(p) for p in all_pids),
    )


def _fmt_uptime(secs: int) -> str:
    if secs < 60:
        return f"{secs}s"
    if secs < 3600:
        return f"{secs // 60}m {secs % 60}s"
    h = secs // 3600
    m = (secs % 3600) // 60
    return f"{h}h {m}m"


def kill_process_tree(pid: int) -> None:
    """Terminate a process and all its spawned child processes."""
    if pid <= 0:
        return

    if IS_WINDOWS:
        try:
            subprocess.run(
                ["taskkill", "/PID", str(pid), "/T", "/F"],
                capture_output=True,
                creationflags=CREATE_NO_WINDOW,
                timeout=10,
            )
        except (OSError, subprocess.SubprocessError):
            pass
        return

    # POSIX / Linux
    pgid = None
    try:
        cand_pgid = os.getpgid(pid)
        if cand_pgid == pid:
            pgid = cand_pgid
    except OSError:
        pass

    children = _get_child_pids_linux(pid)

    # 1. Graceful SIGTERM
    if pgid is not None:
        try:
            os.killpg(pgid, signal.SIGTERM)
        except OSError:
            pass
    for cpid in children:
        try:
            os.kill(cpid, signal.SIGTERM)
        except OSError:
            pass
    try:
        os.kill(pid, signal.SIGTERM)
    except OSError:
        pass

    if shutil.which("pkill"):
        try:
            subprocess.run(["pkill", "-TERM", "-P", str(pid)],
                           capture_output=True, timeout=2)
        except Exception:
            pass

    # Brief delay for cleanup
    time.sleep(0.15)

    # 2. Force SIGKILL if any processes are still running
    if pgid is not None:
        try:
            os.killpg(pgid, signal.SIGKILL)
        except OSError:
            pass
    for cpid in children:
        try:
            os.kill(cpid, signal.SIGKILL)
        except OSError:
            pass
    try:
        os.kill(pid, signal.SIGKILL)
    except OSError:
        pass

    if shutil.which("pkill"):
        try:
            subprocess.run(["pkill", "-KILL", "-P", str(pid)],
                           capture_output=True, timeout=2)
        except Exception:
            pass


def taskkill(pid: int) -> None:
    kill_process_tree(pid)


# --------------------------------------------------------------------------
# Local server scanning (Linux & Windows)
# --------------------------------------------------------------------------

NETSTAT_RE = re.compile(
    r"\s*(?P<proto>TCP|UDP)\s+"
    r"(?P<local>[0-9.:\[\]]+)\s+"
    r"(?P<remote>[0-9.:\[\]]+)\s+"
    r"(?P<state>[A-Z_]+)?\s*"
    r"(?P<pid>\d+)"
)

_pid_name_cache = {}


def _linux_pid_name(pid: int) -> str:
    """Read human-readable process name from /proc on Linux."""
    if pid <= 0:
        return "system"
    try:
        comm_file = Path(f"/proc/{pid}/comm")
        if comm_file.is_file():
            name = comm_file.read_text(encoding="utf-8", errors="replace").strip()
            if name:
                return name
    except OSError:
        pass
    try:
        cmdline_file = Path(f"/proc/{pid}/cmdline")
        if cmdline_file.is_file():
            raw = cmdline_file.read_bytes().split(b"\0")
            if raw and raw[0]:
                exe_name = Path(os.fsdecode(raw[0])).name
                if exe_name:
                    return exe_name
    except OSError:
        pass
    return f"pid {pid}"


def _scan_listening_servers_linux() -> list[dict]:
    """Scan listening TCP ports using Linux ss, lsof, or netstat."""
    found = {}

    # 1. ss -tlpn (fast, default on all modern Linux distributions)
    if shutil.which("ss"):
        try:
            done = subprocess.run(["ss", "-tlpn"], capture_output=True,
                                  text=True, timeout=5)
            for line in (done.stdout or "").splitlines()[1:]:
                parts = line.strip().split()
                if len(parts) >= 4 and parts[0] == "LISTEN":
                    local = parts[3]
                    port_str = local.rsplit(":", 1)[-1]
                    if port_str.isdigit():
                        port = int(port_str)
                        m = re.search(r'users:\(\("([^"]+)",pid=(\d+)', line)
                        pid = int(m.group(2)) if m else 0
                        name = _linux_pid_name(pid) if pid else "system/other"
                        found[(pid, port)] = {
                            "proto": "TCP",
                            "port": port,
                            "pid": pid,
                            "name": name,
                        }
        except Exception:
            pass

    # 2. lsof -iTCP -sTCP:LISTEN -n -P (supplements missing PIDs for user-owned listeners)
    if shutil.which("lsof"):
        try:
            done = subprocess.run(
                ["lsof", "-iTCP", "-sTCP:LISTEN", "-n", "-P"],
                capture_output=True,
                text=True,
                timeout=5,
            )
            for line in (done.stdout or "").splitlines()[1:]:
                parts = line.strip().split()
                if len(parts) >= 9:
                    try:
                        pid = int(parts[1])
                    except ValueError:
                        continue
                    name_col = parts[8]
                    port_str = name_col.rsplit(":", 1)[-1]
                    if port_str.isdigit():
                        port = int(port_str)
                        name = _linux_pid_name(pid)
                        found[(pid, port)] = {
                            "proto": "TCP",
                            "port": port,
                            "pid": pid,
                            "name": name,
                        }
                        if (0, port) in found:
                            del found[(0, port)]
        except Exception:
            pass

    # 3. netstat -tlpn fallback
    if not found and shutil.which("netstat"):
        try:
            done = subprocess.run(
                ["netstat", "-tlpn"],
                capture_output=True,
                text=True,
                timeout=5,
            )
            for line in (done.stdout or "").splitlines():
                parts = line.strip().split()
                if len(parts) >= 6 and parts[0] in ("tcp", "tcp6") and parts[5] == "LISTEN":
                    local = parts[3]
                    port_str = local.rsplit(":", 1)[-1]
                    if port_str.isdigit():
                        port = int(port_str)
                        pid = 0
                        name = "system/other"
                        if len(parts) >= 7 and "/" in parts[6]:
                            p_part = parts[6].split("/", 1)[0]
                            if p_part.isdigit():
                                pid = int(p_part)
                                name = _linux_pid_name(pid)
                        found[(pid, port)] = {
                            "proto": "TCP",
                            "port": port,
                            "pid": pid,
                            "name": name,
                        }
                        if pid and (0, port) in found:
                            del found[(0, port)]
        except Exception:
            pass

    return sorted(found.values(), key=lambda d: d["port"])


def _run_netstat_windows() -> str:
    try:
        done = subprocess.run(
            ["netstat", "-ano"],
            capture_output=True,
            creationflags=CREATE_NO_WINDOW,
            timeout=20,
            text=True,
        )
        return done.stdout or ""
    except (OSError, subprocess.SubprocessError):
        return ""


def _tasklist_names_windows() -> dict:
    if _pid_name_cache:
        return _pid_name_cache
    names = {}
    try:
        done = subprocess.run(
            ["tasklist", "/FO", "CSV", "/NH"],
            capture_output=True,
            creationflags=CREATE_NO_WINDOW,
            timeout=30,
            text=True,
        )
        for line in (done.stdout or "").splitlines():
            try:
                parts = next(iter(__import__("csv").reader([line])))
                if len(parts) >= 2 and parts[1].strip().isdigit():
                    names[int(parts[1].strip())] = parts[0].strip('"')
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
    if not IS_WINDOWS:
        return _scan_listening_servers_linux()

    out = _run_netstat_windows()
    names = _tasklist_names_windows()
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
            found[key] = {
                "proto": proto,
                "port": port,
                "pid": pid,
                "name": names.get(pid, "pid %d" % pid),
            }
    return sorted(found.values(), key=lambda d: d["port"])


# --------------------------------------------------------------------------
# External Console Helpers
# --------------------------------------------------------------------------

def write_batch(key: str, title: str, cwd: Path, env_extra: dict,
                args: list) -> str:
    lines = ["@echo off", "title " + title, 'cd /d "%s"' % cwd]
    for name, value in env_extra.items():
        lines.append('set "%s=%s"' % (name, value))
    lines.append(subprocess.list2cmdline(args))
    path = Path(tempfile.gettempdir()) / ("launcher_%s.cmd" % key)
    path.write_text("\r\n".join(lines) + "\r\n", encoding="utf-8")
    return str(path)


def write_shell_script(key: str, title: str, cwd: Path, env_extra: dict,
                       args: list) -> str:
    """Create a temporary bash script to execute the server in an external terminal."""
    lines = ["#!/usr/bin/env bash"]
    for name, value in env_extra.items():
        lines.append('export %s="%s"' % (name, str(value).replace('"', '\\"')))
    lines.append('cd "%s"' % cwd)
    cmd_str = " ".join(shlex.quote(str(a)) for a in args)
    lines.append('echo -ne "\\033]0;%s\\007"' % title)
    lines.append(cmd_str)
    lines.append("EXIT_CODE=$?")
    lines.append('echo ""')
    lines.append('echo "[launcher] Process exited with code $EXIT_CODE."')
    lines.append('read -n 1 -s -r -p "Press any key to close this terminal..."')
    path = Path(tempfile.gettempdir()) / ("launcher_%s.sh" % key)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    try:
        path.chmod(0o755)
    except OSError:
        pass
    return str(path)


def _get_external_terminal_command(script_path: str, title: str, cwd: Path) -> list[str] | None:
    """Find and format command for an available terminal emulator on Linux."""
    candidates = [
        ("xdg-terminal-exec", [script_path]),
        ("ptyxis", ["--working-directory", str(cwd), "--", script_path]),
        ("gnome-terminal", ["--", script_path]),
        ("konsole", ["-e", script_path]),
        ("xfce4-terminal", ["-e", script_path]),
        ("x-terminal-emulator", ["-e", script_path]),
        ("xterm", ["-T", title, "-e", script_path]),
        ("alacritty", ["-e", script_path]),
        ("kitty", [script_path]),
        ("foot", [script_path]),
        ("mate-terminal", ["-e", script_path]),
        ("lxterminal", ["-e", script_path]),
        ("terminator", ["-x", script_path]),
    ]
    for term, term_args in candidates:
        if shutil.which(term):
            return [term] + term_args
    return None


def stream(proc, log) -> int:
    for raw in iter(proc.stdout.readline, b""):
        text = raw.decode("utf-8", "replace").replace("\r", "")
        for line in text.splitlines():
            log(ANSI.sub("", line))
    return proc.wait()


# --------------------------------------------------------------------------
# Service
# --------------------------------------------------------------------------
# Service Environment & Execution
# --------------------------------------------------------------------------

def _get_project_env(cwd: Path, env_extra: dict) -> dict:
    """Return an environment populated with project virtualenv and node_modules."""
    env = os.environ.copy()
    env.update(env_extra)
    env.setdefault("PYTHONUNBUFFERED", "1")

    cwd_path = Path(cwd)
    # Virtualenv binary discovery (.venv, venv, etc.)
    for folder in (".venv", "venv", "env", ".virtualenv"):
        sub = "Scripts" if IS_WINDOWS else "bin"
        vdir = cwd_path / folder
        vbin = vdir / sub
        if vbin.is_dir():
            cur_path = env.get("PATH", "")
            env["PATH"] = str(vbin) + os.pathsep + cur_path
            env["VIRTUAL_ENV"] = str(vdir)
            break

    # node_modules/.bin binary discovery (for npm/npx/node tools)
    node_bin = cwd_path / "node_modules" / ".bin"
    if node_bin.is_dir():
        cur_path = env.get("PATH", "")
        env["PATH"] = str(node_bin) + os.pathsep + cur_path

    return env


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
        self.sub_proc = None                        # currently running one-off command
        self.state = "stopped"
        self.stopping = False
        self.external_var = None
        self._started_at = None

        # Resource monitoring stats
        self._prev_ticks = 0
        self._prev_tick_time = 0.0
        self.cpu_pct = 0.0
        self.ram_mb = 0.0
        self.uptime_secs = 0

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

    def send_input(self, text: str) -> bool:
        """Send input to running server or execute command in working directory if stopped."""
        text_strip = text.strip()
        if not text_strip:
            if self.alive and self.proc and self.proc.stdin and not self.proc.stdin.closed:
                try:
                    self.proc.stdin.write(b"\n")
                    self.proc.stdin.flush()
                    return True
                except OSError:
                    return False
            return False

        # 1. Shell escape (!cmd or /cmd) to explicitly run a sub-command
        if text_strip.startswith(("!", "/")):
            cmd = text_strip[1:].strip()
            if cmd:
                self.run_sub_command("command", cmd)
                return True

        # 2. If main server is running, pipe to its stdin
        if self.alive and self.proc and self.proc.stdin and not self.proc.stdin.closed:
            try:
                raw = text if text.endswith("\n") else text + "\n"
                self.proc.stdin.write(raw.encode("utf-8"))
                self.proc.stdin.flush()
                self.log("> " + text_strip)
                return True
            except (BrokenPipeError, OSError) as exc:
                self.log("[launcher] failed to send stdin: " + str(exc))
                return False

        # 3. If a sub-command is running (e.g. npm install prompt), pipe to sub-command stdin
        if self.sub_proc and self.sub_proc.poll() is None:
            if self.sub_proc.stdin and not self.sub_proc.stdin.closed:
                try:
                    raw = text if text.endswith("\n") else text + "\n"
                    self.sub_proc.stdin.write(raw.encode("utf-8"))
                    self.sub_proc.stdin.flush()
                    self.log("> " + text_strip)
                    return True
                except (BrokenPipeError, OSError) as exc:
                    self.log("[launcher] failed to send stdin to command: " + str(exc))
                    return False

        # 4. If nothing is running, execute command in working directory (e.g. npm install, pip install)
        self.run_sub_command("command", text_strip)
        return True

    def send_interrupt(self) -> None:
        """Send Ctrl+C (SIGINT) to the running sub-command or main server."""
        target_proc = None
        if self.sub_proc and self.sub_proc.poll() is None:
            target_proc = self.sub_proc
        elif self.alive and self.proc:
            target_proc = self.proc

        if target_proc is None:
            self.log("[launcher] nothing running to interrupt")
            return

        self.log("[launcher] sending Ctrl+C (SIGINT)...")
        if IS_WINDOWS:
            try:
                target_proc.send_signal(signal.CTRL_C_EVENT)
            except Exception:
                pass
        else:
            try:
                pgid = os.getpgid(target_proc.pid)
                os.killpg(pgid, signal.SIGINT)
            except OSError:
                try:
                    target_proc.send_signal(signal.SIGINT)
                except OSError:
                    pass

    def send_eof(self) -> None:
        """Send EOF by closing stdin."""
        target = None
        if self.sub_proc and self.sub_proc.poll() is None and self.sub_proc.stdin:
            target = self.sub_proc
        elif self.alive and self.proc and self.proc.stdin:
            target = self.proc

        if target and target.stdin and not target.stdin.closed:
            try:
                target.stdin.close()
                self.log("[launcher] stdin closed (EOF)")
            except OSError:
                pass

    def run_sub_command(self, label: str, command: str) -> None:
        """Run a command in this service's working directory."""
        if self.sub_proc and self.sub_proc.poll() is None:
            self.log("[launcher] a command is already running (use Ctrl+C to stop it)")
            return

        if not self.cwd.exists():
            self.log("[launcher] folder not found: " + str(self.cwd))
            return

        self.log("[launcher] " + str(self.cwd))
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
                run_args,
                cwd=str(self.cwd),
                env=env,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                stdin=subprocess.PIPE,
                **_proc_kwargs(hidden=True, session=True),
            )
            threading.Thread(target=self._pump_foreign, args=(self.sub_proc,),
                             daemon=True).start()
        except OSError as exc:
            self.sub_proc = None
            self.log("[launcher] could not run: " + str(exc))

    def _pump_foreign(self, proc) -> None:
        try:
            code = stream(proc, self.log)
            self.log("[launcher] command exited with code %d" % code)
        finally:
            if self.sub_proc is proc:
                self.sub_proc = None

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
        env = _get_project_env(self.cwd, self.env_extra)

        self.stopping = False
        self._started_at = None
        self.set_state("starting")
        self.log("[launcher] " + str(self.cwd))
        self.log("[launcher] > " + format_command(self.args))

        run_args = resolve_command(self.args, self.cwd)

        try:
            if external:
                if IS_WINDOWS:
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
                    script = write_shell_script(self.key, "Server - " + self.name,
                                                self.cwd, self.env_extra, run_args)
                    term_cmd = _get_external_terminal_command(
                        script, "Server - " + self.name, self.cwd)
                    if term_cmd:
                        self.proc = subprocess.Popen(
                            term_cmd,
                            cwd=str(self.cwd),
                            env=env,
                            start_new_session=True,
                        )
                        self.log("[launcher] running in terminal: %s" % term_cmd[0])
                        threading.Thread(target=self._wait_only, daemon=True).start()
                    else:
                        self.log("[launcher] no terminal emulator found; running embedded")
                        self.proc = subprocess.Popen(
                            run_args,
                            cwd=str(self.cwd),
                            env=env,
                            stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT,
                            stdin=subprocess.PIPE,
                            start_new_session=True,
                        )
                        threading.Thread(target=self._pump, daemon=True).start()
            else:
                self.proc = subprocess.Popen(
                    run_args,
                    cwd=str(self.cwd),
                    env=env,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.STDOUT,
                    stdin=subprocess.PIPE,
                    **_proc_kwargs(hidden=True, session=True),
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
                args,
                cwd=str(self.cwd),
                env=env,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                stdin=subprocess.DEVNULL,
                **_proc_kwargs(hidden=True, session=True),
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
        if self.proc and self.proc.stdin:
            try:
                self.proc.stdin.close()
            except OSError:
                pass
        self.log("[launcher] stopping pid %d and its child processes..." % pid)
        threading.Thread(target=self._kill_tree, args=(pid,), daemon=True).start()

    def _kill_tree(self, pid: int) -> None:
        kill_process_tree(pid)

    def _pump(self) -> None:
        self._finish(stream(self.proc, self.log))

    def _wait_only(self) -> None:
        self._finish(self.proc.wait())

    def _finish(self, code: int) -> None:
        self.log("[launcher] process exited with code %d" % code)
        self.proc = None
        self.cpu_pct = 0.0
        self.ram_mb = 0.0
        self._prev_ticks = 0
        self._prev_tick_time = 0.0
        self.uptime_secs = 0
        self.set_state("stopped" if self.stopping or code == 0 else "error")
        self.stopping = False

    def sample_stats(self) -> None:
        """Sample CPU % and RAM MB from /proc for this server process tree."""
        if not self.alive or self.proc is None:
            self.cpu_pct = 0.0
            self.ram_mb = 0.0
            return
        try:
            pid = self.proc.pid
            now = time.monotonic()
            ticks, rss_kb = _tree_stats(pid)
            if self._prev_tick_time > 0:
                elapsed = now - self._prev_tick_time
                if elapsed > 0:
                    self.cpu_pct = max(0.0, ((ticks - self._prev_ticks) / (elapsed * HZ)) * 100.0)
            self._prev_ticks = ticks
            self._prev_tick_time = now
            self.ram_mb = rss_kb / 1024.0
            if self._started_at is not None:
                self.uptime_secs = int(time.time() - self._started_at)
        except Exception:
            pass


# --------------------------------------------------------------------------
# Server Templates
# --------------------------------------------------------------------------

SERVER_TEMPLATES = [
    {
        "name": "React (Vite)",
        "command": "npm run dev",
        "port": 5173,
        "actions": [{"label": "Build", "command": "npm run build"}, {"label": "Install", "command": "npm install"}],
        "links": [{"label": "Localhost", "url": "http://localhost:5173"}],
    },
    {
        "name": "Next.js",
        "command": "npm run dev",
        "port": 3000,
        "actions": [{"label": "Build", "command": "npm run build"}, {"label": "Install", "command": "npm install"}],
        "links": [{"label": "Localhost", "url": "http://localhost:3000"}],
    },
    {
        "name": "FastAPI (Uvicorn)",
        "command": "uvicorn main:app --reload --port 8000",
        "port": 8000,
        "actions": [{"label": "Install reqs", "command": "pip install -r requirements.txt"}],
        "links": [{"label": "Swagger Docs", "url": "http://localhost:8000/docs"}],
    },
    {
        "name": "Flask",
        "command": "flask run --port 5000 --debug",
        "port": 5000,
        "env": {"FLASK_ENV": "development", "FLASK_DEBUG": "1"},
        "actions": [{"label": "Install reqs", "command": "pip install -r requirements.txt"}],
        "links": [{"label": "Localhost", "url": "http://localhost:5000"}],
    },
    {
        "name": "Django",
        "command": "python manage.py runserver 0.0.0.0:8000",
        "port": 8000,
        "actions": [{"label": "Migrate", "command": "python manage.py migrate"}],
        "links": [{"label": "Admin Panel", "url": "http://localhost:8000/admin"}],
    },
    {
        "name": "Node / Express",
        "command": "node server.js",
        "port": 3000,
        "actions": [{"label": "Install", "command": "npm install"}],
        "links": [{"label": "Localhost", "url": "http://localhost:3000"}],
    },
    {
        "name": "PostgreSQL",
        "command": "postgres -D ./data",
        "stop_command": "pg_ctl stop -D ./data",
        "port": 5432,
    },
    {
        "name": "Redis",
        "command": "redis-server",
        "stop_command": "redis-cli shutdown",
        "port": 6379,
    },
    {
        "name": "MongoDB",
        "command": "mongod --dbpath ./data/db",
        "port": 27017,
    },
]


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
                 font=font_ui(size)).pack(anchor="w", pady=(pad, 2))

    def _hint(self, parent, text):
        tk.Label(parent, text=text, bg=CARD_BG, fg=MUTED,
                 font=font_ui(8)).pack(anchor="w")

    def _entry(self, parent, var, browse=None):
        row = tk.Frame(parent, bg=CARD_BG)
        row.pack(fill="x", pady=(0, 4))
        ent = tk.Entry(row, textvariable=var, bg=EH, fg=TEXT,
                       insertbackground=TEXT, font=font_mono(10),
                       relief="flat", highlightthickness=1,
                       highlightbackground=CARD_BORDER, highlightcolor=ACCENT)
        ent.pack(side="left", fill="x", expand=True, ipady=4)
        if browse:
            tk.Button(row, text="Browse", bg=EH, fg=TEXT,
                       activebackground=EH_HOVER, activeforeground=TEXT,
                       font=font_ui(9), relief="flat",
                       command=browse).pack(side="right", padx=(6, 0))
        return ent

    # -- ui ---------------------------------------------------------------

    def _build_ui(self):
        outer = tk.Frame(self, bg=BG)
        outer.pack(fill="both", expand=True, padx=12, pady=12)

        card = tk.Frame(outer, bg=CARD_BG)
        card.pack(fill="both", expand=True)

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
            if event.num == 4:
                canvas.yview_scroll(-1, "units")
            elif event.num == 5:
                canvas.yview_scroll(1, "units")
            elif getattr(event, "delta", 0):
                canvas.yview_scroll(int(-event.delta / 120), "units")

        for seq in ("<MouseWheel>", "<Button-4>", "<Button-5>"):
            canvas.bind_all(seq, _on_mousewheel)
        self._dialog_canvas = canvas

        title = "Edit server" if self.edit else "Add a new server"
        tk.Label(inner, text=title, bg=CARD_BG, fg=TEXT,
                 font=font_ui(15, bold=True)).pack(anchor="w")
        tk.Label(inner, text="Saved to servers.json - no code changes needed.",
                 bg=CARD_BG, fg=MUTED, font=font_ui(9)).pack(anchor="w",
                                                             pady=(2, 8))

        f = inner

        # Template Picker
        if not self.edit:
            tpl_box = tk.Frame(f, bg="#202433", padx=10, pady=8, highlightthickness=1,
                               highlightbackground=CARD_BORDER)
            tpl_box.pack(fill="x", pady=(0, 10))
            tk.Label(tpl_box, text="⚡ Pick a Template (Fast Setup)", bg="#202433", fg=ACCENT,
                     font=font_ui(10, bold=True)).pack(anchor="w")
            tk.Label(tpl_box, text="Auto-populates command, port, actions, and links for popular stacks.",
                     bg="#202433", fg=MUTED, font=font_ui(8)).pack(anchor="w", pady=(1, 6))

            tpl_row = tk.Frame(tpl_box, bg="#202433")
            tpl_row.pack(fill="x")
            tpl_names = ["-- Select a template --"] + [t["name"] for t in SERVER_TEMPLATES]
            self.selected_tpl_var = tk.StringVar(value=tpl_names[0])
            tpl_dropdown = ttk.Combobox(tpl_row, textvariable=self.selected_tpl_var,
                                        values=tpl_names, state="readonly", width=28)
            tpl_dropdown.pack(side="left", padx=(0, 8))

            def _on_tpl_selected(event=None):
                val = self.selected_tpl_var.get()
                for t in SERVER_TEMPLATES:
                    if t["name"] == val:
                        self._apply_template(t)
                        break

            tpl_dropdown.bind("<<ComboboxSelected>>", _on_tpl_selected)
            tk.Button(tpl_row, text="Apply Template", bg=ACCENT, fg="#ffffff",
                      activebackground=ACCENT_HOVER, activeforeground="#ffffff",
                      font=font_ui(9, bold=True), relief="flat", padx=10, pady=2,
                      command=_on_tpl_selected).pack(side="left")

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
                                insertbackground=TEXT, font=font_mono(9),
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
                  font=font_ui(9), relief="flat",
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
                  font=font_ui(9), relief="flat",
                  command=self._add_link_row).pack(anchor="w", pady=(2, 6))

        # Buttons
        btn_row = tk.Frame(card, bg=CARD_BG)
        btn_row.pack(fill="x", padx=20, pady=14)
        tk.Button(btn_row, text="Cancel", bg=CARD_BG, fg=MUTED,
                  activebackground="#2a2e3a", activeforeground=TEXT,
                  font=font_ui(10), relief="flat", width=10,
                  command=self._cancel).pack(side="right")
        tk.Button(btn_row, text="Save", bg=ACCENT, fg="#ffffff",
                  activebackground=ACCENT_HOVER, activeforeground="#ffffff",
                  font=font_ui(10, bold=True), relief="flat", width=10,
                  command=self._save).pack(side="right", padx=(0, 8))

        self.focus_force()
        self.bind("<Escape>", lambda e: self._cancel())

    def _unbind_dialog_scroll(self):
        if hasattr(self, "_dialog_canvas"):
            for seq in ("<MouseWheel>", "<Button-4>", "<Button-5>"):
                self._dialog_canvas.unbind_all(seq)

    def _field_row(self, parent, label_var, cmd_var):
        row = tk.Frame(parent, bg=CARD_BG)
        row.pack(fill="x", pady=2)
        tk.Label(row, text="label:", bg=CARD_BG, fg=MUTED,
                 font=font_ui(8)).pack(side="left")
        lbl_ent = tk.Entry(row, textvariable=label_var, bg=EH, fg=TEXT,
                           insertbackground=TEXT, font=font_mono(9),
                           relief="flat", highlightthickness=1,
                           highlightbackground=CARD_BORDER, highlightcolor=ACCENT)
        lbl_ent.pack(side="left", fill="x", expand=False, ipady=3, padx=(2, 8))
        lbl_ent.configure(width=12)
        cmd_ent = tk.Entry(row, textvariable=cmd_var, bg=EH, fg=TEXT,
                           insertbackground=TEXT, font=font_mono(9),
                           relief="flat", highlightthickness=1,
                           highlightbackground=CARD_BORDER, highlightcolor=ACCENT)
        cmd_ent.pack(side="left", fill="x", expand=True, ipady=3)
        tk.Button(row, text="x", bg="#24171c", fg=DANGER,
                  activebackground="#331d23", activeforeground=DANGER,
                  font=font_ui(9), relief="flat", width=2,
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
                 font=font_ui(8)).pack(side="left")
        lbl_ent = tk.Entry(row, textvariable=lv, bg=EH, fg=TEXT,
                           insertbackground=TEXT, font=font_mono(9),
                           relief="flat", highlightthickness=1,
                           highlightbackground=CARD_BORDER, highlightcolor=ACCENT,
                           width=12)
        lbl_ent.pack(side="left", fill="x", expand=False, ipady=3, padx=(2, 8))
        url_ent = tk.Entry(row, textvariable=uv, bg=EH, fg=TEXT,
                           insertbackground=TEXT, font=font_mono(9),
                           relief="flat", highlightthickness=1,
                           highlightbackground=CARD_BORDER, highlightcolor=ACCENT)
        url_ent.pack(side="left", fill="x", expand=True, ipady=3)
        tk.Button(row, text="x", bg="#24171c", fg=DANGER,
                  activebackground="#331d23", activeforeground=DANGER,
                  font=font_ui(9), relief="flat", width=2,
                  command=row.destroy).pack(side="left", padx=(6, 0))
        self.link_rows.append((lv, uv))

    def _apply_template(self, tpl: dict):
        """Populate dialog fields from a selected template."""
        if not self.name_var.get().strip():
            self.name_var.set(tpl.get("name", ""))
        self.cmd_var.set(tpl.get("command", ""))
        self.stopcmd_var.set(tpl.get("stop_command", ""))
        self.port_var.set(str(tpl.get("port", 0)))

        # Populate env text
        if "env" in tpl:
            self.env_text.delete("1.0", "end")
            env_lines = "\n".join("%s=%s" % (k, v) for k, v in tpl["env"].items())
            self.env_text.insert("1.0", env_lines)

        # Clear existing actions and add template actions
        for w in self.actions_box.winfo_children():
            w.destroy()
        self.action_rows.clear()
        for act in tpl.get("actions", []):
            self._add_action_row(act.get("label", ""), act.get("command", ""))

        # Clear existing links and add template links
        for w in self.links_box.winfo_children():
            w.destroy()
        self.link_rows.clear()
        for lnk in tpl.get("links", []):
            self._add_link_row(lnk.get("label", ""), lnk.get("url", ""))

    def _browse_dir(self):
        d = filedialog.askdirectory(parent=self, title="Select working directory")
        if d:
            self.cwd_var.set(d)

    def _cancel(self):
        self._unbind_dialog_scroll()
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
        self._unbind_dialog_scroll()
        self.grab_release()
        self.destroy()


# --------------------------------------------------------------------------
# Quick Launch / Command Palette Dialog (Ctrl+P)
# --------------------------------------------------------------------------

class CommandPaletteDialog(tk.Toplevel):
    """Fuzzy/substring search popup for starting, stopping, and running actions."""

    def __init__(self, parent, services: dict):
        super().__init__(parent)
        self.parent = parent
        self.services = services
        self.result = None
        self._items = []

        self.title("Quick Launch (Ctrl+P)")
        self.geometry("540x420")
        self.minsize(440, 320)
        self.configure(bg=CARD_BG)
        self.transient(parent)
        self.grab_set()

        # Center on parent
        self.update_idletasks()
        px = parent.winfo_x() + (parent.winfo_width() - 540) // 2
        py = parent.winfo_y() + 80
        self.geometry("+%d+%d" % (max(20, px), max(30, py)))

        self._build_ui()
        self._filter("")

        self.protocol("WM_DELETE_WINDOW", self._cancel)
        self.bind("<Escape>", lambda e: self._cancel())

    def _build_ui(self):
        container = tk.Frame(self, bg=CARD_BG, padx=16, pady=14)
        container.pack(fill="both", expand=True)

        hdr = tk.Frame(container, bg=CARD_BG)
        hdr.pack(fill="x", pady=(0, 8))
        tk.Label(hdr, text="⚡ Quick Launch", bg=CARD_BG, fg=TEXT,
                 font=font_ui(12, bold=True)).pack(side="left")
        tk.Label(hdr, text="Esc to close", bg=CARD_BG, fg=MUTED,
                 font=font_ui(8)).pack(side="right")

        self.search_var = tk.StringVar()
        self.search_var.trace_add("write", lambda *args: self._on_search_change())

        input_frame = tk.Frame(container, bg=CARD_BORDER, padx=1, pady=1)
        input_frame.pack(fill="x", pady=(0, 10))

        self.entry = tk.Entry(
            input_frame, textvariable=self.search_var, bg=EH, fg=TEXT,
            insertbackground=TEXT, font=font_mono(10), relief="flat",
            highlightthickness=0
        )
        self.entry.pack(fill="x", ipady=6, padx=8)
        self.entry.focus_set()

        list_frame = tk.Frame(container, bg=CARD_BG)
        list_frame.pack(fill="both", expand=True)

        self.listbox = tk.Listbox(
            list_frame, bg=LOG_BG, fg=TEXT, selectbackground=ACCENT,
            selectforeground="#ffffff", font=font_mono(9),
            activestyle="none", relief="flat", highlightthickness=1,
            highlightbackground=CARD_BORDER, highlightcolor=ACCENT
        )
        vbar = ttk.Scrollbar(list_frame, orient="vertical", command=self.listbox.yview)
        self.listbox.configure(yscrollcommand=vbar.set)

        self.listbox.pack(side="left", fill="both", expand=True)
        vbar.pack(side="right", fill="y")

        self.entry.bind("<Down>", self._on_down)
        self.entry.bind("<Up>", self._on_up)
        self.entry.bind("<Return>", self._on_enter)
        self.listbox.bind("<Return>", self._on_enter)
        self.listbox.bind("<Double-Button-1>", self._on_enter)

        footer = tk.Frame(container, bg=CARD_BG)
        footer.pack(fill="x", pady=(8, 0))
        tk.Label(footer, text="Type to filter • ↑/↓ navigate • Enter select",
                 bg=CARD_BG, fg=MUTED, font=font_ui(8)).pack(side="left")

    def _on_search_change(self):
        self._filter(self.search_var.get().strip().lower())

    def _filter(self, q: str):
        self.listbox.delete(0, tk.END)
        self._items = []

        tokens = q.split() if q else []

        for svc in self.services.values():
            s_name = svc.name.lower()
            s_cmd = " ".join(svc.args).lower()

            # Start/stop entry
            match = True
            for t in tokens:
                if t not in s_name and t not in s_cmd and t not in ("start", "stop"):
                    match = False
                    break

            if match:
                if svc.alive:
                    label = "⏹  Stop %s" % svc.name
                    item = (label, "stop", svc.key, None, None)
                else:
                    label = "▶  Start %s" % svc.name
                    item = (label, "start", svc.key, None, None)
                self._items.append(item)
                self.listbox.insert(tk.END, "  " + label)

            # Sub-commands
            for act_label, act_cmd in svc.custom_actions:
                act_match = True
                for t in tokens:
                    if t not in act_label.lower() and t not in act_cmd.lower() and t not in s_name:
                        act_match = False
                        break
                if act_match:
                    label = "⚡  %s [%s]" % (act_label, svc.name)
                    item = (label, "action", svc.key, act_cmd, act_label)
                    self._items.append(item)
                    self.listbox.insert(tk.END, "  " + label)

        if self._items:
            self.listbox.selection_set(0)
            self.listbox.activate(0)

    def _on_down(self, event):
        size = self.listbox.size()
        if not size:
            return "break"
        cur = self.listbox.curselection()
        idx = (cur[0] + 1) % size if cur else 0
        self.listbox.selection_clear(0, tk.END)
        self.listbox.selection_set(idx)
        self.listbox.see(idx)
        return "break"

    def _on_up(self, event):
        size = self.listbox.size()
        if not size:
            return "break"
        cur = self.listbox.curselection()
        idx = (cur[0] - 1) % size if cur else 0
        self.listbox.selection_clear(0, tk.END)
        self.listbox.selection_set(idx)
        self.listbox.see(idx)
        return "break"

    def _on_enter(self, event=None):
        cur = self.listbox.curselection()
        if not cur or cur[0] >= len(self._items):
            return "break"
        self.result = self._items[cur[0]]
        self.grab_release()
        self.destroy()
        return "break"

    def _cancel(self):
        self.result = None
        self.grab_release()
        self.destroy()


# --------------------------------------------------------------------------
# Application
# --------------------------------------------------------------------------

class LauncherApp:
    def __init__(self, root: tk.Tk):
        self.root = root
        Fonts.init(root)

        self.events = queue.Queue()
        self.services: dict[str, Service] = {}
        self.cards: dict[str, dict] = {}
        self.logs: dict[str, tk.Text] = {}

        self._build_custom_services()
        self._build_ui()

        self.root.bind_all("<Control-p>", lambda e: self.open_command_palette())
        self.root.bind_all("<Control-P>", lambda e: self.open_command_palette())

        self.root.after(120, self._drain)
        self.root.after(1500, self._poll_ports)
        self.root.after(1000, self._poll_stats)
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

        default_shell = [os.environ.get("SHELL", "bash")] if not IS_WINDOWS else ["cmd", "/k"]
        args = split_command(command) if command else default_shell

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
                "command": format_command(svc.args),
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
                        font=font_ui(9))
        style.configure("TNotebook", background=BG, borderwidth=0)
        style.configure("TNotebook.Tab", background=BG, foreground=MUTED,
                        font=font_ui(9), padding=(14, 6))
        style.map("TNotebook.Tab",
                  background=[("selected", CARD_BG)],
                  foreground=[("selected", TEXT)])

        header = tk.Frame(root, bg=BG)
        header.pack(fill="x", padx=20, pady=(16, 6))

        left_hdr = tk.Frame(header, bg=BG)
        left_hdr.pack(side="left", fill="x", expand=True)
        tk.Label(left_hdr, text="Server Launcher", bg=BG, fg=TEXT,
                 font=font_ui(18, bold=True)).pack(anchor="w")
        tk.Label(left_hdr,
                 text="this pc: %s    servers: %d"
                      % (lan_ip(), len(self.services)),
                 bg=BG, fg=MUTED, font=font_ui(9)).pack(anchor="w",
                                                        pady=(2, 0))

        right_hdr = tk.Frame(header, bg=BG)
        right_hdr.pack(side="right")
        tk.Button(right_hdr, text="+ Add Server", bg=ACCENT, fg="#ffffff",
                  activebackground=ACCENT_HOVER, activeforeground="#ffffff",
                  font=font_ui(10, bold=True), relief="flat", padx=14,
                  pady=4, command=self._add_server).pack(side="right", padx=(0, 8))
        tk.Button(right_hdr, text="⚡ Quick Launch (Ctrl+P)", bg=EH, fg=ACCENT,
                  activebackground=EH_HOVER, activeforeground=ACCENT,
                  font=font_ui(10), relief="flat", padx=12, pady=4,
                  command=self.open_command_palette).pack(side="right", padx=(0, 6))
        tk.Button(right_hdr, text="Stop all", bg=EH, fg=TEXT,
                  activebackground=EH_HOVER, activeforeground=TEXT,
                  font=font_ui(10), relief="flat", padx=12, pady=4,
                  command=self.stop_all).pack(side="right", padx=(0, 6))
        tk.Button(right_hdr, text="Start all", bg=SUCCESS, fg="#ffffff",
                  activebackground="#00d2a0", activeforeground="#ffffff",
                  font=font_ui(10, bold=True), relief="flat", padx=12, pady=4,
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
        _bind_canvas_scroll(self.card_canvas)

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
                  font=font_ui(9), relief="flat", padx=9, pady=2,
                  command=command).pack(side="right", padx=(6, 0))

    # -- overview / all running servers on this pc ------------------------

    def _build_overview_tab(self) -> None:
        frame = tk.Frame(self.notebook, bg=BG)
        self.notebook.add(frame, text="  Server Overview  ")

        top = tk.Frame(frame, bg=BG)
        top.pack(fill="x", padx=16, pady=(14, 8))
        tk.Label(top, text="Servers running on this PC",
                 bg=BG, fg=TEXT, font=font_ui(15, bold=True)).pack(side="left")
        self.overview_time_lbl = tk.Label(top, text="", bg=BG, fg=MUTED,
                                          font=font_ui(9))
        self.overview_time_lbl.pack(side="left", padx=(14, 0))
        tk.Button(top, text="Refresh", bg=EH, fg=ACCENT,
                  activebackground=EH_HOVER, activeforeground=ACCENT,
                  font=font_ui(10, bold=True), relief="flat", padx=14, pady=4,
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
        _bind_canvas_scroll(self.ov_canvas)

        # managed servers section
        tk.Label(self._ov_inner, text="Launcher-managed servers", bg=BG, fg=TEXT,
                 font=font_ui(11, bold=True)).pack(anchor="w", padx=2,
                                                   pady=(0, 4))
        self._managed_box = tk.Frame(self._ov_inner, bg=BG)
        self._managed_box.pack(fill="x")

        # external servers section
        tk.Label(self._ov_inner, text="Other processes listening on this PC",
                 bg=BG, fg=TEXT, font=font_ui(11, bold=True)).pack(
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
                 font=font_ui(11, bold=True)).pack(anchor="w")
        tk.Label(text_col, text=detail, bg=CARD_BG, fg=MUTED, anchor="w",
                 font=font_ui(9)).pack(anchor="w")

        if svc is not None and svc.alive and svc.proc:
            stats_str = "pid %d  CPU %4.1f%%  RAM %4.0f MB" % (
                svc.proc.pid, svc.cpu_pct, svc.ram_mb
            )
            tk.Label(inner, text=stats_str, bg=CARD_BG,
                     fg=MUTED, font=font_mono(8)).pack(side="left", padx=(12, 0))
        elif pid and port:
            tk.Label(inner, text="pid %d  :%s" % (pid, port), bg=CARD_BG,
                     fg=MUTED, font=font_mono(9)).pack(side="left",
                                                       padx=(12, 0))
        elif port:
            tk.Label(inner, text=":%s" % port, bg=CARD_BG,
                     fg=MUTED, font=font_mono(9)).pack(side="left",
                                                       padx=(12, 0))

        state_lbl = tk.Label(inner, text=state_text, bg=CARD_BG, fg=TEXT,
                             font=font_ui(9), width=10, anchor="e")
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
                          font=font_ui(9, bold=True), relief="flat", padx=10,
                          pady=3, command=do_stop).pack(side="right")
            else:
                tk.Button(btn_frame, text="Start", bg=SUCCESS, fg="#ffffff",
                          activebackground="#00d2a0", activeforeground="#ffffff",
                          font=font_ui(9, bold=True), relief="flat", padx=10,
                          pady=3, command=do_start).pack(side="right")
        elif pid:
            def do_kill(p=pid):
                self._kill_external(p)
            tk.Button(btn_frame, text="Stop", bg=DANGER, fg="#ffffff",
                      activebackground="#d04030", activeforeground="#ffffff",
                      font=font_ui(9, bold=True), relief="flat", padx=10,
                      pady=3, command=do_kill).pack(side="right")

        return row

    def _kill_external(self, pid: int) -> None:
        if not pid:
            return
        if not messagebox.askyesno(
                "Stop process", "Kill process %d?  It will be terminated." % pid):
            return
        taskkill(pid)
        self._refresh_overview(force=True)

    def _external_sig(self, listeners: list, managed_pids: set) -> tuple:
        sig = []
        for entry in listeners:
            if entry["pid"] and entry["pid"] in managed_pids:
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

        # external scan runs in background
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
            if pid and pid in managed_pids:
                continue
            count += 1
            proto = entry["proto"]
            port = entry["port"]
            name = entry["name"] or ("pid %d" % pid if pid else "system")
            detail = "%s :%s  (%s)" % (name, port, proto)
            self._overview_row(self._external_box, name, detail,
                               "running", pid=pid, port=port)
        if count == 0:
            tk.Label(self._external_box,
                     text="No unmanaged processes found listening on a port.",
                     bg=BG, fg=MUTED, font=font_ui(9)).pack(anchor="w",
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
                 font=font_ui(11, bold=True),
                 anchor="w").pack(side="left")
        if svc.port:
            port_badge = tk.Label(
                name_holder, text=" :%s " % svc.port,
                bg="#252838", fg=ACCENT,
                font=font_mono(8, bold=True),
                relief="flat", padx=4, pady=1
            )
            port_badge.pack(side="left", padx=(6, 0))

        state_lbl = tk.Label(name_row, text=LABEL["stopped"], bg=CARD_BG,
                             fg=MUTED, font=font_ui(9), anchor="e")
        state_lbl.grid(row=0, column=1, sticky="e")

        tk.Label(card_inner, text=svc.subtitle, bg=CARD_BG, fg=MUTED,
                 font=font_ui(9), wraplength=340, justify="left", anchor="w",
                 ).grid(row=1, column=0, columnspan=2, sticky="w",
                        pady=(4, 2))

        stats_lbl = tk.Label(card_inner, text="", bg=CARD_BG, fg="#a0abbd",
                             font=font_mono(8), anchor="w")
        stats_lbl.grid(row=2, column=0, columnspan=2, sticky="w",
                       pady=(0, 6))

        controls = tk.Frame(card_inner, bg=CARD_BG)
        controls.grid(row=3, column=0, columnspan=2, sticky="ew")

        toggle = tk.Button(controls, text="Start", bg=SUCCESS, fg="#ffffff",
                           activebackground="#00d2a0", activeforeground="#ffffff",
                           font=font_ui(10, bold=True), relief="flat",
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

        row = 4

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
                                   font=font_ui(9), relief="flat", padx=9,
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
                               font=font_ui(9), relief="flat", padx=9,
                               pady=3, command=cb)
            button.pack(side="left", padx=(0, 6))
            action_buttons.append(button)

        if svc.links:
            links = tk.Frame(card_inner, bg=CARD_BG)
            links.grid(row=row, column=0, columnspan=2, sticky="w", pady=(8, 0))
            row += 1
            for text, url in svc.links:
                b = tk.Label(links, text=text, bg=CARD_BG, fg=ACCENT,
                             font=(Fonts.UI, 9, "underline"), cursor="hand2")
                b.pack(side="left", padx=(0, 12))
                b.bind("<Button-1>", lambda e, u=url: webbrowser.open(u))

        if svc.missing:
            tk.Label(card_inner, text=svc.missing, bg=CARD_BG, fg=DANGER,
                     font=font_ui(9), wraplength=340, justify="left",
                     anchor="w").grid(row=row, column=0, columnspan=2,
                                      sticky="w", pady=(6, 0))
            toggle.configure(state="disabled")
            for button in action_buttons:
                button.configure(state="disabled")

        self.cards[svc.key] = {
            "dot": dot, "oval": oval, "state": state_lbl,
            "stats": stats_lbl,
            "toggle": toggle, "actions": action_buttons, "outer": outer,
        }

    def _build_log_tab(self, svc: Service) -> None:
        frame = tk.Frame(self.notebook, bg=BG)
        self.notebook.add(frame, text="  %s  " % svc.name)
        frame.rowconfigure(0, weight=1)
        frame.columnconfigure(0, weight=1)

        text = tk.Text(frame, bg=LOG_BG, fg=LOG_FG, insertbackground=LOG_FG,
                       font=font_mono(9), wrap="none", relief="flat",
                       padx=10, pady=8, state="disabled",
                       highlightthickness=0, bd=0)
        text.grid(row=0, column=0, sticky="nsew")
        yscroll = ttk.Scrollbar(frame, orient="vertical", command=text.yview)
        yscroll.grid(row=0, column=1, sticky="ns")
        xscroll = ttk.Scrollbar(frame, orient="horizontal", command=text.xview)
        xscroll.grid(row=1, column=0, sticky="ew")
        text.configure(yscrollcommand=yscroll.set, xscrollcommand=xscroll.set)
        text.tag_configure("launcher", foreground="#7c8aff")
        text.tag_configure("stdin", foreground="#00d2a0")

        # Interactive command / input row (makes the terminal writable)
        input_bar = tk.Frame(frame, bg=CARD_BG, pady=4, padx=8)
        input_bar.grid(row=2, column=0, columnspan=2, sticky="ew", pady=(4, 0))

        tk.Label(input_bar, text="❯", bg=CARD_BG, fg=ACCENT,
                 font=font_mono(10, bold=True)).pack(side="left", padx=(2, 6))

        input_var = tk.StringVar()
        input_ent = tk.Entry(input_bar, textvariable=input_var, bg=EH, fg=TEXT,
                             insertbackground=TEXT, font=font_mono(9),
                             relief="flat", highlightthickness=1,
                             highlightbackground=CARD_BORDER, highlightcolor=ACCENT)
        placeholder = "Run command in directory (e.g. npm i, pip install) or send input..."

        def _on_focus_in(event):
            if input_var.get() == placeholder:
                input_var.set("")
                input_ent.configure(fg=TEXT)

        def _on_focus_out(event):
            if not input_var.get().strip():
                input_var.set(placeholder)
                input_ent.configure(fg=MUTED)

        input_var.set(placeholder)
        input_ent.configure(fg=MUTED)
        input_ent.bind("<FocusIn>", _on_focus_in)
        input_ent.bind("<FocusOut>", _on_focus_out)
        input_ent.pack(side="left", fill="x", expand=True, ipady=3)

        history: list[str] = []
        hist_idx = [0]

        def _do_send(event=None):
            cmd = input_var.get().strip()
            if cmd == placeholder:
                cmd = ""
            if cmd:
                svc.send_input(cmd)
                if not history or history[-1] != cmd:
                    history.append(cmd)
                hist_idx[0] = len(history)
                input_var.set("")
            return "break"

        def _hist_up(event):
            if history and hist_idx[0] > 0:
                hist_idx[0] -= 1
                input_var.set(history[hist_idx[0]])
                input_ent.configure(fg=TEXT)
                input_ent.icursor("end")
            return "break"

        def _hist_down(event):
            if history and hist_idx[0] < len(history) - 1:
                hist_idx[0] += 1
                input_var.set(history[hist_idx[0]])
                input_ent.configure(fg=TEXT)
                input_ent.icursor("end")
            elif hist_idx[0] >= len(history) - 1:
                hist_idx[0] = len(history)
                input_var.set("")
            return "break"

        input_ent.bind("<Return>", _do_send)
        input_ent.bind("<KP_Enter>", _do_send)
        input_ent.bind("<Up>", _hist_up)
        input_ent.bind("<Down>", _hist_down)

        # Typing while focused on the log output automatically routes to input_ent
        def _on_text_key(event):
            # Allow Ctrl shortcuts (copy, etc.)
            if event.state & 4:
                return None
            if event.char and event.char.isprintable():
                if input_var.get() == placeholder:
                    input_var.set("")
                    input_ent.configure(fg=TEXT)
                input_ent.focus_set()
                input_ent.insert("end", event.char)
                return "break"
            elif event.keysym in ("Return", "KP_Enter"):
                input_ent.focus_set()
                return "break"
            return None

        text.bind("<Key>", _on_text_key)

        tk.Button(input_bar, text="Send", bg=ACCENT, fg="#ffffff",
                  activebackground=ACCENT_HOVER, activeforeground="#ffffff",
                  font=font_ui(9, bold=True), relief="flat", padx=10,
                  pady=2, command=_do_send).pack(side="left", padx=(6, 4))

        tk.Button(input_bar, text="Ctrl+C", bg=EH, fg=WARNING,
                  activebackground=EH_HOVER, activeforeground=WARNING,
                  font=font_ui(9), relief="flat", padx=8, pady=2,
                  command=svc.send_interrupt).pack(side="left", padx=(0, 4))

        tk.Button(input_bar, text="Clear", bg=EH, fg=MUTED,
                  activebackground=EH_HOVER, activeforeground=TEXT,
                  font=font_ui(9), relief="flat", padx=8, pady=2,
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
        tag = ""
        if line.startswith("[launcher]"):
            tag = "launcher"
        elif line.startswith("> "):
            tag = "stdin"
        widget.insert("end", line + "\n", tag)
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
        if "stats" in card:
            if svc.alive:
                uptime = _fmt_uptime(svc.uptime_secs) if svc.uptime_secs else ""
                card["stats"].configure(
                    text="CPU %4.1f%%   RAM %4.0f MB%s" % (
                        svc.cpu_pct, svc.ram_mb, ("   up " + uptime) if uptime else ""
                    )
                )
            else:
                card["stats"].configure(text="")
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
            # grace period fallback
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

    def _poll_stats(self) -> None:
        def _worker():
            for svc in list(self.services.values()):
                svc.sample_stats()
            try:
                self.root.after(0, self._apply_stats)
            except Exception:
                pass
        threading.Thread(target=_worker, daemon=True).start()
        self.root.after(1000, self._poll_stats)

    def _apply_stats(self) -> None:
        for key, svc in self.services.items():
            card = self.cards.get(key)
            if not card or "stats" not in card:
                continue
            if svc.alive:
                uptime = _fmt_uptime(svc.uptime_secs) if svc.uptime_secs else "0s"
                card["stats"].configure(
                    text="CPU %4.1f%%   RAM %4.0f MB   Running for %s" % (
                        svc.cpu_pct, svc.ram_mb, uptime
                    )
                )
            else:
                card["stats"].configure(text="")
        if hasattr(self, "_managed_box"):
            self._rebuild_managed_overview()

    def select_server_tab(self, key: str) -> None:
        """Switch the notebook tab to the specified server by key."""
        svc = self.services.get(key)
        if not svc:
            return
        for i, tab_id in enumerate(self.notebook.tabs()):
            if self.notebook.tab(tab_id, "text").strip() == svc.name:
                self.notebook.select(i)
                break

    def open_command_palette(self) -> None:
        """Open the Ctrl+P quick launch dialog."""
        dlg = CommandPaletteDialog(self.root, self.services)
        self.root.wait_window(dlg)

        if not dlg.result:
            return

        label, action, key, cmd, act_label = dlg.result
        svc = self.services.get(key)
        if not svc:
            return

        if action == "start":
            svc.start()
            self.select_server_tab(key)
        elif action == "stop":
            svc.stop()
        elif action == "action" and cmd:
            svc.run_sub_command(act_label, cmd)
            self.select_server_tab(key)

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
            existing.discard(svc.key)

        entry = {
            "key": svc.key,
            "name": svc.name,
            "cwd": str(svc.cwd),
            "command": format_command(svc.args),
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
                                   'Delete "%s"?  This cannot be undone.'
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

    def _update_header_count(self) -> None:
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
                                'Click "+ Add Server" to create one.')
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
    _normalize_path_env()
    root = tk.Tk()
    LauncherApp(root)
    root.mainloop()
    return 0


if __name__ == "__main__":
    sys.exit(main())
