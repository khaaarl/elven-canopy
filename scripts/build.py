#!/usr/bin/env python3
"""Cross-platform build script for Elven Canopy.

Replaces the former build.sh with a stdlib-only Python script that works on
Linux, macOS, and Windows.  Same subcommands, same behavior.

Usage:
    python3 scripts/build.py            # debug build
    python3 scripts/build.py release    # release build
    python3 scripts/build.py test       # run all crate tests
    python3 scripts/build.py quicktest  # test only crates changed vs main
    python3 scripts/build.py gdtest     # run GDScript unit tests (GUT)
    python3 scripts/build.py run        # debug build then launch the game
    python3 scripts/build.py run-branch NAME  # fetch, checkout branch, sync, build+run
    python3 scripts/build.py relay      # optimized standalone relay binary (LTO, stripped)
    python3 scripts/build.py check      # run fmt, clippy, gdformat, gdlint checks
    python3 scripts/build.py coverage   # generate HTML code coverage report

Run from the repo root.
"""

from __future__ import annotations

import argparse
import glob
import os
import platform
import shutil
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
IS_WINDOWS = platform.system() == "Windows"

# Crate directories used by `quicktest` to detect changes and by `check` for
# scoped clippy.  In quicktest, multiplayer_tests is always appended separately
# (not detected by directory changes).  In check/clippy, all are included.
ALL_CRATES = [
    "elven_canopy_prng",
    "elven_canopy_lang",
    "elven_canopy_sim",
    "elven_canopy_sprites",
    "elven_canopy_utils",
    "elven_canopy_protocol",
    "elven_canopy_relay",
    "elven_canopy_music",
    "elven_canopy_gdext",
    "tabulosity",
    "tabulosity_derive",
    "multiplayer_tests",
]

# Crates included in the `test` target (excludes gdext, tabulosity handled
# separately to avoid Cargo feature unification).
TEST_CRATES = [
    "elven_canopy_prng",
    "elven_canopy_lang",
    "elven_canopy_sim",
    "elven_canopy_sprites",
    "elven_canopy_utils",
    "elven_canopy_protocol",
    "elven_canopy_relay",
    "elven_canopy_music",
    "multiplayer_tests",
]


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def run(
    *cmd: str,
    check: bool = True,
    capture: bool = False,
    timeout: int | None = None,
    env: dict[str, str] | None = None,
    **kwargs,
) -> subprocess.CompletedProcess:
    """Run a command.  All commands run from REPO_ROOT."""
    merged_env = None
    if env:
        merged_env = {**os.environ, **env}
    try:
        return subprocess.run(
            cmd,
            check=check,
            capture_output=capture,
            timeout=timeout,
            cwd=REPO_ROOT,
            env=merged_env,
            **kwargs,
        )
    except subprocess.TimeoutExpired:
        print(f"\nCommand timed out after {timeout}s: {' '.join(cmd)}", file=sys.stderr)
        sys.exit(1)


def run_quiet(*cmd: str, **kwargs) -> subprocess.CompletedProcess:
    """Run a command, suppressing stdout and stderr."""
    return run(*cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, **kwargs)


def git(*args: str, capture: bool = False, check: bool = True) -> subprocess.CompletedProcess:
    """Run a git command."""
    return run("git", *args, capture=capture, check=check)


def git_output(*args: str) -> str:
    """Run a git command and return stripped stdout."""
    result = git(*args, capture=True, check=False)
    return result.stdout.decode().strip() if result.returncode == 0 else ""


def find_godot() -> str | None:
    """Locate the Godot binary.  Tries godot-4, godot4, godot."""
    for name in ("godot-4", "godot4", "godot"):
        path = shutil.which(name)
        if path:
            return path
    return None


def require_godot() -> str:
    """Locate Godot or exit with an error."""
    godot = find_godot()
    if not godot:
        print("Error: Godot not found (tried godot-4, godot4, godot)", file=sys.stderr)
        sys.exit(1)
    return godot


def run_godot(*args: str, headless_display: bool = False, **kwargs) -> subprocess.CompletedProcess:
    """Run Godot, optionally wrapping with xvfb-run on Linux.

    headless_display: if True and on Linux, prepend xvfb-run -a so that
    Godot gets a virtual display for script loading / GUT tests.
    """
    godot = require_godot()
    cmd: list[str] = []
    if headless_display and not IS_WINDOWS and shutil.which("xvfb-run"):
        cmd = ["xvfb-run", "-a"]
    cmd.append(godot)
    cmd.extend(args)
    return run(*cmd, **kwargs)


