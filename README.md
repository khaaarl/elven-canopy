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

### 4. C++ build dependencies (for the LLM crate)

The `elven_canopy_llm` crate wraps llama.cpp, which is compiled from C++ source during the Rust build. This requires CMake, a C++ compiler, and LLVM/Clang (for bindgen FFI generation).

**Linux (Debian/Ubuntu):**

```bash
sudo apt install cmake libclang-dev
```

A C++ compiler (`g++`) is typically already installed. If not: `sudo apt install build-essential`.

**macOS:**

CMake and Clang are provided by Xcode Command Line Tools, which you likely already have if you've installed Rust or Homebrew. If not:

```bash
xcode-select --install
```

If CMake is missing after that: `brew install cmake`.

**Windows:**

Three things are needed:

1. **Visual Studio Build Tools 2022** (provides the MSVC C++ compiler and linker):
   ```
   winget install Microsoft.VisualStudio.2022.BuildTools
   ```
   When the Visual Studio Installer opens, select the **"Desktop development with C++"** workload. Make sure **"MSVC v143 - VS 2022 C++ x64/x86 build tools"** and a **Windows SDK** are checked within it.

2. **CMake** (usually included with the Build Tools if you selected the C++ workload above). Verify with `cmake --version`. If missing: `winget install Kitware.CMake`.

3. **LLVM** (provides libclang for bindgen):
   ```
   winget install LLVM.LLVM
   ```
   After installation, set the `LIBCLANG_PATH` environment variable to the LLVM bin directory (typically `C:\Program Files\LLVM\bin`).

**Important:** On Windows, run builds from the **"x64 Native Tools Command Prompt for VS 2022"** (search Start menu) so that the MSVC compiler and linker are on PATH. Building from a regular terminal or PowerShell will fail with `link.exe not found`.

> **Note:** If you previously had Visual Studio 2017 installed, uninstall it — CMake may find the old toolchain first, which lacks C++17 support required by llama.cpp.

The first build compiles llama.cpp from source, which takes several minutes. Subsequent builds are cached.

### 5. Clone and build

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

The first build compiles the Godot GDExtension bindings and llama.cpp from C++ source, which takes several minutes. Subsequent builds are incremental and much faster.

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

### Opening in the Godot editor

After building at least once (so the GDExtension library exists), open the Godot editor and import the `godot/` directory as a project. The build script creates a `godot/target` symlink pointing to the Cargo output directory — this is how Godot finds the compiled library.
