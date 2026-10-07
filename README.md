### Prerequisites
- Python 3.10+
- Linux users must install Tkinter system binaries:
  `sudo apt install python3-tk`

# Server Launcher

A small Tkinter-based control panel for your dev servers. Add your own
servers (path + start command), start and stop them individually or all
together, watch live logs, and see every process currently listening on
your PC.

**No pip installs required** - standard library only. Works on Windows.

## Features

- **Add / Edit / Delete servers** from the UI. Each server has:
  - a **name** and **working directory**
  - a **start command** (e.g. `npm run dev`, `python app.py`, `node server.js`)
  - an optional **stop command** that runs *before* stopping
    (e.g. `firebase emulators:export ./data`)
  - an optional **port** used to detect when it's really running
  - **environment variables** (one `KEY=VALUE` per line)
  - **action buttons** (extra sub-commands that run in that folder)
  - **open-in-browser links**
- **Start / Stop** per server, plus **Start all / Stop all**.
- **Live logs** and status lights for each server.
- **Server Overview tab** - lists every process listening on the PC
  (via `netstat`), with a per-row **Stop** button for servers the launcher
  didn't start.
- **Scrollable** card list so many servers stay manageable.

## Getting started

1. Run the app:

   ```
   python server_launcher.py
   ```

2. Click **+ Add Server**.
3. Fill in a name, pick the working directory, and type a start command.
4. (Optional) add a stop command, a port, env vars, action buttons, or links.
5. Click **Save**, then press **Start** on the new card.

## Commands

Command names can be typed normally - no need to add `.cmd`/`.exe` file
extensions. The launcher resolves them and runs them without an extra shell.

```
npm run dev
python app.py
node server.js
firebase emulators:start
```

### Virtual environment Python

If a server's start command uses `python` and the working directory contains
a virtual environment, the venv's Python is used automatically. The launcher
looks for, in order:

- `.venv\Scripts\python.exe`
- `venv\Scripts\python.exe`
- `env\Scripts\python.exe`
- `.virtualenv\Scripts\python.exe`

If none is found, the system Python is used.

### Quoted arguments

Commands with arguments that contain spaces work too - wrap them in double
quotes and they are passed through correctly, e.g.:

```
python -c "from pathlib import Path; print('hi')"
```

## Stop behavior

- If a **stop command** is set, it runs first (in the server's working
  directory) and then the server's process tree is terminated.
- If no stop command is set, the server is killed directly.
- This is handy for services that need to export/clean up before shutting
  down, e.g. a Firestore emulator exporting its data on stop.

## Status detection

A server shows:

| Status | Meaning |
| ------ | ------- |
| `Starting` | launching... |
| `Running` | process is alive (and its port is open, if one is set) |
| `Stopping` | stop command / kill in progress |
| `Stopped` / `Exited` | not running |

If a port is configured, the server is marked `Running` once the port is
open (or after a short grace period if the process stays alive). If no port
is set, it is marked `Running` as soon as the process is alive.

## Data

Servers you add are saved to `servers.json`, created next to
`server_launcher.py` on your first save. Edit or delete them any time from
their card. Entries look like:

```json
{
  "servers": [
    {
      "key": "...",
      "name": "My Server",
      "cwd": "C:\\path\\to\\project",
      "command": "npm run dev",
      "stop_command": "",
      "port": 3000,
      "env": {},
      "actions": [],
      "links": []
    }
  ]
}
```

## Notes

- Developed for **Windows** (`netstat`, `taskkill`/`tasklist`).
- Standard library only - no third-party dependencies.