def ensure_symlink() -> None:
    """Ensure godot/target points to ../target (symlink or junction)."""
    link = REPO_ROOT / "godot" / "target"
    target = Path("../target")  # relative

    if link.is_symlink():
        try:
            current = Path(os.readlink(link))
        except OSError:
            current = None
        if current == target:
            return
        print(f"Fixing symlink {link} -> {target}")
        link.unlink()
    elif link.exists():
        # Something real is there (not a symlink) — don't clobber it.
        return

    print(f"Creating symlink {link} -> {target}")
    try:
        os.symlink(target, link, target_is_directory=True)
    except OSError:
        if IS_WINDOWS:
            # Symlinks may require dev mode; fall back to a directory junction.
            print("  symlink failed, trying directory junction...")
            # mklink /J needs an absolute target path.
            abs_target = (REPO_ROOT / "target").resolve()
            run("cmd", "/c", "mklink", "/J", str(link), str(abs_target))
        else:
            raise


def ensure_godot_imported() -> None:
    """Run Godot --import if the .godot/ cache doesn't exist yet."""
    dot_godot = REPO_ROOT / "godot" / ".godot"
    if not dot_godot.is_dir():
        print("First run: importing Godot project...")
        run_godot(
            "--path", str(REPO_ROOT / "godot"), "--headless", "--import", "--quit",
            check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )


