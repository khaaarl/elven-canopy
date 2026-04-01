# Elven Canopy

[![CI](https://github.com/khaaarl/elven-canopy/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/khaaarl/elven-canopy/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/khaaarl/elven-canopy/branch/main/graph/badge.svg)](https://codecov.io/gh/khaaarl/elven-canopy)

A Dwarf Fortress-inspired simulation game set in a forest of enormous trees. You are a tree spirit who forms a symbiotic relationship with a village of elves — they sing to you and you grow platforms, walkways, and structures from your trunk and branches. Happy elves generate mana, mana fuels growth, and growth keeps elves happy.

## Playing the game

Download the latest build for your platform from the [Releases page](https://github.com/khaaarl/elven-canopy/releases). Builds are available for Windows, Linux, macOS, and Android.

> **Note:** These builds are not code-signed or notarized. Your OS may show a warning when you try to run them.
> - **Windows:** "Windows protected your PC" — click *More info* then *Run anyway*.
> - **macOS:** "can't be opened because it is from an unidentified developer" — right-click the app, choose *Open*, then click *Open* in the dialog. Alternatively, run `xattr -cr "Elven Canopy.app"` in Terminal.
> - **Linux:** You may need to `chmod +x` the binary after extracting.

## Development setup

The game uses **Godot 4.6** for rendering and UI, with all simulation logic written in **Rust** and bridged via GDExtension. You'll need three things installed: Python 3, Rust, and Godot.

### 1. Python 3

The build script (`scripts/build.py`) requires Python 3.8+ with no external dependencies.

| Platform | Install |
|----------|---------|
| **Linux** | Usually pre-installed. Check with `python3 --version`. If missing: `sudo apt install python3` (Debian/Ubuntu) or equivalent. |
| **macOS** | Usually pre-installed on recent versions. Otherwise: `brew install python3` |
| **Windows** | Download from [python.org](https://www.python.org/downloads/) or install via `winget install Python.Python.3`. Make sure "Add to PATH" is checked during install. |

### 2. Rust

Install via [rustup](https://rustup.rs/):

```bash
# Linux / macOS
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Windows — download and run rustup-init.exe from https://rustup.rs/
```

The project uses Rust edition 2024, which requires Rust 1.85+. After installing, verify with `rustc --version`.

### 3. Godot 4.6

Download **Godot 4.6** (standard build, not .NET) from [godotengine.org](https://godotengine.org/download/).

The Godot binary needs to be on your `PATH` so the build script can find it. It looks for `godot-4`, `godot4`, or `godot` in that order.

| Platform | Typical setup |
|----------|---------------|
| **Linux** | Package managers often install as `godot-4`. Snap: `sudo snap install godot-4`. Or extract the downloaded binary somewhere on your PATH. |
| **macOS** | `brew install --cask godot` puts it on your PATH. Or extract the app and symlink the binary. |
| **Windows** | The download is a standalone `.exe` with a long name like `Godot_v4.6.1-stable_win64.exe`. Create a directory (e.g., `C:\Tools\Godot`), copy the exe there, rename it to `godot.exe`, and add that directory to your PATH. Alternatively, `scoop install godot` handles all of this automatically. |

### 4. Clone and build

```bash
git clone https://github.com/khaaarl/elven-canopy.git
cd elven-canopy

# Build the Rust GDExtension library (debug mode)
python3 scripts/build.py

# Build and launch the game
python3 scripts/build.py run
```

On Windows, use `python` or `py` instead of `python3` if that's how your Python is installed:

```
py scripts\build.py run
```

The first build compiles the Godot GDExtension bindings, which takes a few minutes. Subsequent builds are incremental and much faster.

### Build commands

| Command | Description |
|---------|-------------|
| `python3 scripts/build.py` | Debug build |
| `python3 scripts/build.py release` | Release (optimized) build |
| `python3 scripts/build.py run` | Debug build + launch |
| `python3 scripts/build.py run-branch NAME` | Fetch, checkout branch, sync, build + launch |
| `python3 scripts/build.py test` | Run all Rust and GDScript tests |
| `python3 scripts/build.py quicktest` | Test only crates changed vs main |
| `python3 scripts/build.py gdtest` | Run GDScript unit tests only |
| `python3 scripts/build.py check` | Run formatters, linters, and checks |
| `python3 scripts/build.py relay` | Optimized standalone relay server binary |
| `python3 scripts/build.py coverage` | Generate HTML code coverage report |

### Claude Code sandbox setup (Ubuntu)

This repo's `.claude/settings.json` enables [Claude Code's sandbox mode](https://docs.anthropic.com/en/docs/claude-code/security#sandbox), which runs all shell commands inside a bubblewrap container with restricted filesystem access. To use it on Ubuntu:

1. **Install bubblewrap and socat:**

   ```bash
   sudo apt install bubblewrap socat
   ```

2. **Install the sandbox runtime:**

   ```bash
   sudo npm install -g @anthropic-ai/sandbox-runtime
   ```

3. **Configure AppArmor** (Ubuntu 23.10+):

   Ubuntu restricts unprivileged user namespaces by default, which bubblewrap needs. Create an AppArmor profile to allow them:

   ```bash
   sudo tee /etc/apparmor.d/bwrap << 'EOF'
   abi <abi/4.0>,
   include <tunables/global>

   profile bwrap /usr/bin/bwrap flags=(unconfined) {
     userns,
     include if exists <local/bwrap>
   }
   EOF

   sudo systemctl reload apparmor
   ```

   > **Note:** This step may not be required on future Ubuntu versions if the default AppArmor policy changes.

The sandbox configuration lives in `.claude/settings.json` under the `"sandbox"` key. The checked-in config restricts filesystem writes to the repo and temp directories, and allows network access to all hosts (for GitHub operations and web research). See the [Claude Code sandbox docs](https://docs.anthropic.com/en/docs/claude-code/security#sandbox) for all available options.

### Opening in the Godot editor

After building at least once (so the GDExtension library exists), open the Godot editor and import the `godot/` directory as a project. The build script creates a `godot/target` symlink pointing to the Cargo output directory — this is how Godot finds the compiled library.
