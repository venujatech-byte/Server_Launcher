---
name: nocturnal-command
description: >-
  Design system and implementation guide for Nocturnal Command: a Material 3 Dark / Developer Dashboard
  aesthetic tailored for server orchestrators, developer utilities, terminal cockpits, and high-density technical tools.
  Use whenever designing, styling, or refactoring UI components to have modern nocturnal obsidian surfaces, electric violet
  and cybernetic cyan accents, pill-shaped buttons, status chip pills, and recessed monospace code wells.
---

# Nocturnal Command Design System

Nocturnal Command blends **Material 3 (Material You)** architectural semantics with the functional rigor of modern software engineering. It is crafted for developer utilities, server orchestrators, and local environment managers where technical density must coexist with immediate, ergonomic control.

## 1. Palette & Surface Tokens

### Surface Luminance Tiers (Tonal Depth over Drop Shadows)
Elevation is expressed through luminance stepping on dark obsidian surfaces rather than blurry drop shadows:

- **Canvas & Recessed Terminal (`surface-container-lowest` / `background`)**: `#0a0a0f` / `#13131a`
- **Group Plates & Inactive Segments (`surface-container-low`)**: `#191921` / `#1b1b22`
- **Base Cards & Modules (`surface-container`)**: `#1d1d27` / `#1f1f26`
- **Elevated Cards, Modals & Search (`surface-container-high`)**: `#282833` / `#2a2931`
- **Floating Toolbars & App Bars (`surface-container-highest`)**: `#33333f` / `#34343c`
- **Outline & Hairlines (`outline-variant`)**: `#454552` / `#47464f`
- **Form Strokes & Inactive Toggles (`outline`)**: `#8f8f9d` / `#928f9a`

### Accent & Telemetry Colors
- **Primary (`#c4c0ff`)**: Electric Periwinkle / Violet. High-emphasis focal actions, launch buttons, active navigation markers.
  - `on-primary`: `#2d2a5e` / `#2b2277`
  - `primary-container`: `#423b8f`
  - `on-primary-container`: `#e3dfff`
- **Secondary (`#70d5e4` / `#72d7e6`)**: Telemetry Cyan. Listening ports, network badges, sockets, restart action buttons.
  - `on-secondary`: `#00363e`
  - `secondary-container`: `#004e5a` / `#2f9eac`
  - `on-secondary-container`: `#a3f0fc`
- **Tertiary / Success (`#97f6ca` / `#7ddbb1` / `#4ade80`)**: Mint runtime status, uptime badges, healthy health checks.
  - `tertiary-container`: `#005237` / `#0f3820`
  - `on-tertiary-container`: `#86efac` / `#9af8cc`
- **Danger / Error (`#ffb4ab` / `#b91c1c`)**: Stop buttons, crashed states, error badges.
  - `error-container`: `#93000a`
  - `on-error-container`: `#ffdad6`
- **Warning / Starting (`#fcd34d` / `#f59e0b`)**: Starting, building, compiling states.
  - `warning-container`: `#451a03`
  - `on-warning-container`: `#fef08a`

## 2. Shapes & Geometry

- **Pill Primitives (`rounded-full` / `Rounding::same(999.0)` or `14-16px`)**:
  - Action buttons (`Start All`, `Stop All`, `+ Add Server`, `📥 Import`, `⚡ Quick Launch`).
  - Search bar.
  - Status chip pills (`● Running`, `○ Stopped`).
  - Metric telemetry pills (`PID: 11864`, `⚡ 0.0% CPU`, `💾 182.2 MB RAM`, `⏱ 35s uptime`).
- **Macro Cards**: `16px - 24px` corner radius (`Rounding::same(16.0)`).
- **Command Well (Terminal Code Box)**: `8px` corner radius (`Rounding::same(8.0)`), recessed dark box `#0a0a0f` with subtle 1px border `#2a2936`.

## 3. Component Patterns

### Top App Bar & Search
- Full-width dark surface `#13131a`.
- Pill search bar: `🔍 Search instances, ports, PIDs...` with `Ctrl+K` hint badge.
- Action pills row:
  - `⚡ Quick Launch (Ctrl+P)` (tonal pill)
  - `▶ Start All` (mint/green pill)
  - `⏹ Stop All` (muted red/pill)
  - `📥 Import` (tonal pill)
  - `+ Add Server` (electric violet primary pill)

### M3 Status Chip Pills
- **Running**: Background `#0f3820`, text `#86efac`, leading green dot `●` (`#4ade80`).
- **Stopped**: Background `#1e293b`, text `#cbd5e1`, leading slate dot `○` (`#94a3b8`).
- **Starting / Restarting**: Background `#451a03`, text `#fef08a`, leading amber dot `⟳` (`#fcd34d`).
- **Error / Crashed**: Background `#93000a`, text `#ffdad6`, leading red dot `✕` (`#ffb4ab`).

### Server Instance Cards
- Container: `#1d1d27` with 1px border `#454552` (or `#c4c0ff` when selected).
- Header row: Glowing status dot + Server name in bold + Group tag pill + Port badge + Status chip pill + quick action buttons (`Stop` red pill, `Restart` cyan pill, external launch icon).
- Code well: Monospace `$ <command>` in `#e4e1eb` with right-aligned metadata tags.
- Telemetry row: Clean rounded pill badges for PID, CPU %, RAM MB, and Uptime.
- Bottom toggles: Autostart at boot, auto-restart on fail, and quick tool shortcuts.