def refresh_class_cache() -> str:
    """Delete and rebuild the global class cache.  Returns the cache path."""
    cache = REPO_ROOT / "godot" / ".godot" / "global_script_class_cache.cfg"
    if cache.exists():
        cache.unlink()
    print("Importing Godot project...")
    run_godot(
        "--path", str(REPO_ROOT / "godot"), "--headless", "--import", "--quit",
        check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    if not cache.exists():
        print("Error: Godot import failed to create global_script_class_cache.cfg", file=sys.stderr)
        sys.exit(1)
    return str(cache)


def gd_files() -> list[str]:
    """Return sorted .gd file paths for scripts/ and test/ under godot/."""
    patterns = ["godot/scripts/*.gd", "godot/test/*.gd"]
    files: list[str] = []
    for pat in patterns:
        files.extend(sorted(glob.glob(str(REPO_ROOT / pat))))
    return files


def venv_tool(name: str) -> str:
    """Return the path to a tool inside python/.venv, platform-aware."""
    venv = REPO_ROOT / "python" / ".venv"
    if IS_WINDOWS:
        return str(venv / "Scripts" / f"{name}.exe")
    return str(venv / "bin" / name)


def ensure_gd_tools() -> tuple[str, str]:
    """Ensure gdformat and gdlint are installed, return their paths."""
    gdformat = venv_tool("gdformat")
    gdlint = venv_tool("gdlint")
    if not os.path.isfile(gdformat) or not os.path.isfile(gdlint):
        print("GDScript tools not found — setting up Python venv...")
        venv_dir = str(REPO_ROOT / "python" / ".venv")
        run(sys.executable, "-m", "venv", venv_dir)
        pip = venv_tool("pip")
        run(pip, "install", "-r", str(REPO_ROOT / "python" / "requirements-dev.txt"))
    return gdformat, gdlint


def get_changed_files() -> str:
    """Return newline-separated list of files changed vs main (branch + staged + unstaged)."""
    branch = git_output("diff", "--name-only", "main...HEAD")
    staged = git_output("diff", "--name-only", "--cached")
    unstaged = git_output("diff", "--name-only")
    all_files = set()
    for block in (branch, staged, unstaged):
        for line in block.splitlines():
            if line.strip():
                all_files.add(line.strip())
    return "\n".join(sorted(all_files))


def human_size(nbytes: int) -> str:
    """Format a byte count as a human-readable string."""
    for unit in ("B", "K", "M", "G"):
        if nbytes < 1024:
            return f"{nbytes:.1f}{unit}" if unit != "B" else f"{nbytes}{unit}"
        nbytes /= 1024  # type: ignore[assignment]
    return f"{nbytes:.1f}T"


# ---------------------------------------------------------------------------
# Build info stamp
# ---------------------------------------------------------------------------


def write_build_info() -> None:
    """Write branch@hash to godot/.build_info for non-main debug builds."""
    build_info = REPO_ROOT / "godot" / ".build_info"
    branch = git_output("branch", "--show-current")
    if branch and branch != "main":
        short_hash = git_output("rev-parse", "--short", "HEAD")
        build_info.write_text(f"{branch} @ {short_hash}")
    else:
        build_info.unlink(missing_ok=True)


def clear_build_info() -> None:
    build_info = REPO_ROOT / "godot" / ".build_info"
    build_info.unlink(missing_ok=True)


# ---------------------------------------------------------------------------
# GDScript checks
# ---------------------------------------------------------------------------


def godot_script_check() -> None:
    """Build gdext and verify all GDScript files parse without errors."""
    require_godot()
    print("Building elven_canopy_gdext for GDScript check...")
    run("cargo", "build", "-p", "elven_canopy_gdext")
    refresh_class_cache()
    print("Checking GDScript parse validity...")
    result = run_godot(
        "--path", str(REPO_ROOT / "godot"), "--quit",
        headless_display=True, check=False, capture=True,
    )
    output = (result.stdout or b"").decode() + (result.stderr or b"").decode()
    if "SCRIPT ERROR" in output:
        print(output, file=sys.stderr)
        print("\nGDScript parse check failed!", file=sys.stderr)
        sys.exit(1)
    if "GDScript preload complete" not in output:
        print(output, file=sys.stderr)
        print("\nGDScript parse check failed: preload confirmation missing!", file=sys.stderr)
        sys.exit(1)
    print("GDScript parse check passed.")


def gdscript_unit_tests() -> None:
    """Run GDScript unit tests via GUT."""
    require_godot()
    ensure_godot_imported()
    print("Running GDScript unit tests (GUT)...")
    gut_timeout = int(os.environ.get("GUT_TIMEOUT", "300"))
    result = run_godot(
        "--path", str(REPO_ROOT / "godot"),
        "--headless", "--script", "res://test/gut_runner.gd",
        headless_display=True, check=False, timeout=gut_timeout,
    )
    if result.returncode != 0:
        print("GDScript unit tests failed!", file=sys.stderr)
        sys.exit(1)
    print("GDScript unit tests passed.")


# ---------------------------------------------------------------------------
# Subcommands
# ---------------------------------------------------------------------------


def cmd_debug() -> None:
    print("Building elven_canopy_gdext (debug)...")
    ensure_symlink()
    run("cargo", "build", "-p", "elven_canopy_gdext")
    write_build_info()
    print("Done. Run: cd godot && godot")


def cmd_release() -> None:
    print("Building elven_canopy_gdext (release)...")
    ensure_symlink()
    run("cargo", "build", "-p", "elven_canopy_gdext", "--release")
    clear_build_info()
    print("Done. Run: cd godot && godot")


def cmd_test() -> None:
    ensure_symlink()
    # Tabulosity tests run separately to avoid Cargo feature unification:
    # elven_canopy_sim depends on tabulosity with features=["serde"], which
    # would activate serde for tabulosity's own test targets.
    print("Running tabulosity tests...")
    run("cargo", "test", "-p", "tabulosity", "-p", "tabulosity_derive",
        "--", "--test-threads=16")
    print()
    print("Running tabulosity serde tests...")
    run("cargo", "test", "-p", "tabulosity", "--features", "serde",
        "--test", "serde", "--", "--test-threads=16")
    print()
    pkg_args = []
    for crate in TEST_CRATES:
        pkg_args.extend(["-p", crate])
    print("Running all other crate tests...")
    run("cargo", "test", *pkg_args, "--", "--test-threads=16")
    print()
    godot_script_check()
    print()
    print("All tests passed.")


def cmd_quicktest() -> None:
    ensure_symlink()
    changed = get_changed_files()

    tab_packages: list[str] = []
    other_packages: list[str] = []
    for crate_dir in ALL_CRATES:
        if any(f.startswith(f"{crate_dir}/") for f in changed.splitlines()):
            if crate_dir in ("tabulosity", "tabulosity_derive"):
                tab_packages.extend(["-p", crate_dir])
            else:
                other_packages.extend(["-p", crate_dir])

    has_rust = any(f.endswith((".rs", "Cargo.toml")) for f in changed.splitlines())

    if tab_packages:
        print(f"Running tabulosity tests: {' '.join(tab_packages)}")
        run("cargo", "test", *tab_packages, "--", "--test-threads=16")
        print()
        print("Running tabulosity serde tests...")
        run("cargo", "test", "-p", "tabulosity", "--features", "serde",
            "--test", "serde", "--", "--test-threads=16")
        print()

    if other_packages:
        if "multiplayer_tests" not in other_packages:
            other_packages.extend(["-p", "multiplayer_tests"])
        print(f"Running tests for: {' '.join(other_packages)}")
        run("cargo", "test", *other_packages, "--", "--test-threads=16")
        print()
    elif has_rust:
        print("Running multiplayer tests...")
        run("cargo", "test", "-p", "multiplayer_tests", "--", "--test-threads=16")
        print()
    else:
        print("No Rust changes detected, skipping Rust tests.")

    if any(f.endswith(".gd") for f in changed.splitlines()):
        godot_script_check()
        print()
        gdscript_unit_tests()
        print()

    print("All tests passed.")


def cmd_gdtest() -> None:
    ensure_symlink()
    gdscript_unit_tests()


def cmd_run() -> None:
    ensure_symlink()
    print("Building elven_canopy_gdext (debug)...")
    run("cargo", "build", "-p", "elven_canopy_gdext")
    write_build_info()
    refresh_class_cache()
    print("Launching Elven Canopy...")
    run_godot(
        "--path", str(REPO_ROOT / "godot"),
        env={"RUST_BACKTRACE": "1"},
    )


def cmd_run_branch(branch_name: str) -> None:
    ensure_symlink()
    if not branch_name:
        print("Usage: python3 scripts/build.py run-branch <branch-name>", file=sys.stderr)
        print("  branch-name can be exact (feature/F-foo) or partial (F-foo)", file=sys.stderr)
        sys.exit(1)

    print("Fetching latest from origin...")
    git("fetch", "--prune")

    # Resolve branch name: try exact, then feature/, then bug/ prefix.
    all_branches = git_output("branch", "-a", "--format=%(refname:short)")
    branch_list = all_branches.splitlines()
    resolved = ""
    for candidate in (branch_name, f"feature/{branch_name}", f"bug/{branch_name}"):
        if candidate in branch_list:
            resolved = candidate
            break
        if f"origin/{candidate}" in branch_list:
            resolved = candidate
            break

    if not resolved:
        print(f"Error: no branch found matching '{branch_name}'", file=sys.stderr)
        print(f"Tried: {branch_name}, feature/{branch_name}, bug/{branch_name}", file=sys.stderr)
        print(file=sys.stderr)
        print("Available branches:", file=sys.stderr)
        for b in sorted(branch_list):
            if b.startswith(("feature/", "bug/")):
                print(f"  {b}", file=sys.stderr)
        sys.exit(1)

    # Record HEAD before checkout so we can touch changed files afterward.
    prev_head = git_output("rev-parse", "HEAD")

    current_branch = git_output("branch", "--show-current")
    if current_branch != resolved:
        print(f"Switching to {resolved}...")
        git("checkout", resolved)
    else:
        print(f"Already on {resolved}.")

    if resolved == "main":
        git("pull")
    else:
        git("fetch", "origin", "main:main")
        local_rev = git_output("rev-parse", "HEAD")
        remote_rev = git_output("rev-parse", f"origin/{resolved}")
        if local_rev != remote_rev:
            print(f"Updating to {remote_rev[:8]}...")
            git("reset", "--hard", f"origin/{resolved}")
        else:
            print("Already up to date.")

    # Touch changed source files so cargo's mtime detection triggers rebuilds.
    new_head = git_output("rev-parse", "HEAD")
    if prev_head and new_head and prev_head != new_head:
        diff_output = git_output("diff", "--name-only", prev_head, new_head,
                                 "--", "*.rs", "Cargo.toml", "Cargo.lock")
        for f in diff_output.splitlines():
            fpath = REPO_ROOT / f
            if fpath.is_file():
                fpath.touch()

    print()
    print("Building elven_canopy_gdext (debug)...")
    run("cargo", "build", "-p", "elven_canopy_gdext")
    write_build_info()
    refresh_class_cache()
    print("Launching Elven Canopy...")
    run_godot(
        "--path", str(REPO_ROOT / "godot"),
        env={"RUST_BACKTRACE": "1"},
    )


def cmd_relay() -> None:
    ensure_symlink()
    print("Building standalone relay (release, LTO, stripped)...")
    run("cargo", "build", "-p", "elven_canopy_relay", "--profile", "relay-release", "--bin", "relay")
    ext = ".exe" if IS_WINDOWS else ""
    relay_bin = REPO_ROOT / "target" / "relay-release" / f"relay{ext}"
    if relay_bin.is_file():
        size = human_size(relay_bin.stat().st_size)
        print(f"Done. Binary: {relay_bin} ({size})")
        print(f"Run:  {relay_bin} --help")
    else:
        print(f"Done. Binary: {relay_bin}")


def cmd_coverage() -> None:
    ensure_symlink()
    if not shutil.which("cargo-llvm-cov"):
        print("cargo-llvm-cov not found. Install with: cargo install cargo-llvm-cov", file=sys.stderr)
        sys.exit(1)
    # Tabulosity runs separately to avoid Cargo feature unification.
    print("Running tabulosity coverage...")
    run("cargo", "llvm-cov", "--no-report", "-p", "tabulosity", "-p", "tabulosity_derive",
        "--", "--test-threads=16")
    print()
    print("Running other crate coverage...")
    run("cargo", "llvm-cov", "--no-report", "--workspace",
        "--exclude", "elven_canopy_gdext", "--exclude", "tabulosity", "--exclude", "tabulosity_derive",
        "--", "--test-threads=16")
    print()
    print("Generating HTML report...")
    run("cargo", "llvm-cov", "report", "--html", "--output-dir", "target/llvm-cov")
    print()
    print("Generating LCOV file...")
    run("cargo", "llvm-cov", "report", "--lcov", "--output-path", "target/llvm-cov/lcov.info")
    print()
    print("Coverage report: target/llvm-cov/html/index.html")
    print("LCOV file:       target/llvm-cov/lcov.info")


def cmd_check() -> None:
    ensure_symlink()
    print("Checking Rust formatting...")
    run("cargo", "fmt", "--all", "--check")
    print()

    # Scope clippy to changed crates on feature branches.
    current_branch = git_output("branch", "--show-current")
    clippy_args: list[str] = ["--workspace"]
    skip_clippy = False

    if current_branch and current_branch != "main":
        changed = get_changed_files()
        clippy_packages: list[str] = []
        for crate_dir in ALL_CRATES:
            if any(f.startswith(f"{crate_dir}/") for f in changed.splitlines()):
                clippy_packages.extend(["-p", crate_dir])
        if clippy_packages:
            clippy_args = clippy_packages
        elif any(f.endswith((".rs", "Cargo.toml")) for f in changed.splitlines()):
            clippy_args = ["--workspace"]
        else:
            skip_clippy = True

    if skip_clippy:
        print("No Rust changes detected, skipping Clippy.")
    else:
        scope_str = " ".join(clippy_args)
        print(f"Running Clippy ({scope_str})...")
        run("cargo", "clippy", *clippy_args, "--", "-D", "warnings")
    print()

    # GDScript formatting and linting.
    gdformat, gdlint = ensure_gd_tools()
    files = gd_files()
    if not files:
        print("No .gd files found, skipping GDScript checks.")
    else:
        print("Checking GDScript formatting...")
        run(gdformat, "--check", "--line-length", "100", *files)
        print()
        print("Running GDScript linter...")
        run(gdlint, *files)
    print()
    print("All checks passed.")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Build script for Elven Canopy",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    sub = parser.add_subparsers(dest="command")

    sub.add_parser("debug", help="Debug build (default)")
    sub.add_parser("release", help="Release build")
    sub.add_parser("test", help="Run all crate tests")
    sub.add_parser("quicktest", help="Test only crates changed vs main")
    sub.add_parser("gdtest", help="Run GDScript unit tests (GUT)")
    sub.add_parser("run", help="Debug build then launch the game")
    rb = sub.add_parser("run-branch", help="Fetch, checkout branch, sync, build+run")
    rb.add_argument("branch", help="Branch name (exact or partial)")
    sub.add_parser("relay", help="Optimized standalone relay binary")
    sub.add_parser("check", help="Run fmt, clippy, gdformat, gdlint checks")
    sub.add_parser("coverage", help="Generate HTML code coverage report")

    args = parser.parse_args()
    command = args.command or "debug"

    try:
        if command == "debug":
            cmd_debug()
        elif command == "release":
            cmd_release()
        elif command == "test":
            cmd_test()
        elif command == "quicktest":
            cmd_quicktest()
        elif command == "gdtest":
            cmd_gdtest()
        elif command == "run":
            cmd_run()
        elif command == "run-branch":
            cmd_run_branch(args.branch)
        elif command == "relay":
            cmd_relay()
        elif command == "check":
            cmd_check()
        elif command == "coverage":
            cmd_coverage()
        else:
            parser.print_help()
            sys.exit(1)
    except subprocess.CalledProcessError:
        print(file=sys.stderr)
        print("========================================", file=sys.stderr)
        print("FAILED — Error in scripts/build.py", file=sys.stderr)
        print("ERROR: build step Failed (see above)", file=sys.stderr)
        print("========================================", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
