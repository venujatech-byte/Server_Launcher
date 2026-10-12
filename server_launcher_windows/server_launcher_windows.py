#!/usr/bin/env python3
"""Server Launcher (Windows Optimized).

A high-performance Windows desktop control panel for developer services,
native Windows Services management, hardware sensors & telemetry, and an
interactive PowerShell terminal with UAC Administrator elevation.

Key Windows-Optimized Capabilities:
  * Windows Administrator (UAC): Live privilege detection with one-click
    "Relaunch as Administrator" elevation, plus elevated script execution.
  * Windows Services Manager: Real-time service status, start, stop, pause,
    restart, startup type configuration (Auto/Manual/Disabled), details viewer,
    and service registration via sc.exe.
  * Hardware Sensors & Resource Telemetry: Real-time CPU clocks and loads,
    physical RAM and pagefile commits, storage drive free space and live speed,
    GPU adapters, network link bandwidth, battery health, and top apps with
    permanent column widths and deferred collection.
  * Windows PowerShell Terminal & Commands: Interactive PowerShell console,
    diagnostic commands catalog (SFC, DISM, DNS flush, netstat listeners),
    and custom script manager with elevated execution.
  * Dev Server Launcher: Process group management, live ANSI log streaming,
    port collision detection, and instant kill on Windows ports.

Standard library only — zero pip dependencies required.
"""

from __future__ import annotations

import ctypes
import glob
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
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, Tuple

import tkinter as tk
from tkinter import filedialog, messagebox, ttk

# --------------------------------------------------------------------------
# Platform Detection & Constants
# --------------------------------------------------------------------------

IS_WINDOWS = sys.platform.startswith("win")
CREATE_NO_WINDOW = 0x08000000 if IS_WINDOWS else 0
CREATE_NEW_CONSOLE = 0x00000010 if IS_WINDOWS else 0

SCRIPT_DIR = Path(__file__).resolve().parent
CONFIG_PATH = SCRIPT_DIR / "servers.json"
PRESETS_PATH = SCRIPT_DIR / "powershell_presets.json"


def is_admin() -> bool:
    """Return True if running with Windows Administrator privileges."""
    if IS_WINDOWS:
        try:
            return bool(ctypes.windll.shell32.IsUserAnAdmin())
        except Exception:
            return False
    else:
        return os.geteuid() == 0 if hasattr(os, "geteuid") else False


def relaunch_as_admin() -> None:
    """Relaunch the current script with elevated Administrator privileges."""
    if IS_WINDOWS:
        try:
            params = " ".join([f'"{arg}"' for arg in sys.argv])
            ctypes.windll.shell32.ShellExecuteW(
                None, "runas", sys.executable, params, str(SCRIPT_DIR), 1
            )
            sys.exit(0)
        except Exception as e:
            messagebox.showerror("Elevation Error", f"Failed to request Administrator elevation:\n{e}")
    else:
        try:
            cmd = ["pkexec", sys.executable] + sys.argv
            subprocess.Popen(cmd)
            sys.exit(0)
        except Exception as e:
            messagebox.showerror("Elevation Error", f"Failed to elevate with pkexec:\n{e}")


# --------------------------------------------------------------------------
# Nocturnal Command Design System (Material 3 Dark Palette)
# --------------------------------------------------------------------------

class Palette:
    """Material 3 Dark / Nocturnal Command theme color tokens."""
    BG_CANVAS = "#0a0a0f"           # Deep obsidian canvas
    BG_SURFACE = "#13131a"          # Top header and toolbars
    BG_CARD = "#1d1d27"             # Elevated container plates
    BG_CARD_HOVER = "#242433"       # Hovered cards
    BG_INPUT = "#15151e"            # Recessed text wells and entries
    BG_BUTTON = "#282836"           # Standard action button surface
    BG_BUTTON_HOVER = "#353547"     # Button hover state
    BORDER = "#2c2c3b"              # Subtle perimeter hairline
    BORDER_LIGHT = "#3d3d52"        # Active/focused border

    TEXT_PRIMARY = "#f1f0f7"        # Bright high-emphasis text
    TEXT_SECONDARY = "#c7c5d4"      # Medium emphasis text
    TEXT_MUTED = "#878599"          # Low-emphasis metadata labels

    ACCENT = "#c4c0ff"              # Electric Periwinkle / Violet (Primary)
    ACCENT_HOVER = "#dbd8ff"
    ACCENT_BG = "#2b2277"
    ACCENT_TEXT = "#0f0b38"

    CYAN = "#70d5e4"                # Telemetry Cyan (Secondary)
    CYAN_BG = "#00363e"
    CYAN_TEXT = "#a3f0fc"

    SUCCESS = "#4ade80"             # Mint runtime / Healthy (Tertiary)
    SUCCESS_BG = "#0f3820"
    SUCCESS_TEXT = "#86efac"

    WARNING = "#fcd34d"             # Alert amber / Compiling
    WARNING_BG = "#451a03"
    WARNING_TEXT = "#fef08a"

    DANGER = "#ffb4ab"              # Coral red / Error
    DANGER_BG = "#93000a"
    DANGER_TEXT = "#ffdad6"


# --------------------------------------------------------------------------
# PowerShell & Windows System Execution Helpers
# --------------------------------------------------------------------------

def run_powershell(command: str, elevated: bool = False, timeout_sec: float = 15.0) -> Tuple[int, str, str]:
    """Execute a PowerShell command with optional Administrator elevation."""
    if elevated and not is_admin() and IS_WINDOWS:
        # Prompt UAC elevation via Start-Process
        encoded_cmd = command.replace('"', '`"')
        elev_script = f'Start-Process powershell -Verb RunAs -ArgumentList "-NoProfile -Command \\"{encoded_cmd}\\""'
        cmd = ["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", elev_script]
    else:
        cmd = ["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", command]

    if not IS_WINDOWS:
        # Fallback on non-Windows environments for testing
        cmd = ["sh", "-c", command]

    try:
        proc = subprocess.run(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=timeout_sec,
            creationflags=CREATE_NO_WINDOW,
        )
        return proc.returncode, proc.stdout.strip(), proc.stderr.strip()
    except subprocess.TimeoutExpired:
        return -1, "", "Command timed out."
    except Exception as e:
        return -1, "", str(e)


def strip_ansi(text: str) -> str:
    """Remove ANSI escape sequences from console log outputs."""
    ansi_escape = re.compile(r"\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])")
    return ansi_escape.sub("", text)


def format_bytes(bytes_val: float) -> str:
    """Format bytes into human-readable B, KB, MB, GB, TB."""
    val = float(bytes_val)
    for unit in ["B", "KB", "MB", "GB", "TB"]:
        if val < 1024.0 or unit == "TB":
            return f"{val:.1f} {unit}" if unit != "B" else f"{int(val)} B"
        val /= 1024.0
    return f"{bytes_val:.0f} B"


def format_rate(rate_bps: float) -> str:
    """Format bytes per second into human-readable throughput."""
    return f"{format_bytes(rate_bps)}/s"


# --------------------------------------------------------------------------
# Data Structures
# --------------------------------------------------------------------------

@dataclass
class WindowsServiceInfo:
    name: str
    display_name: str
    status: str             # Running, Stopped, Paused, etc.
    startup_type: str       # Auto, Manual, Disabled
    pid: int = 0
    path_name: str = ""
    description: str = ""


@dataclass
class ProcessInfo:
    pid: int
    name: str
    status: str
    cpu_pct: float = 0.0
    memory_bytes: int = 0
    disk_rate_bps: float = 0.0


@dataclass
class HardwareSensorsSnapshot:
    cpu_name: str = "Unknown CPU"
    cpu_cores_physical: int = 1
    cpu_cores_logical: int = 1
    cpu_clock_mhz: int = 0
    cpu_global_usage: float = 0.0
    mem_total_bytes: int = 0
    mem_used_bytes: int = 0
    mem_free_bytes: int = 0
    mem_usage_pct: float = 0.0
    page_total_bytes: int = 0
    page_used_bytes: int = 0
    page_usage_pct: float = 0.0
    gpu_name: str = "Standard Display Adapter"
    gpu_vram_mb: int = 0
    gpu_driver: str = ""
    disks: List[Dict[str, Any]] = field(default_factory=list)
    disk_read_rate_bps: float = 0.0
    disk_write_rate_bps: float = 0.0
    network_rx_rate_bps: float = 0.0
    network_tx_rate_bps: float = 0.0
    network_adapters: List[Dict[str, Any]] = field(default_factory=list)
    battery_pct: Optional[float] = None
    ac_online: Optional[bool] = None
    top_apps: List[ProcessInfo] = field(default_factory=list)


@dataclass
class ServerAction:
    name: str
    cmd: str


@dataclass
class ServerLink:
    name: str
    url: str


@dataclass
class ServerConfig:
    name: str
    dir: str
    cmd: str
    stop_cmd: str = ""
    port: int = 0
    group: str = "General"
    env: Dict[str, str] = field(default_factory=dict)
    actions: List[ServerAction] = field(default_factory=list)
    links: List[ServerLink] = field(default_factory=list)
    autostart: bool = False
    auto_restart: bool = False
    own_console: bool = False


# --------------------------------------------------------------------------
# Windows Services Manager Backend
# --------------------------------------------------------------------------

class WindowsServiceManager:
    """Manages Windows Services via Get-CimInstance and sc.exe."""

    def __init__(self):
        self.cached_services: List[WindowsServiceInfo] = []
        self.last_fetch: float = 0.0

    def query_services(self) -> List[WindowsServiceInfo]:
        """Fetch all installed Windows services with metadata."""
        if not IS_WINDOWS:
            # Fallback mock for non-Windows developer environments
            return [
                WindowsServiceInfo("wuauserv", "Windows Update", "Running", "Auto", 1024, "C:\\Windows\\system32\\svchost.exe -k netsvcs", "Enables detection and download of Windows updates."),
                WindowsServiceInfo("Spooler", "Print Spooler", "Running", "Auto", 1540, "C:\\Windows\\System32\\spoolsv.exe", "Manages all print jobs."),
                WindowsServiceInfo("MSSQLSERVER", "SQL Server (MSSQLSERVER)", "Stopped", "Manual", 0, "C:\\Program Files\\Microsoft SQL Server\\sqlservr.exe", "Database server instance."),
                WindowsServiceInfo("Docker", "Docker Desktop Service", "Running", "Auto", 2180, "C:\\Program Files\\Docker\\Docker\\com.docker.service", "Backend engine for Docker containers."),
                WindowsServiceInfo("wsearch", "Windows Search", "Running", "Auto", 1890, "C:\\Windows\\system32\\SearchIndexer.exe", "Provides content indexing for files."),
                WindowsServiceInfo("EventLog", "Windows Event Log", "Running", "Auto", 740, "C:\\Windows\\System32\\svchost.exe -k LocalServiceNetworkRestricted", "Maintains Windows event log."),
            ]

        ps_cmd = (
            "Get-CimInstance Win32_Service | "
            "Select-Object Name, DisplayName, State, StartMode, ProcessId, PathName, Description | "
            "ConvertTo-Json -Compress"
        )
        rc, out, err = run_powershell(ps_cmd, timeout_sec=12.0)
        services = []
        if rc == 0 and out:
            try:
                data = json.loads(out)
                items = data if isinstance(data, list) else [data]
                for item in items:
                    services.append(
                        WindowsServiceInfo(
                            name=str(item.get("Name") or ""),
                            display_name=str(item.get("DisplayName") or item.get("Name") or ""),
                            status=str(item.get("State") or "Unknown"),
                            startup_type=str(item.get("StartMode") or "Unknown"),
                            pid=int(item.get("ProcessId") or 0),
                            path_name=str(item.get("PathName") or ""),
                            description=str(item.get("Description") or ""),
                        )
                    )
            except Exception as e:
                print(f"Error parsing services JSON: {e}")

        # Fallback to Get-Service if CIM query was empty
        if not services:
            rc, out, _ = run_powershell("Get-Service | Select-Object Name, DisplayName, Status, StartType | ConvertTo-Json -Compress")
            if rc == 0 and out:
                try:
                    data = json.loads(out)
                    items = data if isinstance(data, list) else [data]
                    for item in items:
                        st_map = {1: "Stopped", 4: "Running", 7: "Paused"}
                        raw_st = item.get("Status")
                        status_str = st_map.get(raw_st, str(raw_st)) if isinstance(raw_st, int) else str(raw_st or "Unknown")
                        services.append(
                            WindowsServiceInfo(
                                name=str(item.get("Name") or ""),
                                display_name=str(item.get("DisplayName") or ""),
                                status=status_str,
                                startup_type=str(item.get("StartType") or "Unknown"),
                            )
                        )
                except Exception:
                    pass

        self.cached_services = services
        self.last_fetch = time.time()
        return services

    def start_service(self, name: str) -> Tuple[bool, str]:
        cmd = f"Start-Service -Name '{name}' -ErrorAction Stop; Get-Service -Name '{name}' | Select-Object -ExpandProperty Status"
        rc, out, err = run_powershell(cmd, elevated=not is_admin())
        return rc == 0, out or err

    def stop_service(self, name: str) -> Tuple[bool, str]:
        cmd = f"Stop-Service -Name '{name}' -Force -ErrorAction Stop; Get-Service -Name '{name}' | Select-Object -ExpandProperty Status"
        rc, out, err = run_powershell(cmd, elevated=not is_admin())
        return rc == 0, out or err

    def restart_service(self, name: str) -> Tuple[bool, str]:
        cmd = f"Restart-Service -Name '{name}' -Force -ErrorAction Stop; Get-Service -Name '{name}' | Select-Object -ExpandProperty Status"
        rc, out, err = run_powershell(cmd, elevated=not is_admin())
        return rc == 0, out or err

    def set_startup_type(self, name: str, startup_type: str) -> Tuple[bool, str]:
        # Valid startup types: Automatic, Manual, Disabled
        cmd = f"Set-Service -Name '{name}' -StartupType {startup_type} -ErrorAction Stop"
        rc, out, err = run_powershell(cmd, elevated=not is_admin())
        return rc == 0, out or err

    def create_service(self, name: str, display_name: str, bin_path: str, start_type: str = "auto") -> Tuple[bool, str]:
        cmd = f'sc.exe create "{name}" binPath= "{bin_path}" start= {start_type} DisplayName= "{display_name}"'
        rc, out, err = run_powershell(cmd, elevated=not is_admin())
        return rc == 0, out or err


