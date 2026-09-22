# Codenotch for Linux (Debian 13+)

A serious, maintainable Linux port of Codenotch, targeting Debian 13 (trixie) as the first-class reference platform while adhering to cross-distribution standards (XDG Base Directory specification, freedesktop desktop entries, AppIndicator, and Wayland/X11 compatibility).

---

## 1. System Requirements

### Reference Platform
- **OS**: Debian GNU/Linux 13 (trixie) x86_64
- **Kernel**: 6.1+ (supports `/proc/[pid]/io` counters and standard namespaces)
- **Display**: Wayland (GNOME, KDE Plasma, Sway) or X11

### Build Dependencies
Install the required development libraries:

```bash
sudo apt update && sudo apt install -y \
  build-essential \
  curl \
  pkg-config \
  libwebkit2gtk-4.1-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  libssl-dev \
  libxdo-dev
```

### Rust Toolchain
Rust 1.80+ is required. If not installed via Debian packages (`rustc`, `cargo`), install via rustup:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
```

---

## 2. Building and Running

The root `Makefile` provides convenient Linux targets:

### Compile the Claude Code Hook
The hook binary is detached, zero-dependency, and reports CLI lifecycle events to Codenotch:

```bash
make linux-hook
# Binary output: windows/target/hook/release/codenotch-hook
```

### Build the Main Application
```bash
make linux-build
```

### Run Diagnostics (`doctor`)
Inspect the local Linux environment, active display server (Wayland vs X11), installed CLI tools, and configuration directories without exposing secrets:

```bash
make linux-doctor
# Or directly:
cargo run --manifest-path windows/Cargo.toml -p codenotch -- doctor
```

### Run in Demo Mode
Test the notch interface with realistic mock data across Claude, Codex, Cursor, Grok, and Antigravity:

```bash
CODENOTCH_DEMO=1 make linux-run
```

### Run Tests
Execute the Rust unit tests and UI script validation:

```bash
make linux-test
```

### Package `.deb` and `.AppImage`
Build native Linux distribution bundles:

```bash
make linux-package
# Package output:
#   windows/target/release/bundle/deb/*.deb
#   windows/target/release/bundle/appimage/*.AppImage
```

---

## 3. Architecture & Linux Integration

- **Process & I/O Tracking**: Linux process inspection uses native `/proc/[pid]/cmdline`, `/proc/[pid]/stat`, and `/proc/[pid]/io` readers, bypassing Windows-specific Win32 APIs.
- **CLI Discovery**: Scans standard Linux paths (`/usr/local/bin`, `/usr/bin`, `~/.local/bin`, `~/bin`), version managers (NVM, FNM, Volta, ASDF), and package managers (`pnpm`, `npm` global prefix) for `claude`, `codex`, and `agy`.
- **System Tray**: Powered by `libayatana-appindicator3` via Tauri, integrating natively with GNOME Shell (via AppIndicator extension), KDE Plasma, and XFCE.
- **Execution Management & IDE Watcher**: Provides manual control (`codenotch-control`) and automatic IDE presence monitoring (`systemd --user` service `codenotch-ide-watch.service`) that automatically launches Codenotch when configured IDEs (Cursor, Antigravity, VS Code) are open and cleanly stops Codenotch when all IDEs close.
- **Autostart**: Deprecates unconditional login autostart (`~/.config/autostart/codenotch.desktop`). Only the lightweight `codenotch-ide-watch.service` user service starts at login when auto-mode is enabled (`codenotch-control auto-on`).
- **Credential Safety**: Strictly preserves read-only access to existing local credentials (`~/.claude/`, `~/.codex/`, `~/.grok/`, Cursor DB). No tokens or credentials are logged or transmitted externally.

---

## 4. Execution Management (`codenotch-control`)

Control Codenotch manually or toggle automatic IDE monitoring:

```bash
# Manual control
codenotch-control start     # Start Codenotch if not running
codenotch-control stop      # Stop Codenotch (keeps IDEs untouched; pauses auto-watcher for active session)
codenotch-control restart   # Restart Codenotch
codenotch-control status    # View running status, PID, and detected IDEs
codenotch-control toggle    # Toggle between running and stopped

# Automatic mode (systemd --user IDE watcher)
codenotch-control auto-on     # Enable and start monitoring on login
codenotch-control auto-off    # Disable automatic monitoring completely
codenotch-control auto-status # View systemd user service status

# Pause / Resume
codenotch-control pause     # Pause IDE monitoring indefinitely
codenotch-control resume    # Resume IDE monitoring
```