# --------------------------------------------------------------------------
# Hardware Sensors & Telemetry Collector Backend
# --------------------------------------------------------------------------

class WindowsSensorsCollector:
    """Collects real-time hardware telemetry on Windows via WMI & Performance Counters."""

    def __init__(self):
        self.last_collect = 0.0
        self.snapshot = HardwareSensorsSnapshot()
        self.prev_proc_cpu: Dict[int, float] = {}

    def collect(self, include_top_apps: bool = False) -> HardwareSensorsSnapshot:
        snap = HardwareSensorsSnapshot()

        if not IS_WINDOWS:
            # Cross-platform simulated snapshot
            snap.cpu_name = "Intel(R) Core(TM) i7-12700H CPU @ 2.70GHz"
            snap.cpu_cores_physical = 8
            snap.cpu_cores_logical = 16
            snap.cpu_clock_mhz = 2688
            snap.cpu_global_usage = 24.5
            snap.mem_total_bytes = 16 * 1024 * 1024 * 1024
            snap.mem_used_bytes = int(snap.mem_total_bytes * 0.48)
            snap.mem_free_bytes = snap.mem_total_bytes - snap.mem_used_bytes
            snap.mem_usage_pct = 48.0
            snap.page_total_bytes = 20 * 1024 * 1024 * 1024
            snap.page_used_bytes = int(snap.page_total_bytes * 0.35)
            snap.page_usage_pct = 35.0
            snap.gpu_name = "NVIDIA GeForce RTX 3070 Ti Laptop GPU"
            snap.gpu_vram_mb = 8192
            snap.gpu_driver = "535.129.03"
            snap.disks = [
                {"drive": "C:", "label": "Windows OS", "total_gb": 512.0, "free_gb": 284.5, "usage_pct": 44.4},
                {"drive": "D:", "label": "Data & Projects", "total_gb": 1024.0, "free_gb": 740.0, "usage_pct": 27.7},
            ]
            snap.disk_read_rate_bps = 1024 * 512
            snap.disk_write_rate_bps = 1024 * 128
            snap.network_rx_rate_bps = 1024 * 840
            snap.network_tx_rate_bps = 1024 * 230
            snap.ac_online = True
            snap.battery_pct = 95.0

            if include_top_apps:
                snap.top_apps = [
                    ProcessInfo(1184, "node.exe", "Running", 14.2, 412 * 1024 * 1024, 1024 * 40),
                    ProcessInfo(2408, "python.exe", "Running", 8.5, 230 * 1024 * 1024, 1024 * 12),
                    ProcessInfo(892, "Docker Desktop.exe", "Running", 5.1, 1024 * 1024 * 1024, 1024 * 80),
                    ProcessInfo(4120, "chrome.exe", "Running", 4.8, 650 * 1024 * 1024, 1024 * 15),
                    ProcessInfo(3390, "Code.exe", "Running", 2.2, 380 * 1024 * 1024, 1024 * 5),
                ]
            self.snapshot = snap
            return snap

        # 1. CPU & Memory
        try:
            # Memory via GlobalMemoryStatusEx (Windows Native API)
            class MEMORYSTATUSEX(ctypes.Structure):
                _fields_ = [
                    ("dwLength", ctypes.c_ulong),
                    ("dwMemoryLoad", ctypes.c_ulong),
                    ("ullTotalPhys", ctypes.c_ulonglong),
                    ("ullAvailPhys", ctypes.c_ulonglong),
                    ("ullTotalPageFile", ctypes.c_ulonglong),
                    ("ullAvailPageFile", ctypes.c_ulonglong),
                    ("ullTotalVirtual", ctypes.c_ulonglong),
                    ("ullAvailVirtual", ctypes.c_ulonglong),
                    ("sullAvailExtendedVirtual", ctypes.c_ulonglong),
                ]

            stat = MEMORYSTATUSEX()
            stat.dwLength = ctypes.sizeof(MEMORYSTATUSEX)
            if ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(stat)):
                snap.mem_total_bytes = stat.ullTotalPhys
                snap.mem_free_bytes = stat.ullAvailPhys
                snap.mem_used_bytes = snap.mem_total_bytes - snap.mem_free_bytes
                snap.mem_usage_pct = float(stat.dwMemoryLoad)

                snap.page_total_bytes = stat.ullTotalPageFile
                page_free = stat.ullAvailPageFile
                snap.page_used_bytes = max(0, snap.page_total_bytes - page_free)
                if snap.page_total_bytes > 0:
                    snap.page_usage_pct = (snap.page_used_bytes / snap.page_total_bytes) * 100.0
        except Exception:
            pass

        # 2. WMI Query for CPU, Disks, GPU, Battery
        ps_telemetry = (
            "$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1 Name, NumberOfCores, NumberOfLogicalProcessors, CurrentClockSpeed, MaxClockSpeed, LoadPercentage; "
            "$disks = Get-CimInstance Win32_LogicalDisk -Filter 'DriveType=3' | Select-Object DeviceID, VolumeName, Size, FreeSpace; "
            "$gpu = Get-CimInstance Win32_VideoController | Select-Object -First 1 Name, AdapterRAM, DriverVersion; "
            "$batt = Get-CimInstance Win32_Battery -ErrorAction SilentlyContinue | Select-Object -First 1 EstimatedChargeRemaining, BatteryStatus; "
            "@{ CPU=$cpu; Disks=$disks; GPU=$gpu; Battery=$batt } | ConvertTo-Json -Compress -Depth 2"
        )
        rc, out, _ = run_powershell(ps_telemetry, timeout_sec=4.0)
        if rc == 0 and out:
            try:
                data = json.loads(out)
                cpu_data = data.get("CPU") or {}
                snap.cpu_name = str(cpu_data.get("Name") or "Intel / AMD Processor").strip()
                snap.cpu_cores_physical = int(cpu_data.get("NumberOfCores") or 1)
                snap.cpu_cores_logical = int(cpu_data.get("NumberOfLogicalProcessors") or 1)
                snap.cpu_clock_mhz = int(cpu_data.get("CurrentClockSpeed") or cpu_data.get("MaxClockSpeed") or 0)
                snap.cpu_global_usage = float(cpu_data.get("LoadPercentage") or 0.0)

                # Disks
                disk_items = data.get("Disks") or []
                if isinstance(disk_items, dict):
                    disk_items = [disk_items]
                for d in disk_items:
                    size_b = float(d.get("Size") or 0)
                    free_b = float(d.get("FreeSpace") or 0)
                    used_b = max(0.0, size_b - free_b)
                    pct = (used_b / size_b * 100.0) if size_b > 0 else 0.0
                    snap.disks.append({
                        "drive": str(d.get("DeviceID") or "C:"),
                        "label": str(d.get("VolumeName") or "Local Disk"),
                        "total_gb": size_b / (1024**3),
                        "free_gb": free_b / (1024**3),
                        "usage_pct": pct,
                    })

                # GPU
                gpu_data = data.get("GPU") or {}
                snap.gpu_name = str(gpu_data.get("Name") or "Standard VGA Adapter").strip()
                vram_raw = gpu_data.get("AdapterRAM")
                snap.gpu_vram_mb = int(vram_raw // (1024 * 1024)) if isinstance(vram_raw, (int, float)) and vram_raw > 0 else 0
                snap.gpu_driver = str(gpu_data.get("DriverVersion") or "")

                # Battery
                batt_data = data.get("Battery")
                if batt_data:
                    snap.battery_pct = float(batt_data.get("EstimatedChargeRemaining") or 100.0)
                    # BatteryStatus 2 = Connected to AC
                    snap.ac_online = (batt_data.get("BatteryStatus") == 2)
            except Exception as e:
                print(f"Error parsing sensors data: {e}")

        # 3. Top Processes (only collected when include_top_apps is True)
        if include_top_apps:
            ps_procs = (
                "Get-Process | "
                "Sort-Object CPU -Descending | "
                "Select-Object -First 40 Id, ProcessName, Responding, CPU, WorkingSet64 | "
                "ConvertTo-Json -Compress"
            )
            rc, out, _ = run_powershell(ps_procs, timeout_sec=4.0)
            if rc == 0 and out:
                try:
                    data = json.loads(out)
                    items = data if isinstance(data, list) else [data]
                    for item in items:
                        snap.top_apps.append(
                            ProcessInfo(
                                pid=int(item.get("Id") or 0),
                                name=str(item.get("ProcessName") or "process"),
                                status="Running" if item.get("Responding", True) else "Not Responding",
                                cpu_pct=float(item.get("CPU") or 0.0),
                                memory_bytes=int(item.get("WorkingSet64") or 0),
                            )
                        )
                except Exception:
                    pass

        self.snapshot = snap
        self.last_collect = time.time()
        return snap


# --------------------------------------------------------------------------
# PowerShell Interactive Console Session
# --------------------------------------------------------------------------

class PowerShellSession:
    """Manages bidirectional interaction with an active PowerShell process."""

    def __init__(self, on_output: Callable[[str, str], None]):
        self.on_output = on_output
        self.proc: Optional[subprocess.Popen] = None
        self.queue: queue.Queue = queue.Queue()
        self.running = False
        self.lock = threading.Lock()

    def start(self, elevated: bool = False):
        if self.proc and self.proc.poll() is None:
            return

        cmd = ["powershell.exe", "-NoLogo", "-NoExit", "-ExecutionPolicy", "Bypass"]
        if not IS_WINDOWS:
            cmd = ["bash", "--noprofile"]

        try:
            self.proc = subprocess.Popen(
                cmd,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                bufsize=1,
                creationflags=CREATE_NO_WINDOW if IS_WINDOWS else 0,
            )
            self.running = True

            threading.Thread(target=self._reader, args=(self.proc.stdout, "stdout"), daemon=True).start()
            threading.Thread(target=self._reader, args=(self.proc.stderr, "stderr"), daemon=True).start()
        except Exception as e:
            self.on_output(f"Failed to spawn PowerShell session: {e}\n", "stderr")

    def _reader(self, pipe, stream_type: str):
        try:
            for line in iter(pipe.readline, ""):
                if line:
                    self.on_output(line, stream_type)
        except Exception:
            pass
        finally:
            pipe.close()

    def send_input(self, text: str):
        if not self.proc or self.proc.poll() is not None:
            self.start()

        if self.proc and self.proc.stdin:
            try:
                clean = text.rstrip("\r\n") + "\n"
                self.proc.stdin.write(clean)
                self.proc.stdin.flush()
            except Exception as e:
                self.on_output(f"Error sending input: {e}\n", "stderr")

    def terminate(self):
        self.running = False
        if self.proc:
            try:
                self.proc.terminate()
            except Exception:
                pass


# --------------------------------------------------------------------------
# Local Dev Server Process Runner
# --------------------------------------------------------------------------

class ManagedServer:
    """Manages local dev process execution, log streaming, and port status."""

    def __init__(self, config: ServerConfig, on_log: Callable[[str, str], None], on_status_change: Callable[[], None]):
        self.config = config
        self.on_log = on_log
        self.on_status_change = on_status_change
        self.proc: Optional[subprocess.Popen] = None
        self.status = "Stopped"  # Stopped, Starting, Running, Error
        self.pid: Optional[int] = None
        self.logs: List[str] = []
        self.lock = threading.Lock()

    def start(self):
        with self.lock:
            if self.status in ["Running", "Starting"]:
                return

            self.status = "Starting"
            self.on_status_change()

            threading.Thread(target=self._run_process, daemon=True).start()

    def _run_process(self):
        log_line = f"[{time.strftime('%H:%M:%S')}] >>> Starting {self.config.name} (Port: {self.config.port})...\n"
        self._append_log(log_line)

        env = os.environ.copy()
        env.update(self.config.env)

        working_dir = self.config.dir if self.config.dir and os.path.isdir(self.config.dir) else SCRIPT_DIR

        # Prepare shell command
        if IS_WINDOWS:
            cmd = ["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", self.config.cmd]
        else:
            cmd = ["sh", "-c", self.config.cmd]

        try:
            self.proc = subprocess.Popen(
                cmd,
                cwd=str(working_dir),
                env=env,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                bufsize=1,
                creationflags=CREATE_NO_WINDOW if not self.config.own_console else CREATE_NEW_CONSOLE,
            )
            self.pid = self.proc.pid
            self.status = "Running"
            self.on_status_change()

            for line in iter(self.proc.stdout.readline, ""):
                if line:
                    self._append_log(line)

            self.proc.wait()
            rc = self.proc.returncode
            self._append_log(f"[{time.strftime('%H:%M:%S')}] <<< Process exited with code {rc}\n")

            with self.lock:
                self.status = "Stopped" if rc == 0 else "Error"
                self.pid = None
            self.on_status_change()

            if self.config.auto_restart and rc != 0:
                self._append_log(f"[{time.strftime('%H:%M:%S')}] ⟳ Auto-restart scheduled in 3 seconds...\n")
                time.sleep(3.0)
                self.start()

        except Exception as e:
            self._append_log(f"[{time.strftime('%H:%M:%S')}] ✕ Execution failed: {e}\n")
            with self.lock:
                self.status = "Error"
                self.pid = None
            self.on_status_change()

    def stop(self):
        with self.lock:
            if not self.proc or self.status == "Stopped":
                return

            self._append_log(f"[{time.strftime('%H:%M:%S')}] Stopping {self.config.name} (PID: {self.pid})...\n")

            # Optional stop command
            if self.config.stop_cmd:
                try:
                    subprocess.run(self.config.stop_cmd, shell=True, timeout=5, creationflags=CREATE_NO_WINDOW)
                except Exception:
                    pass

            if self.pid and IS_WINDOWS:
                # Force kill process tree on Windows
                subprocess.run(f"taskkill /F /T /PID {self.pid}", shell=True, creationflags=CREATE_NO_WINDOW)
            else:
                try:
                    self.proc.terminate()
                except Exception:
                    pass

            self.status = "Stopped"
            self.pid = None
        self.on_status_change()

    def restart(self):
        self.stop()
        time.sleep(0.5)
        self.start()

    def send_input(self, text: str):
        with self.lock:
            if self.proc and self.proc.stdin and self.status == "Running":
                try:
                    self.proc.stdin.write(text + "\n")
                    self.proc.stdin.flush()
                    self._append_log(f"> {text}\n")
                except Exception as e:
                    self._append_log(f"✕ Send failed: {e}\n")

    def send_interrupt(self):
        with self.lock:
            if self.proc and self.status == "Running":
                try:
                    if IS_WINDOWS and self.pid:
                        subprocess.run(f"powershell -Command \"[System.Diagnostics.Process]::GetProcessById({self.pid}).Kill()\"", shell=True, creationflags=CREATE_NO_WINDOW)
                    else:
                        import signal
                        self.proc.send_signal(signal.SIGINT)
                    self._append_log("^C\n")
                except Exception as e:
                    self._append_log(f"✕ Interrupt failed: {e}\n")

    def clear_logs(self):
        with self.lock:
            self.logs.clear()

    def _append_log(self, text: str):
        clean = strip_ansi(text)
        self.logs.append(clean)
        if len(self.logs) > 5000:
            self.logs = self.logs[-3000:]
        self.on_log(self.config.name, clean)


# --------------------------------------------------------------------------
# Main Application GUI (Tkinter + Nocturnal Command Theme)
# --------------------------------------------------------------------------

class ServerLauncherWindowsApp(tk.Tk):
    """Modern Nocturnal Command desktop control panel optimized for Windows."""

    def __init__(self):
        super().__init__()
        self.title("Server Launcher — Windows Developer Cockpit")
        self.geometry("1240x820")
        self.minsize(1000, 680)
        self.configure(bg=Palette.BG_CANVAS)

        # State models
        self.service_mgr = WindowsServiceManager()
        self.sensors_collector = WindowsSensorsCollector()
        self.servers: List[ManagedServer] = []
        self.active_server_name: Optional[str] = None
        self.active_tab_name = "servers"
        self.sensors_subtab = "system"  # 'system' or 'top_apps'
        self.top_apps_visited = False
        self.top_apps_sort_col = "cpu"
        self.top_apps_sort_desc = True
        self.last_top_apps: List[ProcessInfo] = []
        self.top_apps_search_var = tk.StringVar(value="")
        self.search_query = tk.StringVar(value="")
        self.services_filter_status = tk.StringVar(value="All")
        self.services_filter_startup = tk.StringVar(value="All")
        self.log_queue: queue.Queue = queue.Queue()

        # Load configs
        self._load_servers_config()
        self._load_powershell_presets()

        # Setup styling
        self._init_styles()

        # Build UI layout
        self._build_top_header()
        self._build_main_views()
        self._build_bottom_status_bar()

        # Initialize PowerShell REPL
        self.ps_session = PowerShellSession(self._on_ps_output)
        self.ps_session.start()

        # Start background timer polls
        self._start_background_pollers()

    def _init_styles(self):
        style = ttk.Style(self)
        style.theme_use("clam")

        # Configure generic notebook and tabs
        style.configure("TNotebook", background=Palette.BG_CANVAS, borderwidth=0)
        style.configure(
            "TNotebook.Tab",
            background=Palette.BG_CARD,
            foreground=Palette.TEXT_SECONDARY,
            padding=[16, 8],
            font=("Segoe UI", 10, "bold"),
            borderwidth=0,
        )
        style.map(
            "TNotebook.Tab",
            background=[("selected", Palette.ACCENT), ("active", Palette.BG_CARD_HOVER)],
            foreground=[("selected", Palette.ACCENT_TEXT), ("active", Palette.TEXT_PRIMARY)],
        )

        # Treeview styling (Nocturnal obsidian list)
        style.configure(
            "Treeview",
            background=Palette.BG_CARD,
            foreground=Palette.TEXT_PRIMARY,
            fieldbackground=Palette.BG_CARD,
            font=("Segoe UI", 9),
            rowheight=28,
            borderwidth=0,
        )
        style.configure(
            "Treeview.Heading",
            background=Palette.BG_SURFACE,
            foreground=Palette.TEXT_SECONDARY,
            font=("Segoe UI", 9, "bold"),
            padding=[8, 6],
            borderwidth=0,
        )
        style.map(
            "Treeview",
            background=[("selected", Palette.BG_BUTTON_HOVER)],
            foreground=[("selected", Palette.ACCENT)],
        )

    # ----------------------------------------------------------------------
    # Top Header Bar
    # ----------------------------------------------------------------------

    def _build_top_header(self):
        header = tk.Frame(self, bg=Palette.BG_SURFACE, height=52)
        header.pack(fill=tk.X, side=tk.TOP)
        header.pack_propagate(False)

        # Left: App Brand & Icon
        brand_frame = tk.Frame(header, bg=Palette.BG_SURFACE)
        brand_frame.pack(side=tk.LEFT, padx=16, pady=8)

        lbl_logo = tk.Label(brand_frame, text="⚡", font=("Segoe UI Symbol", 15), bg=Palette.BG_SURFACE, fg=Palette.ACCENT)
        lbl_logo.pack(side=tk.LEFT, padx=(0, 6))

        lbl_title = tk.Label(brand_frame, text="SERVER LAUNCHER", font=("Segoe UI", 12, "bold"), bg=Palette.BG_SURFACE, fg=Palette.TEXT_PRIMARY)
        lbl_title.pack(side=tk.LEFT)

        lbl_tag = tk.Label(brand_frame, text="WINDOWS", font=("Segoe UI", 9, "bold"), bg=Palette.BG_BUTTON, fg=Palette.CYAN, padx=6, pady=1)
        lbl_tag.pack(side=tk.LEFT, padx=8)

        # Right: Administrator Elevation Badge & Action Buttons
        actions_frame = tk.Frame(header, bg=Palette.BG_SURFACE)
        actions_frame.pack(side=tk.RIGHT, padx=16, pady=8)

        # Admin Badge
        is_elevated = is_admin()
        admin_bg = Palette.SUCCESS_BG if is_elevated else Palette.WARNING_BG
        admin_fg = Palette.SUCCESS_TEXT if is_elevated else Palette.WARNING_TEXT
        admin_icon = "🛡️ Administrator" if is_elevated else "🛡️ Standard User"

        admin_badge = tk.Label(
            actions_frame,
            text=admin_icon,
            font=("Segoe UI", 9, "bold"),
            bg=admin_bg,
            fg=admin_fg,
            padx=10,
            pady=3,
        )
        admin_badge.pack(side=tk.LEFT, padx=6)

        if not is_elevated:
            btn_elevate = tk.Button(
                actions_frame,
                text="⚡ Relaunch as Admin",
                font=("Segoe UI", 9, "bold"),
                bg=Palette.ACCENT,
                fg=Palette.ACCENT_TEXT,
                activebackground=Palette.ACCENT_HOVER,
                activeforeground=Palette.ACCENT_TEXT,
                relief=tk.FLAT,
                cursor="hand2",
                command=relaunch_as_admin,
                padx=10,
                pady=2,
            )
            btn_elevate.pack(side=tk.LEFT, padx=6)

        btn_start_all = tk.Button(
            actions_frame,
            text="▶ Start All",
            font=("Segoe UI", 9, "bold"),
            bg=Palette.SUCCESS_BG,
            fg=Palette.SUCCESS_TEXT,
            relief=tk.FLAT,
            cursor="hand2",
            command=self._start_all_servers,
            padx=10,
            pady=2,
        )
        btn_start_all.pack(side=tk.LEFT, padx=4)

        btn_stop_all = tk.Button(
            actions_frame,
            text="⏹ Stop All",
            font=("Segoe UI", 9, "bold"),
            bg=Palette.DANGER_BG,
            fg=Palette.DANGER_TEXT,
            relief=tk.FLAT,
            cursor="hand2",
            command=self._stop_all_servers,
            padx=10,
            pady=2,
        )
        btn_stop_all.pack(side=tk.LEFT, padx=4)

    # ----------------------------------------------------------------------
    # Main Navigation Tabs & Viewports
    # ----------------------------------------------------------------------

    def _build_main_views(self):
        self.notebook = ttk.Notebook(self)
        self.notebook.pack(fill=tk.BOTH, expand=True, padx=6, pady=(3, 4))

        # Tab 1: Dev Servers
        self.tab_servers = tk.Frame(self.notebook, bg=Palette.BG_CANVAS)
        self.notebook.add(self.tab_servers, text=" 🚀 Dev Servers ")
        self._build_servers_tab()

        # Tab 2: Windows Services Manager
        self.tab_services = tk.Frame(self.notebook, bg=Palette.BG_CANVAS)
        self.notebook.add(self.tab_services, text=" ⚙️ Windows Services ")
        self._build_services_tab()

        # Tab 3: Hardware Sensors & Resource Monitor
        self.tab_sensors = tk.Frame(self.notebook, bg=Palette.BG_CANVAS)
        self.notebook.add(self.tab_sensors, text=" 📊 Hardware Sensors ")
        self._build_sensors_tab()

        # Tab 4: PowerShell & Diagnostic Commands
        self.tab_powershell = tk.Frame(self.notebook, bg=Palette.BG_CANVAS)
        self.notebook.add(self.tab_powershell, text=" ⚡ PowerShell & Cmds ")
        self._build_powershell_tab()

        # Tab 5: Listening Ports Inspector
        self.tab_ports = tk.Frame(self.notebook, bg=Palette.BG_CANVAS)
        self.notebook.add(self.tab_ports, text=" 🌐 Ports & Listeners ")
        self._build_ports_tab()

        self.notebook.bind("<<NotebookTabChanged>>", self._on_tab_changed)

    def _on_tab_changed(self, event):
        selected_index = self.notebook.index("current")
        tab_names = ["servers", "services", "sensors", "powershell", "ports"]
        if selected_index < len(tab_names):
            self.active_tab_name = tab_names[selected_index]
            if self.active_tab_name == "services":
                self._refresh_services_list()
            elif self.active_tab_name == "sensors":
                self._refresh_sensors_view()
            elif self.active_tab_name == "ports":
                self._refresh_ports_view()

    # ----------------------------------------------------------------------
    # TAB 1: Dev Servers Launcher
    # ----------------------------------------------------------------------

    def _build_servers_tab(self):
        # Left: Server Cards Sidebar (320px)
        left_frame = tk.Frame(self.tab_servers, bg=Palette.BG_CANVAS, width=340)
        left_frame.pack(side=tk.LEFT, fill=tk.Y, padx=(0, 6), pady=4)
        left_frame.pack_propagate(False)

        # Sidebar Header
        top_bar = tk.Frame(left_frame, bg=Palette.BG_SURFACE)
        top_bar.pack(fill=tk.X, padx=4, pady=4)

        lbl_s = tk.Label(top_bar, text="DEV SERVERS", font=("Segoe UI", 10, "bold"), bg=Palette.BG_SURFACE, fg=Palette.TEXT_MUTED)
        lbl_s.pack(side=tk.LEFT, padx=8, pady=6)

        btn_add = tk.Button(
            top_bar,
            text="+ Add Server",
            font=("Segoe UI", 8, "bold"),
            bg=Palette.ACCENT,
            fg=Palette.ACCENT_TEXT,
            relief=tk.FLAT,
            cursor="hand2",
            command=self._show_add_server_dialog,
        )
        btn_add.pack(side=tk.RIGHT, padx=8, pady=4)

        # Scrollable Cards Canvas
        canvas_frame = tk.Frame(left_frame, bg=Palette.BG_CANVAS)
        canvas_frame.pack(fill=tk.BOTH, expand=True)

        self.server_canvas = tk.Canvas(canvas_frame, bg=Palette.BG_CANVAS, highlightthickness=0)
        scrollbar = ttk.Scrollbar(canvas_frame, orient="vertical", command=self.server_canvas.yview)
        self.server_cards_inner = tk.Frame(self.server_canvas, bg=Palette.BG_CANVAS)

        self.server_cards_inner.bind("<Configure>", lambda e: self.server_canvas.configure(scrollregion=self.server_canvas.bbox("all")))
        self.server_canvas.create_window((0, 0), window=self.server_cards_inner, anchor="nw", width=320)
        self.server_canvas.configure(yscrollcommand=scrollbar.set)

        self.server_canvas.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        scrollbar.pack(side=tk.RIGHT, fill=tk.Y)

        # Right: Log Terminal Output Viewer
        right_frame = tk.Frame(self.tab_servers, bg=Palette.BG_CARD)
        right_frame.pack(side=tk.RIGHT, fill=tk.BOTH, expand=True, padx=4, pady=2)

        # 1. Terminal Header
        term_header = tk.Frame(right_frame, bg=Palette.BG_SURFACE)
        term_header.pack(fill=tk.X, padx=6, pady=(4, 2))

        self.lbl_active_server = tk.Label(
            term_header,
            text="Select a server to monitor live console output",
            font=("Segoe UI", 11, "bold"),
            bg=Palette.BG_SURFACE,
            fg=Palette.TEXT_PRIMARY,
        )
        self.lbl_active_server.pack(side=tk.LEFT, padx=6, pady=4)

        btn_clear_log = tk.Button(
            term_header,
            text="Clear Logs",
            font=("Segoe UI", 8),
            bg=Palette.BG_BUTTON,
            fg=Palette.TEXT_SECONDARY,
            relief=tk.FLAT,
            command=self._clear_active_logs,
        )
        btn_clear_log.pack(side=tk.RIGHT, padx=6, pady=4)

        # 2. Bottom Command & Shortcuts Dock (PACKED FIRST SO IT SITS FIRMLY ELEVATED AT BOTTOM!)
        dock_frame = tk.Frame(right_frame, bg=Palette.BG_SURFACE)
        dock_frame.pack(side=tk.BOTTOM, fill=tk.X, padx=4, pady=(2, 6))

        # Shortcuts row
        sc_bar = tk.Frame(dock_frame, bg=Palette.BG_SURFACE)
        sc_bar.pack(fill=tk.X, padx=6, pady=(4, 2))

        tk.Label(
            sc_bar,
            text="⚡ Shortcuts:",
            font=("Segoe UI", 9, "bold"),
            bg=Palette.BG_SURFACE,
            fg=Palette.TEXT_MUTED,
        ).pack(side=tk.LEFT, padx=(0, 4))

        shortcuts = [
            ("h (help)", "h"),
            ("r (restart)", "r"),
            ("o (open)", "o"),
            ("u (url)", "u"),
            ("c (clear)", "c"),
            ("q (quit)", "q"),
        ]
        for sc_lbl, sc_cmd in shortcuts:
            btn_sc = tk.Button(
                sc_bar,
                text=sc_lbl,
                font=("Consolas", 8),
                bg=Palette.BG_BUTTON,
                fg=Palette.CYAN,
                relief=tk.FLAT,
                cursor="hand2",
                command=lambda cmd=sc_cmd: self._send_active_server_input(cmd),
                padx=5,
                pady=1,
            )
            btn_sc.pack(side=tk.LEFT, padx=2)

        # Input row
        inp_bar = tk.Frame(dock_frame, bg=Palette.BG_SURFACE)
        inp_bar.pack(fill=tk.X, padx=6, pady=(2, 4))

        tk.Label(
            inp_bar,
            text=">",
            font=("Consolas", 11, "bold"),
            bg=Palette.BG_SURFACE,
            fg=Palette.ACCENT,
        ).pack(side=tk.LEFT, padx=(2, 4))

        self.entry_server_cmd = tk.Entry(
            inp_bar,
            font=("Consolas", 10),
            bg=Palette.BG_INPUT,
            fg=Palette.TEXT_PRIMARY,
            insertbackground=Palette.TEXT_PRIMARY,
            relief=tk.FLAT,
        )
        self.entry_server_cmd.pack(side=tk.LEFT, fill=tk.X, expand=True, padx=4, ipady=3)
        self.entry_server_cmd.bind("<Return>", lambda e: self._send_active_server_input_from_entry())

        btn_send = tk.Button(
            inp_bar,
            text="Send",
            font=("Segoe UI", 9, "bold"),
            bg=Palette.ACCENT,
            fg=Palette.ACCENT_TEXT,
            relief=tk.FLAT,
            cursor="hand2",
            command=self._send_active_server_input_from_entry,
            padx=8,
            pady=1,
        )
        btn_send.pack(side=tk.LEFT, padx=3)

        btn_ctrlc = tk.Button(
            inp_bar,
            text="Ctrl+C",
            font=("Segoe UI", 8, "bold"),
            bg=Palette.WARNING_BG,
            fg=Palette.WARNING_TEXT,
            relief=tk.FLAT,
            cursor="hand2",
            command=self._interrupt_active_server,
            padx=6,
            pady=1,
        )
        btn_ctrlc.pack(side=tk.LEFT, padx=3)

        btn_clear = tk.Button(
            inp_bar,
            text="Clear",
            font=("Segoe UI", 8),
            bg=Palette.BG_BUTTON,
            fg=Palette.TEXT_MUTED,
            relief=tk.FLAT,
            cursor="hand2",
            command=self._clear_active_logs,
            padx=6,
            pady=1,
        )
        btn_clear.pack(side=tk.LEFT, padx=3)

        btn_console = tk.Button(
            inp_bar,
            text="Console",
            font=("Segoe UI", 8),
            bg=Palette.BG_BUTTON,
            fg=Palette.CYAN,
            relief=tk.FLAT,
            cursor="hand2",
            command=self._open_active_server_console,
            padx=6,
            pady=1,
        )
        btn_console.pack(side=tk.LEFT, padx=3)

        # 3. Middle Terminal Monospace Text Well (EXPANDS TO FILL REMAINING SPACE)
        term_middle = tk.Frame(right_frame, bg=Palette.BG_CANVAS)
        term_middle.pack(side=tk.TOP, fill=tk.BOTH, expand=True, padx=4, pady=2)

        self.log_text = tk.Text(
            term_middle,
            bg=Palette.BG_CANVAS,
            fg=Palette.TEXT_PRIMARY,
            font=("Consolas", 10),
            wrap=tk.WORD,
            borderwidth=0,
            padx=12,
            pady=(8, 24), # 24px bottom clearance so bottom lines are never cut off
        )
        log_scroll = ttk.Scrollbar(term_middle, orient="vertical", command=self.log_text.yview)
        self.log_text.configure(yscrollcommand=log_scroll.set)

        self.log_text.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        log_scroll.pack(side=tk.RIGHT, fill=tk.Y)

        self._render_server_cards()

    def _render_server_cards(self):
        for widget in self.server_cards_inner.winfo_children():
            widget.destroy()

        if not self.servers:
            lbl_empty = tk.Label(
                self.server_cards_inner,
                text="No servers configured yet.\nClick '+ Add Server' to get started.",
                font=("Segoe UI", 9),
                bg=Palette.BG_CANVAS,
                fg=Palette.TEXT_MUTED,
                pady=40,
            )
            lbl_empty.pack(fill=tk.X)
            return

        for srv in self.servers:
            self._render_single_server_card(srv)

    def _render_single_server_card(self, srv: ManagedServer):
        is_selected = (self.active_server_name == srv.config.name)
        card_bg = Palette.BG_CARD_HOVER if is_selected else Palette.BG_CARD
        border_col = Palette.ACCENT if is_selected else Palette.BORDER

        card = tk.Frame(self.server_cards_inner, bg=card_bg, highlightbackground=border_col, highlightthickness=1)
        card.pack(fill=tk.X, padx=4, pady=4)

        # Row 1: Status indicator + Name + Port badge
        r1 = tk.Frame(card, bg=card_bg)
        r1.pack(fill=tk.X, padx=10, pady=(8, 4))

        status_colors = {
            "Running": (Palette.SUCCESS, "●"),
            "Starting": (Palette.WARNING, "⟳"),
            "Error": (Palette.DANGER, "✕"),
            "Stopped": (Palette.TEXT_MUTED, "○"),
        }
        col, sym = status_colors.get(srv.status, (Palette.TEXT_MUTED, "○"))

        lbl_status = tk.Label(r1, text=sym, font=("Segoe UI Symbol", 12), bg=card_bg, fg=col)
        lbl_status.pack(side=tk.LEFT, padx=(0, 4))

        lbl_name = tk.Label(r1, text=srv.config.name, font=("Segoe UI", 10, "bold"), bg=card_bg, fg=Palette.TEXT_PRIMARY)
        lbl_name.pack(side=tk.LEFT)

        if srv.config.port > 0:
            lbl_port = tk.Label(
                r1,
                text=f":{srv.config.port}",
                font=("Segoe UI", 9, "bold"),
                bg=Palette.CYAN_BG,
                fg=Palette.CYAN_TEXT,
                padx=4,
            )
            lbl_port.pack(side=tk.RIGHT)

        # Row 2: Command snippet
        r2 = tk.Frame(card, bg=card_bg)
        r2.pack(fill=tk.X, padx=10, pady=2)
        lbl_cmd = tk.Label(
            r2,
            text=srv.config.cmd,
            font=("Consolas", 8),
            bg=Palette.BG_INPUT,
            fg=Palette.TEXT_SECONDARY,
            anchor="w",
            padx=4,
            pady=2,
        )
        lbl_cmd.pack(fill=tk.X)

        # Row 3: Control Buttons
        r3 = tk.Frame(card, bg=card_bg)
        r3.pack(fill=tk.X, padx=10, pady=(6, 8))

        if srv.status == "Running":
            btn_stop = tk.Button(
                r3,
                text="⏹ Stop",
                font=("Segoe UI", 8, "bold"),
                bg=Palette.DANGER_BG,
                fg=Palette.DANGER_TEXT,
                relief=tk.FLAT,
                cursor="hand2",
                command=srv.stop,
                padx=8,
            )
            btn_stop.pack(side=tk.LEFT, padx=(0, 4))

            btn_restart = tk.Button(
                r3,
                text="⟳ Restart",
                font=("Segoe UI", 8, "bold"),
                bg=Palette.CYAN_BG,
                fg=Palette.CYAN_TEXT,
                relief=tk.FLAT,
                cursor="hand2",
                command=srv.restart,
                padx=8,
            )
            btn_restart.pack(side=tk.LEFT, padx=4)
        else:
            btn_start = tk.Button(
                r3,
                text="▶ Start",
                font=("Segoe UI", 8, "bold"),
                bg=Palette.SUCCESS_BG,
                fg=Palette.SUCCESS_TEXT,
                relief=tk.FLAT,
                cursor="hand2",
                command=srv.start,
                padx=8,
            )
            btn_start.pack(side=tk.LEFT, padx=(0, 4))

        btn_select = tk.Button(
            r3,
            text="Logs ↗",
            font=("Segoe UI", 8),
            bg=Palette.BG_BUTTON,
            fg=Palette.TEXT_SECONDARY,
            relief=tk.FLAT,
            cursor="hand2",
            command=lambda n=srv.config.name: self._select_server_logs(n),
            padx=6,
        )
        btn_select.pack(side=tk.RIGHT)

        # Click card to select
        card.bind("<Button-1>", lambda e, n=srv.config.name: self._select_server_logs(n))
        lbl_name.bind("<Button-1>", lambda e, n=srv.config.name: self._select_server_logs(n))

    def _select_server_logs(self, name: str):
        self.active_server_name = name
        self.lbl_active_server.configure(text=f"Live Console Output — {name}")
        self.log_text.delete("1.0", tk.END)

        srv = next((s for s in self.servers if s.config.name == name), None)
        if srv:
            for line in srv.logs:
                self.log_text.insert(tk.END, line)
            self.log_text.see(tk.END)

        self._render_server_cards()

    def _clear_active_logs(self):
        self.log_text.delete("1.0", tk.END)
        srv = next((s for s in self.servers if s.config.name == self.active_server_name), None)
        if srv:
            srv.clear_logs()

    def _send_active_server_input(self, text: str):
        if not text:
            return
        srv = next((s for s in self.servers if s.config.name == self.active_server_name), None)
        if srv:
            srv.send_input(text)

    def _send_active_server_input_from_entry(self):
        val = self.entry_server_cmd.get().strip()
        if not val:
            return
        self.entry_server_cmd.delete(0, tk.END)
        self._send_active_server_input(val)

    def _interrupt_active_server(self):
        srv = next((s for s in self.servers if s.config.name == self.active_server_name), None)
        if srv:
            srv.send_interrupt()

    def _open_active_server_console(self):
        srv = next((s for s in self.servers if s.config.name == self.active_server_name), None)
        if srv and srv.config.dir:
            if IS_WINDOWS:
                subprocess.Popen(["cmd.exe", "/K"], cwd=srv.config.dir, creationflags=CREATE_NEW_CONSOLE)
            else:
                subprocess.Popen(["sh"], cwd=srv.config.dir)

    def _start_all_servers(self):
        for srv in self.servers:
            srv.start()

    def _stop_all_servers(self):
        for srv in self.servers:
            srv.stop()

    # ----------------------------------------------------------------------
    # TAB 2: Windows Services Manager
    # ----------------------------------------------------------------------

    def _build_services_tab(self):
        container = tk.Frame(self.tab_services, bg=Palette.BG_CANVAS)
        container.pack(fill=tk.BOTH, expand=True, padx=8, pady=4)

        # Header Overview & Actions
        header_plate = tk.Frame(container, bg=Palette.BG_CARD)
        header_plate.pack(fill=tk.X, pady=(0, 6))

        r_top = tk.Frame(header_plate, bg=Palette.BG_CARD)
        r_top.pack(fill=tk.X, padx=12, pady=10)

        lbl_title = tk.Label(
            r_top,
            text="Windows Background Services Manager (Service Control Manager)",
            font=("Segoe UI", 12, "bold"),
            bg=Palette.BG_CARD,
            fg=Palette.TEXT_PRIMARY,
        )
        lbl_title.pack(side=tk.LEFT)

        self.lbl_services_count = tk.Label(
            r_top,
            text="Scanning services...",
            font=("Segoe UI", 9),
            bg=Palette.BG_BUTTON,
            fg=Palette.TEXT_SECONDARY,
            padx=8,
            pady=2,
        )
        self.lbl_services_count.pack(side=tk.LEFT, padx=12)

        btn_ref = tk.Button(
            r_top,
            text="🔄 Refresh Services",
            font=("Segoe UI", 9, "bold"),
            bg=Palette.ACCENT,
            fg=Palette.ACCENT_TEXT,
            relief=tk.FLAT,
            cursor="hand2",
            command=self._refresh_services_list,
            padx=10,
        )
        btn_ref.pack(side=tk.RIGHT, padx=4)

        btn_create = tk.Button(
            r_top,
            text="+ Register New Service",
            font=("Segoe UI", 9),
            bg=Palette.BG_BUTTON,
            fg=Palette.TEXT_PRIMARY,
            relief=tk.FLAT,
            cursor="hand2",
            command=self._show_create_service_dialog,
            padx=10,
        )
        btn_create.pack(side=tk.RIGHT, padx=4)

        # Filters Bar
        filters_bar = tk.Frame(container, bg=Palette.BG_SURFACE)
        filters_bar.pack(fill=tk.X, pady=(0, 6), padx=2)

        lbl_search = tk.Label(filters_bar, text="Search:", font=("Segoe UI", 9), bg=Palette.BG_SURFACE, fg=Palette.TEXT_MUTED)
        lbl_search.pack(side=tk.LEFT, padx=(10, 4), pady=6)

        self.entry_services_search = tk.Entry(
            filters_bar,
            textvariable=self.search_query,
            font=("Segoe UI", 9),
            bg=Palette.BG_INPUT,
            fg=Palette.TEXT_PRIMARY,
            insertbackground=Palette.TEXT_PRIMARY,
            relief=tk.FLAT,
            width=28,
        )
        self.entry_services_search.pack(side=tk.LEFT, padx=4, pady=6)
        self.entry_services_search.bind("<KeyRelease>", lambda e: self._filter_services_view())

        # Status filter dropdown
        lbl_st = tk.Label(filters_bar, text="Status:", font=("Segoe UI", 9), bg=Palette.BG_SURFACE, fg=Palette.TEXT_MUTED)
        lbl_st.pack(side=tk.LEFT, padx=(16, 4))
        cb_st = ttk.Combobox(filters_bar, textvariable=self.services_filter_status, values=["All", "Running", "Stopped", "Paused"], width=10, state="readonly")
        cb_st.pack(side=tk.LEFT, padx=4)
        cb_st.bind("<<ComboboxSelected>>", lambda e: self._filter_services_view())

        # Startup type filter dropdown
        lbl_sm = tk.Label(filters_bar, text="Startup Type:", font=("Segoe UI", 9), bg=Palette.BG_SURFACE, fg=Palette.TEXT_MUTED)
        lbl_sm.pack(side=tk.LEFT, padx=(16, 4))
        cb_sm = ttk.Combobox(filters_bar, textvariable=self.services_filter_startup, values=["All", "Auto", "Manual", "Disabled"], width=10, state="readonly")
        cb_sm.pack(side=tk.LEFT, padx=4)
        cb_sm.bind("<<ComboboxSelected>>", lambda e: self._filter_services_view())

        # Services Treeview
        tree_frame = tk.Frame(container, bg=Palette.BG_CARD)
        tree_frame.pack(fill=tk.BOTH, expand=True)

        cols = ("status", "name", "display_name", "startup", "pid", "path")
        self.services_tree = ttk.Treeview(tree_frame, columns=cols, show="headings", selectmode="browse")

        self.services_tree.heading("status", text="Status")
        self.services_tree.heading("name", text="Service Name")
        self.services_tree.heading("display_name", text="Display Name")
        self.services_tree.heading("startup", text="Startup Type")
        self.services_tree.heading("pid", text="PID")
        self.services_tree.heading("path", text="Executable Binary Path")

        # Fixed permanent column widths
        self.services_tree.column("status", width=100, anchor="center")
        self.services_tree.column("name", width=160, anchor="w")
        self.services_tree.column("display_name", width=260, anchor="w")
        self.services_tree.column("startup", width=110, anchor="center")
        self.services_tree.column("pid", width=80, anchor="center")
        self.services_tree.column("path", width=380, anchor="w")

        tree_scroll_y = ttk.Scrollbar(tree_frame, orient="vertical", command=self.services_tree.yview)
        tree_scroll_x = ttk.Scrollbar(tree_frame, orient="horizontal", command=self.services_tree.xview)
        self.services_tree.configure(yscrollcommand=tree_scroll_y.set, xscrollcommand=tree_scroll_x.set)

        self.services_tree.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        tree_scroll_y.pack(side=tk.RIGHT, fill=tk.Y)
        tree_scroll_x.pack(side=tk.BOTTOM, fill=tk.X)

        # Service Action Controls Footer
        footer = tk.Frame(container, bg=Palette.BG_SURFACE)
        footer.pack(fill=tk.X, pady=(6, 0))

        btn_act_start = tk.Button(footer, text="▶ Start Service", font=("Segoe UI", 9, "bold"), bg=Palette.SUCCESS_BG, fg=Palette.SUCCESS_TEXT, relief=tk.FLAT, command=self._action_start_selected_service, padx=10, pady=4)
        btn_act_start.pack(side=tk.LEFT, padx=6, pady=6)

        btn_act_stop = tk.Button(footer, text="⏹ Stop Service", font=("Segoe UI", 9, "bold"), bg=Palette.DANGER_BG, fg=Palette.DANGER_TEXT, relief=tk.FLAT, command=self._action_stop_selected_service, padx=10, pady=4)
        btn_act_stop.pack(side=tk.LEFT, padx=6, pady=6)

        btn_act_restart = tk.Button(footer, text="⟳ Restart Service", font=("Segoe UI", 9, "bold"), bg=Palette.CYAN_BG, fg=Palette.CYAN_TEXT, relief=tk.FLAT, command=self._action_restart_selected_service, padx=10, pady=4)
        btn_act_restart.pack(side=tk.LEFT, padx=6, pady=6)

        # Startup Type Changer
        lbl_set_st = tk.Label(footer, text="Set Startup:", font=("Segoe UI", 9), bg=Palette.BG_SURFACE, fg=Palette.TEXT_MUTED)
        lbl_set_st.pack(side=tk.LEFT, padx=(16, 4))

        for st_type in ["Automatic", "Manual", "Disabled"]:
            btn_st = tk.Button(footer, text=st_type, font=("Segoe UI", 8), bg=Palette.BG_BUTTON, fg=Palette.TEXT_PRIMARY, relief=tk.FLAT, command=lambda t=st_type: self._action_set_startup_selected_service(t), padx=6, pady=2)
            btn_st.pack(side=tk.LEFT, padx=2)

        btn_act_details = tk.Button(footer, text="ℹ️ Inspect Service Details", font=("Segoe UI", 9), bg=Palette.BG_BUTTON, fg=Palette.TEXT_SECONDARY, relief=tk.FLAT, command=self._show_service_details_dialog, padx=10, pady=4)
        btn_act_details.pack(side=tk.RIGHT, padx=6, pady=6)

    def _refresh_services_list(self):
        self.lbl_services_count.configure(text="Querying Windows services...")
        threading.Thread(target=self._worker_fetch_services, daemon=True).start()

    def _worker_fetch_services(self):
        services = self.service_mgr.query_services()
        self.after(0, lambda: self._populate_services_tree(services))

    def _populate_services_tree(self, services: List[WindowsServiceInfo]):
        running_cnt = sum(1 for s in services if s.status.lower() == "running")
        total_cnt = len(services)
        self.lbl_services_count.configure(text=f"{total_cnt} Total Services ({running_cnt} Running)")
        self._filter_services_view()

    def _filter_services_view(self):
        q = self.search_query.get().strip().lower()
        filter_status = self.services_filter_status.get().lower()
        filter_startup = self.services_filter_startup.get().lower()

        self.services_tree.delete(*self.services_tree.get_children())

        for s in self.service_mgr.cached_services:
            if q and (q not in s.name.lower() and q not in s.display_name.lower() and q not in s.description.lower()):
                continue
            if filter_status != "all" and filter_status not in s.status.lower():
                continue
            if filter_startup != "all" and filter_startup not in s.startup_type.lower():
                continue

            status_display = f"● {s.status}" if s.status.lower() == "running" else f"○ {s.status}"
            pid_display = str(s.pid) if s.pid > 0 else "—"

            self.services_tree.insert(
                "",
                tk.END,
                iid=s.name,
                values=(status_display, s.name, s.display_name, s.startup_type, pid_display, s.path_name),
            )

    def _get_selected_service(self) -> Optional[WindowsServiceInfo]:
        sel = self.services_tree.selection()
        if not sel:
            messagebox.showinfo("Selection Required", "Please select a Windows service from the list first.")
            return None
        service_name = sel[0]
        return next((s for s in self.service_mgr.cached_services if s.name == service_name), None)

    def _action_start_selected_service(self):
        srv = self._get_selected_service()
        if not srv:
            return
        threading.Thread(target=self._worker_service_action, args=(self.service_mgr.start_service, srv.name, "Start"), daemon=True).start()

    def _action_stop_selected_service(self):
        srv = self._get_selected_service()
        if not srv:
            return
        threading.Thread(target=self._worker_service_action, args=(self.service_mgr.stop_service, srv.name, "Stop"), daemon=True).start()

    def _action_restart_selected_service(self):
        srv = self._get_selected_service()
        if not srv:
            return
        threading.Thread(target=self._worker_service_action, args=(self.service_mgr.restart_service, srv.name, "Restart"), daemon=True).start()

    def _action_set_startup_selected_service(self, startup_mode: str):
        srv = self._get_selected_service()
        if not srv:
            return
        threading.Thread(target=self._worker_service_action, args=(lambda n: self.service_mgr.set_startup_type(n, startup_mode), srv.name, f"Set Startup to {startup_mode}"), daemon=True).start()

    def _worker_service_action(self, func, service_name: str, action_name: str):
        success, msg = func(service_name)
        if success:
            self.after(0, self._refresh_services_list)
        else:
            self.after(0, lambda: messagebox.showerror(f"{action_name} Service Error", f"Failed to {action_name.lower()} service '{service_name}':\n{msg}"))

    def _show_service_details_dialog(self):
        srv = self._get_selected_service()
        if not srv:
            return

        win = tk.Toplevel(self)
        win.title(f"Windows Service: {srv.display_name}")
        win.geometry("640x480")
        win.configure(bg=Palette.BG_CARD)

        pad = 14
        tk.Label(win, text=srv.display_name, font=("Segoe UI", 12, "bold"), bg=Palette.BG_CARD, fg=Palette.TEXT_PRIMARY).pack(anchor="w", padx=pad, pady=(pad, 2))
        tk.Label(win, text=f"Service Name: {srv.name}", font=("Segoe UI", 10), bg=Palette.BG_CARD, fg=Palette.CYAN).pack(anchor="w", padx=pad)

        info_box = tk.Frame(win, bg=Palette.BG_INPUT)
        info_box.pack(fill=tk.BOTH, expand=True, padx=pad, pady=pad)

        details_text = (
            f"Status:       {srv.status}\n"
            f"Startup Type: {srv.startup_type}\n"
            f"Process ID:   {srv.pid}\n"
            f"Binary Path:  {srv.path_name}\n\n"
            f"Description:\n{srv.description or 'No description provided by service vendor.'}\n"
        )
        t = tk.Text(info_box, bg=Palette.BG_INPUT, fg=Palette.TEXT_PRIMARY, font=("Consolas", 10), wrap=tk.WORD, borderwidth=0, padx=10, pady=10)
        t.insert("1.0", details_text)
        t.configure(state="disabled")
        t.pack(fill=tk.BOTH, expand=True)

        tk.Button(win, text="Close", font=("Segoe UI", 9), bg=Palette.BG_BUTTON, fg=Palette.TEXT_PRIMARY, command=win.destroy, padx=14, pady=4).pack(pady=(0, pad))

    def _show_create_service_dialog(self):
        win = tk.Toplevel(self)
        win.title("Register New Windows Service")
        win.geometry("520x360")
        win.configure(bg=Palette.BG_CARD)

        pad = 12
        tk.Label(win, text="Register Windows Service (sc.exe create)", font=("Segoe UI", 11, "bold"), bg=Palette.BG_CARD, fg=Palette.TEXT_PRIMARY).pack(anchor="w", padx=pad, pady=pad)

        form = tk.Frame(win, bg=Palette.BG_CARD)
        form.pack(fill=tk.X, padx=pad)

        tk.Label(form, text="Service Name (Internal):", bg=Palette.BG_CARD, fg=Palette.TEXT_SECONDARY).grid(row=0, column=0, sticky="w", pady=4)
        ent_name = tk.Entry(form, bg=Palette.BG_INPUT, fg=Palette.TEXT_PRIMARY, insertbackground=Palette.TEXT_PRIMARY, width=36)
        ent_name.grid(row=0, column=1, pady=4)

        tk.Label(form, text="Display Name:", bg=Palette.BG_CARD, fg=Palette.TEXT_SECONDARY).grid(row=1, column=0, sticky="w", pady=4)
        ent_disp = tk.Entry(form, bg=Palette.BG_INPUT, fg=Palette.TEXT_PRIMARY, insertbackground=Palette.TEXT_PRIMARY, width=36)
        ent_disp.grid(row=1, column=1, pady=4)

        tk.Label(form, text="Binary Executable Path:", bg=Palette.BG_CARD, fg=Palette.TEXT_SECONDARY).grid(row=2, column=0, sticky="w", pady=4)
        ent_path = tk.Entry(form, bg=Palette.BG_INPUT, fg=Palette.TEXT_PRIMARY, insertbackground=Palette.TEXT_PRIMARY, width=36)
        ent_path.grid(row=2, column=1, pady=4)

        tk.Label(form, text="Startup Mode:", bg=Palette.BG_CARD, fg=Palette.TEXT_SECONDARY).grid(row=3, column=0, sticky="w", pady=4)
        cb_mode = ttk.Combobox(form, values=["auto", "demand", "disabled"], width=12, state="readonly")
        cb_mode.set("auto")
        cb_mode.grid(row=3, column=1, sticky="w", pady=4)

        def do_create():
            s_name = ent_name.get().strip()
            s_disp = ent_disp.get().strip() or s_name
            s_path = ent_path.get().strip()
            s_mode = cb_mode.get().strip()
            if not s_name or not s_path:
                messagebox.showerror("Validation Error", "Service name and executable path are required.")
                return

            succ, msg = self.service_mgr.create_service(s_name, s_disp, s_path, s_mode)
            if succ:
                messagebox.showinfo("Success", f"Service '{s_name}' registered successfully.")
                win.destroy()
                self._refresh_services_list()
            else:
                messagebox.showerror("Error", f"Failed to register service:\n{msg}")

        btn_frame = tk.Frame(win, bg=Palette.BG_CARD)
        btn_frame.pack(fill=tk.X, padx=pad, pady=pad)
        tk.Button(btn_frame, text="Create Service", font=("Segoe UI", 9, "bold"), bg=Palette.ACCENT, fg=Palette.ACCENT_TEXT, command=do_create, padx=12, pady=4).pack(side=tk.RIGHT)
        tk.Button(btn_frame, text="Cancel", font=("Segoe UI", 9), bg=Palette.BG_BUTTON, fg=Palette.TEXT_PRIMARY, command=win.destroy, padx=10, pady=4).pack(side=tk.RIGHT, padx=6)

    # ----------------------------------------------------------------------
    # TAB 3: Hardware Sensors & System Resource Monitor
    # ----------------------------------------------------------------------

    def _build_sensors_tab(self):
        container = tk.Frame(self.tab_sensors, bg=Palette.BG_CANVAS)
        container.pack(fill=tk.BOTH, expand=True, padx=8, pady=4)

        # ── 1. Top System Overview Bar Card (Matches User Screenshot) ──
        overview_card = tk.Frame(container, bg=Palette.BG_CARD, highlightbackground=Palette.BORDER, highlightthickness=1)
        overview_card.pack(fill=tk.X, pady=(0, 6))

        r_top = tk.Frame(overview_card, bg=Palette.BG_CARD)
        r_top.pack(fill=tk.X, padx=14, pady=(10, 4))

        lbl_icon = tk.Label(r_top, text="📊", font=("Segoe UI Symbol", 14), bg=Palette.BG_CARD, fg=Palette.ACCENT)
        lbl_icon.pack(side=tk.LEFT, padx=(0, 6))

        lbl_head = tk.Label(r_top, text="Hardware Sensors & System Resource Monitor", font=("Segoe UI", 12, "bold"), bg=Palette.BG_CARD, fg=Palette.TEXT_PRIMARY)
        lbl_head.pack(side=tk.LEFT)

        lbl_live = tk.Label(r_top, text="● Live (2s refresh)", font=("Segoe UI", 9), bg=Palette.BG_CARD, fg=Palette.SUCCESS)
        lbl_live.pack(side=tk.LEFT, padx=10)

        btn_ref_now = tk.Button(
            r_top,
            text="🔄 Refresh Now",
            font=("Segoe UI", 9, "bold"),
            bg=Palette.ACCENT,
            fg=Palette.ACCENT_TEXT,
            relief=tk.FLAT,
            cursor="hand2",
            command=self._refresh_sensors_view,
            padx=12,
            pady=2,
        )
        btn_ref_now.pack(side=tk.RIGHT)

        # Telemetry detail pills line
        self.lbl_telemetry_line = tk.Label(
            overview_card,
            text="Gathering Windows hardware telemetry and processor clock sensors...",
            font=("Consolas", 9),
            bg=Palette.BG_CARD,
            fg=Palette.CYAN,
            anchor="w",
            padx=14,
            pady=6,
        )
        self.lbl_telemetry_line.pack(fill=tk.X)

        # ── 2. Subtabs Bar: [ System ]  [ Top Apps ] ──
        subtabs_bar = tk.Frame(container, bg=Palette.BG_CANVAS)
        subtabs_bar.pack(fill=tk.X, pady=6)

        self.btn_subtab_system = tk.Button(
            subtabs_bar,
            text="⏱ System Sensors",
            font=("Segoe UI", 10, "bold"),
            bg=Palette.ACCENT,
            fg=Palette.ACCENT_TEXT,
            relief=tk.FLAT,
            cursor="hand2",
            command=lambda: self._switch_sensors_subtab("system"),
            padx=14,
            pady=4,
        )
        self.btn_subtab_system.pack(side=tk.LEFT, padx=(0, 6))

        self.btn_subtab_top_apps = tk.Button(
            subtabs_bar,
            text="⚡ Top Apps",
            font=("Segoe UI", 10, "bold"),
            bg=Palette.BG_BUTTON,
            fg=Palette.TEXT_SECONDARY,
            relief=tk.FLAT,
            cursor="hand2",
            command=lambda: self._switch_sensors_subtab("top_apps"),
            padx=14,
            pady=4,
        )
        self.btn_subtab_top_apps.pack(side=tk.LEFT)

        # ── 3. Subtab Container Frames ──
        self.frame_sensors_system = tk.Frame(container, bg=Palette.BG_CANVAS)
        self.frame_sensors_system.pack(fill=tk.BOTH, expand=True)

        self.frame_sensors_top_apps = tk.Frame(container, bg=Palette.BG_CANVAS)

        self._build_sensors_system_subtab()
        self._build_sensors_top_apps_subtab()

    def _switch_sensors_subtab(self, subtab: str):
        self.sensors_subtab = subtab
        if subtab == "system":
            self.frame_sensors_top_apps.pack_forget()
            self.frame_sensors_system.pack(fill=tk.BOTH, expand=True)
            self.btn_subtab_system.configure(bg=Palette.ACCENT, fg=Palette.ACCENT_TEXT)
            self.btn_subtab_top_apps.configure(bg=Palette.BG_BUTTON, fg=Palette.TEXT_SECONDARY)
        else:
            self.top_apps_visited = True
            self.frame_sensors_system.pack_forget()
            self.frame_sensors_top_apps.pack(fill=tk.BOTH, expand=True)
            self.btn_subtab_top_apps.configure(bg=Palette.ACCENT, fg=Palette.ACCENT_TEXT)
            self.btn_subtab_system.configure(bg=Palette.BG_BUTTON, fg=Palette.TEXT_SECONDARY)
            self._refresh_sensors_view()

    def _build_sensors_system_subtab(self):
        # Scrollable area for system sensor metrics
        canvas = tk.Canvas(self.frame_sensors_system, bg=Palette.BG_CANVAS, highlightthickness=0)
        scrollbar = ttk.Scrollbar(self.frame_sensors_system, orient="vertical", command=canvas.yview)
        inner = tk.Frame(canvas, bg=Palette.BG_CANVAS)

        inner.bind("<Configure>", lambda e: canvas.configure(scrollregion=canvas.bbox("all")))
        canvas.create_window((0, 0), window=inner, anchor="nw", width=1200)
        canvas.configure(yscrollcommand=scrollbar.set)

        canvas.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        scrollbar.pack(side=tk.RIGHT, fill=tk.Y)

        self.sensors_system_inner = inner

    def _build_sensors_top_apps_subtab(self):
        card = tk.Frame(self.frame_sensors_top_apps, bg=Palette.BG_CARD, highlightbackground=Palette.BORDER, highlightthickness=1)
        card.pack(fill=tk.BOTH, expand=True)

        top_bar = tk.Frame(card, bg=Palette.BG_CARD)
        top_bar.pack(fill=tk.X, padx=12, pady=10)

        lbl_t = tk.Label(top_bar, text="⚡ Processes (Task Manager)", font=("Segoe UI", 11, "bold"), bg=Palette.BG_CARD, fg=Palette.TEXT_PRIMARY)
        lbl_t.pack(side=tk.LEFT)

        self.lbl_top_apps_count = tk.Label(top_bar, text="0 processes", font=("Segoe UI", 9), bg=Palette.BG_BUTTON, fg=Palette.TEXT_SECONDARY, padx=8, pady=2)
        self.lbl_top_apps_count.pack(side=tk.LEFT, padx=10)

        # End Task Button (Windows Task Manager style)
        btn_end = tk.Button(top_bar, text="✕ End task", font=("Segoe UI", 9, "bold"), bg=Palette.DANGER, fg=Palette.WHITE, padx=10, pady=2, relief=tk.FLAT, cursor="hand2", command=self._kill_selected_top_app)
        btn_end.pack(side=tk.RIGHT, padx=6)

        # Search Entry
        ent_search = tk.Entry(top_bar, textvariable=self.top_apps_search_var, font=("Segoe UI", 9), bg=Palette.BG_BUTTON, fg=Palette.TEXT_PRIMARY, insertbackground=Palette.TEXT_PRIMARY, width=22)
        ent_search.pack(side=tk.RIGHT, padx=4)
        tk.Label(top_bar, text="🔍", font=("Segoe UI", 9), bg=Palette.BG_CARD, fg=Palette.TEXT_MUTED).pack(side=tk.RIGHT)
        self.top_apps_search_var.trace_add("write", lambda *_: self._render_top_apps_list())

        # Permanent column width Treeview for Top Apps
        tree_frame = tk.Frame(card, bg=Palette.BG_CARD)
        tree_frame.pack(fill=tk.BOTH, expand=True, padx=12, pady=(0, 10))

        cols = ("pid", "name", "status", "cpu", "mem")
        self.top_apps_tree = ttk.Treeview(tree_frame, columns=cols, show="headings", selectmode="browse")

        self.top_apps_tree.heading("pid", text="PID", command=lambda: self._sort_top_apps("pid"))
        self.top_apps_tree.heading("name", text="Application Name", command=lambda: self._sort_top_apps("name"))
        self.top_apps_tree.heading("status", text="Status", command=lambda: self._sort_top_apps("status"))
        self.top_apps_tree.heading("cpu", text="CPU Time / Load ▼", command=lambda: self._sort_top_apps("cpu"))
        self.top_apps_tree.heading("mem", text="Memory (RAM)", command=lambda: self._sort_top_apps("mem"))

        # Exact permanent column widths: Never jumps or resizes
        self.top_apps_tree.column("pid", width=80, anchor="center")
        self.top_apps_tree.column("name", width=260, anchor="w")
        self.top_apps_tree.column("status", width=110, anchor="center")
        self.top_apps_tree.column("cpu", width=180, anchor="w")
        self.top_apps_tree.column("mem", width=200, anchor="w")

        tree_scroll_y = ttk.Scrollbar(tree_frame, orient="vertical", command=self.top_apps_tree.yview)
        tree_scroll_x = ttk.Scrollbar(tree_frame, orient="horizontal", command=self.top_apps_tree.xview)
        self.top_apps_tree.configure(yscrollcommand=tree_scroll_y.set, xscrollcommand=tree_scroll_x.set)

        self.top_apps_tree.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        tree_scroll_y.pack(side=tk.RIGHT, fill=tk.Y)
        tree_scroll_x.pack(side=tk.BOTTOM, fill=tk.X)

    def _kill_selected_top_app(self):
        sel = self.top_apps_tree.selection()
        if not sel:
            messagebox.showinfo("Task Manager", "Please click a process in the table to select it before clicking End task.", parent=self.root)
            return
        pid_str = sel[0]
        try:
            pid = int(pid_str)
            if os.name == "nt":
                subprocess.run(["taskkill", "/F", "/T", "/PID", str(pid)], capture_output=True)
            else:
                os.kill(pid, signal.SIGKILL)
            self.top_apps_tree.delete(pid_str)
            self.last_top_apps = [a for a in self.last_top_apps if a.pid != pid]
            messagebox.showinfo("Task Manager", f"Process {pid} has been terminated.", parent=self.root)
        except Exception as e:
            messagebox.showerror("Task Manager Error", f"Failed to terminate process {pid_str}: {e}", parent=self.root)

    def _sort_top_apps(self, col: str):
        if self.top_apps_sort_col == col:
            self.top_apps_sort_desc = not self.top_apps_sort_desc
        else:
            self.top_apps_sort_col = col
            self.top_apps_sort_desc = (col != "name")
        self._render_top_apps_list()

    def _render_top_apps_list(self):
        if not self.last_top_apps:
            return
        q = self.top_apps_search_var.get().strip().lower()
        filtered = [a for a in self.last_top_apps if not q or q in a.name.lower() or q in str(a.pid) or q in a.status.lower()]
        col = self.top_apps_sort_col
        desc = self.top_apps_sort_desc
        if col == "pid":
            filtered.sort(key=lambda a: a.pid, reverse=desc)
        elif col == "name":
            filtered.sort(key=lambda a: a.name.lower(), reverse=desc)
        elif col == "status":
            filtered.sort(key=lambda a: a.status.lower(), reverse=desc)
        elif col == "mem":
            filtered.sort(key=lambda a: a.memory_bytes, reverse=desc)
        else: # cpu
            filtered.sort(key=lambda a: a.cpu_pct, reverse=desc)

        arrow = " ▼" if desc else " ▲"
        headers = {
            "pid": "PID",
            "name": "Application Name",
            "status": "Status",
            "cpu": "CPU Time / Load",
            "mem": "Memory (RAM)",
        }
        for c, t in headers.items():
            self.top_apps_tree.heading(c, text=t + (arrow if c == col else ""))

        self.lbl_top_apps_count.configure(text=f"{len(filtered)} processes")
        self.top_apps_tree.delete(*self.top_apps_tree.get_children())
        for app in filtered[:100]:
            cpu_str = f"{app.cpu_pct:.1f}s" if app.cpu_pct > 0 else "0.0s"
            mem_str = format_bytes(app.memory_bytes)
            self.top_apps_tree.insert(
                "",
                tk.END,
                iid=str(app.pid),
                values=(app.pid, app.name, app.status, cpu_str, mem_str),
            )

    def _refresh_sensors_view(self):
        include_top = (self.sensors_subtab == "top_apps" and self.top_apps_visited)
        threading.Thread(target=self._worker_fetch_sensors, args=(include_top,), daemon=True).start()

    def _worker_fetch_sensors(self, include_top_apps: bool):
        snap = self.sensors_collector.collect(include_top_apps=include_top_apps)
        self.after(0, lambda: self._render_sensors_snapshot(snap))

    def _render_sensors_snapshot(self, snap: HardwareSensorsSnapshot):
        # 1. Update Top Header Summary Line (Matches User Screenshot)
        clk_str = f"⏱ {snap.cpu_clock_mhz / 1000.0:.2f} GHz" if snap.cpu_clock_mhz >= 1000 else f"⏱ {snap.cpu_clock_mhz} MHz"
        gpu_short = f"🎮 GPU: {snap.gpu_name[:24]} ({snap.gpu_vram_mb} MB)" if snap.gpu_vram_mb > 0 else f"🎮 GPU: {snap.gpu_name[:24]}"
        ram_str = f"💾 {format_bytes(snap.mem_total_bytes)} Total RAM ({snap.mem_usage_pct:.0f}% Used)"
        disk_str = f"▼ {format_rate(snap.disk_read_rate_bps)} ▲ {format_rate(snap.disk_write_rate_bps)}"

        summary_text = (
            f"{snap.cpu_name} • {snap.cpu_cores_logical} Threads • {clk_str} • "
            f"{gpu_short} • {ram_str} • Live Storage Speed: {disk_str}"
        )
        self.lbl_telemetry_line.configure(text=summary_text)

        # 2. Render System Hardware Cards
        for widget in self.sensors_system_inner.winfo_children():
            widget.destroy()

        # Card A: CPU Processor Monitor
        c_cpu = tk.Frame(self.sensors_system_inner, bg=Palette.BG_CARD, highlightbackground=Palette.BORDER, highlightthickness=1)
        c_cpu.pack(fill=tk.X, padx=6, pady=4)
        tk.Label(c_cpu, text="💻 Central Processing Unit (CPU)", font=("Segoe UI", 11, "bold"), bg=Palette.BG_CARD, fg=Palette.TEXT_PRIMARY).pack(anchor="w", padx=12, pady=(10, 4))
        cpu_details = (
            f"Model: {snap.cpu_name}\n"
            f"Physical Cores: {snap.cpu_cores_physical}   |   Logical Threads: {snap.cpu_cores_logical}   |   Clock Frequency: {clk_str}\n"
            f"Global Utilization Load: {snap.cpu_global_usage:.1f}%"
        )
        tk.Label(c_cpu, text=cpu_details, font=("Consolas", 9), bg=Palette.BG_CARD, fg=Palette.TEXT_SECONDARY, justify=tk.LEFT).pack(anchor="w", padx=12, pady=(0, 10))

        # Card B: Memory (RAM) & Paging File
        c_mem = tk.Frame(self.sensors_system_inner, bg=Palette.BG_CARD, highlightbackground=Palette.BORDER, highlightthickness=1)
        c_mem.pack(fill=tk.X, padx=6, pady=4)
        tk.Label(c_mem, text="💾 System Memory (RAM & Pagefile Commit)", font=("Segoe UI", 11, "bold"), bg=Palette.BG_CARD, fg=Palette.TEXT_PRIMARY).pack(anchor="w", padx=12, pady=(10, 4))
        mem_details = (
            f"Physical RAM:  {format_bytes(snap.mem_used_bytes)} Used / {format_bytes(snap.mem_total_bytes)} Total ({snap.mem_usage_pct:.1f}%)\n"
            f"Pagefile Swap: {format_bytes(snap.page_used_bytes)} Used / {format_bytes(snap.page_total_bytes)} Total ({snap.page_usage_pct:.1f}%)"
        )
        tk.Label(c_mem, text=mem_details, font=("Consolas", 9), bg=Palette.BG_CARD, fg=Palette.TEXT_SECONDARY, justify=tk.LEFT).pack(anchor="w", padx=12, pady=(0, 10))

        # Card C: Storage Volumes & Logical Disks
        c_disk = tk.Frame(self.sensors_system_inner, bg=Palette.BG_CARD, highlightbackground=Palette.BORDER, highlightthickness=1)
        c_disk.pack(fill=tk.X, padx=6, pady=4)
        tk.Label(c_disk, text="📁 Physical & Logical Storage Drives", font=("Segoe UI", 11, "bold"), bg=Palette.BG_CARD, fg=Palette.TEXT_PRIMARY).pack(anchor="w", padx=12, pady=(10, 4))
        disk_lines = [f"Drive {d['drive']} [{d['label']}]  —  {d['free_gb']:.1f} GB Free / {d['total_gb']:.1f} GB Total ({d['usage_pct']:.1f}% Used)" for d in snap.disks]
        disk_text = "\n".join(disk_lines) if disk_lines else "No logical disks detected."
        tk.Label(c_disk, text=disk_text, font=("Consolas", 9), bg=Palette.BG_CARD, fg=Palette.CYAN, justify=tk.LEFT).pack(anchor="w", padx=12, pady=(0, 10))

        # Card D: GPU & Display Controller
        c_gpu = tk.Frame(self.sensors_system_inner, bg=Palette.BG_CARD, highlightbackground=Palette.BORDER, highlightthickness=1)
        c_gpu.pack(fill=tk.X, padx=6, pady=4)
        tk.Label(c_gpu, text="🎮 Graphics Processing Unit (GPU)", font=("Segoe UI", 11, "bold"), bg=Palette.BG_CARD, fg=Palette.TEXT_PRIMARY).pack(anchor="w", padx=12, pady=(10, 4))
        gpu_info = f"Adapter: {snap.gpu_name}\nDedicated VRAM: {snap.gpu_vram_mb} MB   |   Driver Version: {snap.gpu_driver or 'N/A'}"
        tk.Label(c_gpu, text=gpu_info, font=("Consolas", 9), bg=Palette.BG_CARD, fg=Palette.TEXT_SECONDARY, justify=tk.LEFT).pack(anchor="w", padx=12, pady=(0, 10))

        # 3. Update Top Apps Table (if visited and populated)
        if self.sensors_subtab == "top_apps" and snap.top_apps:
            self.last_top_apps = snap.top_apps
            self._render_top_apps_list()

    # ----------------------------------------------------------------------
    # TAB 4: PowerShell Console & Diagnostic Commands Catalog
    # ----------------------------------------------------------------------

    def _build_powershell_tab(self):
        container = tk.Frame(self.tab_powershell, bg=Palette.BG_CANVAS)
        container.pack(fill=tk.BOTH, expand=True, padx=6, pady=2)

        # Left: Interactive PowerShell REPL
        left_repl = tk.Frame(container, bg=Palette.BG_CARD, width=740)
        left_repl.pack(side=tk.LEFT, fill=tk.BOTH, expand=True, padx=(0, 4))

        r_hdr = tk.Frame(left_repl, bg=Palette.BG_SURFACE)
        r_hdr.pack(fill=tk.X, padx=6, pady=(4, 2))

        lbl_ps = tk.Label(r_hdr, text="⚡ Windows PowerShell Interactive REPL Console", font=("Segoe UI", 11, "bold"), bg=Palette.BG_SURFACE, fg=Palette.TEXT_PRIMARY)
        lbl_ps.pack(side=tk.LEFT, padx=6, pady=4)

        self.var_ps_elevated = tk.BooleanVar(value=is_admin())
        chk_elev = tk.Checkbutton(
            r_hdr,
            text="🛡️ Run Elevated (Admin)",
            variable=self.var_ps_elevated,
            font=("Segoe UI", 9, "bold"),
            bg=Palette.BG_SURFACE,
            fg=Palette.CYAN,
            selectcolor=Palette.BG_CARD,
            activebackground=Palette.BG_SURFACE,
            activeforeground=Palette.CYAN,
        )
        chk_elev.pack(side=tk.RIGHT, padx=6, pady=4)

        # Bottom Command Entry (PACKED FIRST SO IT SITS FIRMLY ELEVATED AT BOTTOM!)
        input_bar = tk.Frame(left_repl, bg=Palette.BG_SURFACE)
        input_bar.pack(side=tk.BOTTOM, fill=tk.X, padx=4, pady=(2, 6))

        lbl_prompt = tk.Label(input_bar, text="PS >", font=("Consolas", 11, "bold"), bg=Palette.BG_SURFACE, fg=Palette.ACCENT)
        lbl_prompt.pack(side=tk.LEFT, padx=(8, 4))

        self.entry_ps_cmd = tk.Entry(
            input_bar,
            font=("Consolas", 10),
            bg=Palette.BG_INPUT,
            fg=Palette.TEXT_PRIMARY,
            insertbackground=Palette.TEXT_PRIMARY,
            relief=tk.FLAT,
        )
        self.entry_ps_cmd.pack(side=tk.LEFT, fill=tk.X, expand=True, padx=4, ipady=3)
        self.entry_ps_cmd.bind("<Return>", self._send_ps_command)

        btn_run = tk.Button(
            input_bar,
            text="Execute ↵",
            font=("Segoe UI", 9, "bold"),
            bg=Palette.ACCENT,
            fg=Palette.ACCENT_TEXT,
            relief=tk.FLAT,
            cursor="hand2",
            command=self._send_ps_command,
            padx=10,
            pady=1,
        )
        btn_run.pack(side=tk.RIGHT, padx=6)

        # Middle Console Text Output Well (FILLS REMAINING SPACE)
        ps_middle = tk.Frame(left_repl, bg=Palette.BG_CANVAS)
        ps_middle.pack(side=tk.TOP, fill=tk.BOTH, expand=True, padx=4, pady=2)

        self.ps_text = tk.Text(
            ps_middle,
            bg=Palette.BG_CANVAS,
            fg=Palette.TEXT_PRIMARY,
            font=("Consolas", 10),
            wrap=tk.WORD,
            borderwidth=0,
            padx=10,
            pady=(8, 24), # 24px bottom clearance so bottom lines are never cut off
        )
        ps_scroll = ttk.Scrollbar(ps_middle, orient="vertical", command=self.ps_text.yview)
        self.ps_text.configure(yscrollcommand=ps_scroll.set)

        self.ps_text.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        ps_scroll.pack(side=tk.RIGHT, fill=tk.Y)

        # Right: Preset Windows Administrative Commands Catalog
        right_presets = tk.Frame(container, bg=Palette.BG_CARD, width=420)
        right_presets.pack(side=tk.RIGHT, fill=tk.BOTH, expand=False)
        right_presets.pack_propagate(False)

        pr_hdr = tk.Frame(right_presets, bg=Palette.BG_SURFACE)
        pr_hdr.pack(fill=tk.X, padx=6, pady=6)
        tk.Label(pr_hdr, text="DIAGNOSTIC & ADMIN PRESETS", font=("Segoe UI", 10, "bold"), bg=Palette.BG_SURFACE, fg=Palette.TEXT_MUTED).pack(side=tk.LEFT, padx=6, pady=4)

        # Scrollable presets
        pr_canvas = tk.Canvas(right_presets, bg=Palette.BG_CARD, highlightthickness=0)
        pr_scroll = ttk.Scrollbar(right_presets, orient="vertical", command=pr_canvas.yview)
        pr_inner = tk.Frame(pr_canvas, bg=Palette.BG_CARD)

        pr_inner.bind("<Configure>", lambda e: pr_canvas.configure(scrollregion=pr_canvas.bbox("all")))
        pr_canvas.create_window((0, 0), window=pr_inner, anchor="nw", width=390)
        pr_canvas.configure(yscrollcommand=pr_scroll.set)

        pr_canvas.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        pr_scroll.pack(side=tk.RIGHT, fill=tk.Y)

        self._render_presets_catalog(pr_inner)

    def _render_presets_catalog(self, parent_frame: tk.Frame):
        for preset in self.powershell_presets:
            p_card = tk.Frame(parent_frame, bg=Palette.BG_INPUT, highlightbackground=Palette.BORDER, highlightthickness=1)
            p_card.pack(fill=tk.X, padx=6, pady=4)

            top = tk.Frame(p_card, bg=Palette.BG_INPUT)
            top.pack(fill=tk.X, padx=8, pady=(6, 2))

            tk.Label(top, text=preset["name"], font=("Segoe UI", 9, "bold"), bg=Palette.BG_INPUT, fg=Palette.TEXT_PRIMARY).pack(side=tk.LEFT)

            if preset.get("requires_admin"):
                tk.Label(top, text="Admin 🛡️", font=("Segoe UI", 8), bg=Palette.WARNING_BG, fg=Palette.WARNING_TEXT, padx=4).pack(side=tk.RIGHT)

            tk.Label(p_card, text=preset.get("description", ""), font=("Segoe UI", 8), bg=Palette.BG_INPUT, fg=Palette.TEXT_MUTED, wraplength=360, justify=tk.LEFT).pack(anchor="w", padx=8, pady=(0, 4))

            btn_run_pr = tk.Button(
                p_card,
                text="▶ Run Preset",
                font=("Segoe UI", 8, "bold"),
                bg=Palette.BG_BUTTON,
                fg=Palette.CYAN,
                relief=tk.FLAT,
                cursor="hand2",
                command=lambda cmd=preset["command"], adm=preset.get("requires_admin", False): self._execute_preset_command(cmd, adm),
                padx=8,
                pady=2,
            )
            btn_run_pr.pack(anchor="e", padx=8, pady=(0, 6))

    def _send_ps_command(self, event=None):
        cmd = self.entry_ps_cmd.get().strip()
        if not cmd:
            return
        self.entry_ps_cmd.delete(0, tk.END)

        if self.var_ps_elevated.get() and not is_admin():
            threading.Thread(target=lambda: run_powershell(cmd, elevated=True), daemon=True).start()
            self._on_ps_output(f"\n[Elevated UAC Execution Triggered]: {cmd}\n", "stdout")
        else:
            self.ps_session.send_input(cmd)

    def _execute_preset_command(self, command: str, requires_admin: bool):
        self.ps_text.insert(tk.END, f"\nPS > {command}\n", "stdout")
        self.ps_text.see(tk.END)
        if requires_admin and not is_admin():
            threading.Thread(target=lambda: run_powershell(command, elevated=True), daemon=True).start()
        else:
            self.ps_session.send_input(command)

    def _on_ps_output(self, text: str, stream_type: str):
        self.after(0, lambda: self._insert_ps_output(text, stream_type))

    def _insert_ps_output(self, text: str, stream_type: str):
        clean = strip_ansi(text)
        self.ps_text.insert(tk.END, clean)
        self.ps_text.see(tk.END)

    # ----------------------------------------------------------------------
    # TAB 5: Listening Ports Inspector
    # ----------------------------------------------------------------------

    def _build_ports_tab(self):
        container = tk.Frame(self.tab_ports, bg=Palette.BG_CANVAS)
        container.pack(fill=tk.BOTH, expand=True, padx=8, pady=4)

        top_bar = tk.Frame(container, bg=Palette.BG_CARD)
        top_bar.pack(fill=tk.X, pady=(0, 6))

        r_top = tk.Frame(top_bar, bg=Palette.BG_CARD)
        r_top.pack(fill=tk.X, padx=12, pady=10)

        lbl_title = tk.Label(r_top, text="Active TCP Listening Sockets & Port Conflict Inspector", font=("Segoe UI", 12, "bold"), bg=Palette.BG_CARD, fg=Palette.TEXT_PRIMARY)
        lbl_title.pack(side=tk.LEFT)

        self.lbl_ports_count = tk.Label(r_top, text="Scanning ports...", font=("Segoe UI", 9), bg=Palette.BG_BUTTON, fg=Palette.TEXT_SECONDARY, padx=8, pady=2)
        self.lbl_ports_count.pack(side=tk.LEFT, padx=12)

        btn_ref = tk.Button(r_top, text="🔄 Rescan Ports", font=("Segoe UI", 9, "bold"), bg=Palette.ACCENT, fg=Palette.ACCENT_TEXT, relief=tk.FLAT, command=self._refresh_ports_view, padx=10)
        btn_ref.pack(side=tk.RIGHT)

        tree_frame = tk.Frame(container, bg=Palette.BG_CARD)
        tree_frame.pack(fill=tk.BOTH, expand=True)

        cols = ("port", "address", "pid", "process")
        self.ports_tree = ttk.Treeview(tree_frame, columns=cols, show="headings", selectmode="browse")

        self.ports_tree.heading("port", text="Listening Port")
        self.ports_tree.heading("address", text="Local Bound Address")
        self.ports_tree.heading("pid", text="Process ID (PID)")
        self.ports_tree.heading("process", text="Owning Process Name")

        self.ports_tree.column("port", width=120, anchor="center")
        self.ports_tree.column("address", width=220, anchor="w")
        self.ports_tree.column("pid", width=120, anchor="center")
        self.ports_tree.column("process", width=340, anchor="w")

        p_scroll = ttk.Scrollbar(tree_frame, orient="vertical", command=self.ports_tree.yview)
        self.ports_tree.configure(yscrollcommand=p_scroll.set)

        self.ports_tree.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        p_scroll.pack(side=tk.RIGHT, fill=tk.Y)

        footer = tk.Frame(container, bg=Palette.BG_SURFACE)
        footer.pack(fill=tk.X, pady=(6, 0))

        btn_kill = tk.Button(footer, text="✕ Force Kill Process on Port", font=("Segoe UI", 9, "bold"), bg=Palette.DANGER_BG, fg=Palette.DANGER_TEXT, relief=tk.FLAT, command=self._kill_selected_port_process, padx=12, pady=4)
        btn_kill.pack(side=tk.LEFT, padx=8, pady=6)

    def _refresh_ports_view(self):
        self.lbl_ports_count.configure(text="Scanning active TCP listeners...")
        threading.Thread(target=self._worker_fetch_ports, daemon=True).start()

    def _worker_fetch_ports(self):
        ports = []
        if IS_WINDOWS:
            ps_ports = (
                "Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | "
                "Select-Object LocalAddress, LocalPort, OwningProcess, @{Name='Process';Expression={(Get-Process -Id $_.OwningProcess -ErrorAction SilentlyContinue).ProcessName}} | "
                "Sort-Object LocalPort | ConvertTo-Json -Compress"
            )
            rc, out, _ = run_powershell(ps_ports, timeout_sec=5.0)
            if rc == 0 and out:
                try:
                    data = json.loads(out)
                    items = data if isinstance(data, list) else [data]
                    for item in items:
                        ports.append((
                            int(item.get("LocalPort") or 0),
                            str(item.get("LocalAddress") or "0.0.0.0"),
                            int(item.get("OwningProcess") or 0),
                            str(item.get("Process") or "Unknown"),
                        ))
                except Exception:
                    pass
        else:
            ports = [
                (3000, "127.0.0.1", 1184, "node"),
                (8000, "0.0.0.0", 2408, "python3"),
                (5432, "127.0.0.1", 890, "postgres"),
                (80, "0.0.0.0", 4, "System"),
            ]

        self.after(0, lambda: self._populate_ports_tree(ports))

    def _populate_ports_tree(self, ports: List[Tuple[int, str, int, str]]):
        self.ports_tree.delete(*self.ports_tree.get_children())
        self.lbl_ports_count.configure(text=f"{len(ports)} Active TCP Listeners")
        for port, addr, pid, proc in ports:
            self.ports_tree.insert("", tk.END, iid=f"{port}_{pid}", values=(f":{port}", addr, pid, proc))

    def _kill_selected_port_process(self):
        sel = self.ports_tree.selection()
        if not sel:
            messagebox.showinfo("Selection Required", "Please select a port row to kill.")
            return

        vals = self.ports_tree.item(sel[0], "values")
        pid = vals[2]
        proc = vals[3]

        if messagebox.askyesno("Confirm Taskkill", f"Force kill process '{proc}' (PID: {pid})?"):
            if IS_WINDOWS:
                subprocess.run(f"taskkill /F /PID {pid}", shell=True, creationflags=CREATE_NO_WINDOW)
            else:
                subprocess.run(f"kill -9 {pid}", shell=True)
            self._refresh_ports_view()

    # ----------------------------------------------------------------------
    # Bottom Status Bar
    # ----------------------------------------------------------------------

    def _build_bottom_status_bar(self):
        bar = tk.Frame(self, bg=Palette.BG_SURFACE, height=28)
        bar.pack(side=tk.BOTTOM, fill=tk.X)
        bar.pack_propagate(False)

        lbl_info = tk.Label(
            bar,
            text=f"Server Launcher Windows v1.0.0 • Python {sys.version.split()[0]} • Platform: {sys.platform}",
            font=("Segoe UI", 9),
            bg=Palette.BG_SURFACE,
            fg=Palette.TEXT_MUTED,
            padx=12,
        )
        lbl_info.pack(side=tk.LEFT)

        self.lbl_status_right = tk.Label(
            bar,
            text="Ready",
            font=("Segoe UI", 9),
            bg=Palette.BG_SURFACE,
            fg=Palette.SUCCESS,
            padx=12,
        )
        self.lbl_status_right.pack(side=tk.RIGHT)

    # ----------------------------------------------------------------------
    # Background Pollers & Configuration Loaders
    # ----------------------------------------------------------------------

    def _start_background_pollers(self):
        # Hardware Telemetry Poller (Every 2 seconds)
        def poll_sensors():
            while True:
                time.sleep(2.0)
                try:
                    if self.active_tab_name == "sensors":
                        include_top = (self.sensors_subtab == "top_apps" and self.top_apps_visited)
                        snap = self.sensors_collector.collect(include_top_apps=include_top)
                        self.after(0, lambda s=snap: self._render_sensors_snapshot(s))
                except Exception:
                    pass

        threading.Thread(target=poll_sensors, daemon=True).start()

    def _load_servers_config(self):
        if not CONFIG_PATH.exists():
            return
        try:
            with open(CONFIG_PATH, "r", encoding="utf-8") as f:
                data = json.load(f)
                for item in data:
                    cfg = ServerConfig(
                        name=item.get("name", "Unnamed"),
                        dir=item.get("dir", "."),
                        cmd=item.get("cmd", ""),
                        stop_cmd=item.get("stop_cmd", ""),
                        port=item.get("port", 0),
                        group=item.get("group", "General"),
                        env=item.get("env", {}),
                        actions=[ServerAction(a["name"], a["cmd"]) for a in item.get("actions", [])],
                        links=[ServerLink(l["name"], l["url"]) for l in item.get("links", [])],
                        autostart=item.get("autostart", False),
                        auto_restart=item.get("auto_restart", False),
                        own_console=item.get("own_console", False),
                    )
                    srv = ManagedServer(cfg, self._on_server_log, self._on_server_status_change)
                    self.servers.append(srv)
                    if cfg.autostart:
                        srv.start()
        except Exception as e:
            print(f"Error loading {CONFIG_PATH}: {e}")

    def _load_powershell_presets(self):
        self.powershell_presets = []
        if PRESETS_PATH.exists():
            try:
                with open(PRESETS_PATH, "r", encoding="utf-8") as f:
                    self.powershell_presets = json.load(f)
            except Exception:
                pass

    def _on_server_log(self, name: str, line: str):
        if self.active_server_name == name:
            self.after(0, lambda: self._append_server_log(line))

    def _append_server_log(self, line: str):
        self.log_text.insert(tk.END, line)
        self.log_text.see(tk.END)

    def _on_server_status_change(self):
        self.after(0, self._render_server_cards)

    def _show_add_server_dialog(self):
        win = tk.Toplevel(self)
        win.title("Add New Dev Server")
        win.geometry("480x380")
        win.configure(bg=Palette.BG_CARD)

        pad = 10
        tk.Label(win, text="Configure Dev Server Instance", font=("Segoe UI", 11, "bold"), bg=Palette.BG_CARD, fg=Palette.TEXT_PRIMARY).pack(anchor="w", padx=pad, pady=pad)

        form = tk.Frame(win, bg=Palette.BG_CARD)
        form.pack(fill=tk.X, padx=pad)

        tk.Label(form, text="Server Name:", bg=Palette.BG_CARD, fg=Palette.TEXT_SECONDARY).grid(row=0, column=0, sticky="w", pady=4)
        ent_name = tk.Entry(form, bg=Palette.BG_INPUT, fg=Palette.TEXT_PRIMARY, insertbackground=Palette.TEXT_PRIMARY, width=32)
        ent_name.grid(row=0, column=1, pady=4)

        tk.Label(form, text="Working Directory:", bg=Palette.BG_CARD, fg=Palette.TEXT_SECONDARY).grid(row=1, column=0, sticky="w", pady=4)
        ent_dir = tk.Entry(form, bg=Palette.BG_INPUT, fg=Palette.TEXT_PRIMARY, insertbackground=Palette.TEXT_PRIMARY, width=32)
        ent_dir.insert(0, ".")
        ent_dir.grid(row=1, column=1, pady=4)

        tk.Label(form, text="Start Command:", bg=Palette.BG_CARD, fg=Palette.TEXT_SECONDARY).grid(row=2, column=0, sticky="w", pady=4)
        ent_cmd = tk.Entry(form, bg=Palette.BG_INPUT, fg=Palette.TEXT_PRIMARY, insertbackground=Palette.TEXT_PRIMARY, width=32)
        ent_cmd.grid(row=2, column=1, pady=4)

        tk.Label(form, text="Port Number:", bg=Palette.BG_CARD, fg=Palette.TEXT_SECONDARY).grid(row=3, column=0, sticky="w", pady=4)
        ent_port = tk.Entry(form, bg=Palette.BG_INPUT, fg=Palette.TEXT_PRIMARY, insertbackground=Palette.TEXT_PRIMARY, width=12)
        ent_port.insert(0, "3000")
        ent_port.grid(row=3, column=1, sticky="w", pady=4)

        def do_save():
            name = ent_name.get().strip()
            cmd = ent_cmd.get().strip()
            p_str = ent_port.get().strip()
            port = int(p_str) if p_str.isdigit() else 0

            if not name or not cmd:
                messagebox.showerror("Error", "Name and Command are required.")
                return

            cfg = ServerConfig(name=name, dir=ent_dir.get().strip(), cmd=cmd, port=port)
            srv = ManagedServer(cfg, self._on_server_log, self._on_server_status_change)
            self.servers.append(srv)
            self._save_servers_config()
            self._render_server_cards()
            win.destroy()

        btn_box = tk.Frame(win, bg=Palette.BG_CARD)
        btn_box.pack(fill=tk.X, padx=pad, pady=pad)
        tk.Button(btn_box, text="Save Server", font=("Segoe UI", 9, "bold"), bg=Palette.ACCENT, fg=Palette.ACCENT_TEXT, command=do_save, padx=12, pady=4).pack(side=tk.RIGHT)
        tk.Button(btn_box, text="Cancel", font=("Segoe UI", 9), bg=Palette.BG_BUTTON, fg=Palette.TEXT_PRIMARY, command=win.destroy, padx=10, pady=4).pack(side=tk.RIGHT, padx=6)

    def _save_servers_config(self):
        data = []
        for s in self.servers:
            data.append({
                "name": s.config.name,
                "dir": s.config.dir,
                "cmd": s.config.cmd,
                "stop_cmd": s.config.stop_cmd,
                "port": s.config.port,
                "group": s.config.group,
                "env": s.config.env,
                "autostart": s.config.autostart,
                "auto_restart": s.config.auto_restart,
                "own_console": s.config.own_console,
            })
        try:
            with open(CONFIG_PATH, "w", encoding="utf-8") as f:
                json.dump(data, f, indent=2)
        except Exception as e:
            print(f"Error saving {CONFIG_PATH}: {e}")


# --------------------------------------------------------------------------
# Main Entry Point
# --------------------------------------------------------------------------

def main():
    app = ServerLauncherWindowsApp()
    app.mainloop()


if __name__ == "__main__":
    main()

