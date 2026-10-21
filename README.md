# Fuji Mitsu maker Explore

FMakerExplore is a filesystem utility for the Fuji and Mitsu MakerCmd interface. It connects to the handset over USB, enters Maker Mode when necessary, and provides access to files and directories on the phone.

The project contains:

- **`fmaker-explore`** — the Rust core and command-line tool.
- **`fmaker-explore-gui`** — a small Qt 6 Widgets file manager that talks to the Rust core through a C ABI.

## Features

- Connect to and probe the MakerCmd interface.
- List available drive letters and directory contents.
- Display file size, attributes, and timestamp data.
- Download files with checksum validation and resume support.
- Upload files with checksum validation and automatic retry handling.
- Create directories, delete files, and change the read-only attribute.
- Display the available space on the `D:` drive.
- Browse the phone with a simple Qt 6 GUI, including light/dark mode switching.

## Requirements

- Rust 1.85 or newer
- CMake 3.21 or newer and a C++17 compiler (for the GUI)
- Qt 6.4 or newer with the `Widgets` and `Concurrent` modules for dynamic builds (Qt 6.5+ enables native system theme detection)
- Linux: `pkexec`/polkit for privilege elevation; `libudev` development files when building dynamically

`libusb` is built from source through the `vendored` feature of `rusb`, so a system libusb installation is not required.

```bash
# Debian/Ubuntu
sudo apt install build-essential cmake ninja-build libudev-dev qt6-base-dev
# Fedora
sudo dnf install gcc-c++ cmake ninja-build libudev-devel qt6-qtbase-devel
# macOS
brew install cmake ninja qt
```

## Building the GUI

There are exactly three local build scripts, one per platform. **Static linking
is the default on all three**: the script builds a static Qt 6 (qtbase only)
when one is not already installed, then links Qt, the Rust core, the bundled
libusb, and the C++ runtime into the GUI. Use `--dynamic` / `-Dynamic` to build
against an installed Qt instead.

| Platform | Script | Default build | Package output (`--package` / `-Package`) |
| --- | --- | --- | --- |
| Linux | `./scripts/build-linux.sh` | `build-static/fmaker-explore-gui` | `dist/fmaker-explore-gui-linux-<arch>-static.tar.gz` |
| macOS | `./scripts/build-macos.sh` | `build-static/fmaker-explore-gui.app` | `dist/fmaker-explore-gui-macos-static.zip` |
| Windows | `.\scripts\build-windows.ps1` | `build-static\fmaker-explore-gui.exe` | `dist\fmaker-explore-gui-windows-x86_64-static.zip` |

With `--dynamic` / `-Dynamic` the package names drop the `-static` suffix and
the build directory is `build/`.

```bash
# Linux: static (first run also builds Qt, roughly 10-40 minutes)
./scripts/build-linux.sh --clean --package --run

# Linux: dynamic, using the system Qt
./scripts/build-linux.sh --dynamic --clean --package
```

```bash
# macOS: static
./scripts/build-macos.sh --clean --package --run

# macOS: dynamic, using Homebrew/system Qt
./scripts/build-macos.sh --dynamic --clean --package
```

```powershell
# Windows: static
.\scripts\build-windows.ps1 -Clean -Package

# Windows: dynamic, using an installed Qt
.\scripts\build-windows.ps1 -Dynamic -Configuration Release -Clean -Package
```

Options:

| Option | Linux / macOS | Windows | Meaning |
| --- | --- | --- | --- |
| Static (default) | `--static` | *(default)* | Build/use static Qt and a static GUI |
| Dynamic | `--dynamic` | `-Dynamic` | Use an installed Qt |
| Clean | `--clean` | `-Clean` | Remove the build directory (and `target-static`) |
| Jobs | `--jobs N` | `-Jobs N` | Parallel build jobs |
| Package | `--package` | `-Package` | Create the archive under `dist/` |
| Run | `--run` | `-Run` | Start the GUI after building |
| Test | `--test` | `-Test` | Run CTest if tests are configured |
| Qt prefix | `--qt-prefix DIR` | `-QtPrefix DIR` | Static Qt install, or `CMAKE_PREFIX_PATH` for dynamic |
| Qt version | `--qt-version VER` | `-QtVersion VER` | Qt version to build when static Qt is missing |
| Rebuild Qt | `--rebuild-qt` | `-RebuildQt` | Force a static Qt rebuild |
| Debug | `--dynamic --debug` | `-Dynamic -Configuration Debug` | Debug build (static builds are Release-only) |

Linux additionally supports `--test`, `--no-elevate` (with `--run`), and
`--keep-debug`. macOS supports `--test`. Windows accepts
`-Configuration Debug|Release`, `-BuildDir DIR`, and `-QtWork DIR`.

Qt is found through `QTDIR`, `QT_ROOT_DIR`, `qtpaths6`, or Homebrew on macOS.
On Windows the script imports the MSVC environment with `vswhere`; dynamic
builds use Ninja when both Ninja and MSVC are available, otherwise they
prebuild the Rust library and use the Visual Studio generator.

`--package` / `-Package` runs `macdeployqt` or `windeployqt` for dynamic Qt
builds. Static packages contain no Qt DLLs or frameworks and need no deployment
step.

### Static build details

| Platform | Static Qt prefix | Work directory | Notes |
| --- | --- | --- | --- |
| Linux | `~/.local/qt6-static` | `~/.cache/fmaker-explore/qt-static` | XCB, Wayland, offscreen and compose plugins |
| macOS | `~/.local/qt6-static-macos` | `~/.cache/fmaker-explore/qt-static-macos` | Cocoa plugin; system frameworks stay dynamic |
| Windows | `%LOCALAPPDATA%\fmakerexplore\qt6-static` | `%LOCALAPPDATA%\fmakerexplore\qt-static` | MSVC `/MT` + Rust `+crt-static` |

Override the prefix with `QT_STATIC_PREFIX`, the work directory with
`QT_STATIC_WORK`, and the Qt version with `QT_STATIC_VERSION`. Static
builds use `build-static/` and `target-static/`, and pass:

- `-DFM_STATIC_QT=ON` — link the platform and style plugins from static Qt.
  Linux imports only the XCB, Wayland, offscreen, and compose input-context
  plugins.
- `-DFM_RUST_TARGET_DIR=target-static` — keep the static Rust artifacts
  separate.
- `-DFM_LIBUSB_UDEV=OFF` — on Linux this uses libusb's netlink backend, so
  `libudev` is not required.
- `-DFM_STATIC_RUNTIME=ON` — Linux links `libstdc++`/`libgcc` statically;
  Windows uses `/MT` and builds Rust with `+crt-static`.

On Linux the resulting binary embeds Qt 6, the Rust core, libusb, and the C++
runtime. Its direct dynamic dependencies are system libraries only: glibc,
X11/XCB, Wayland, EGL/OpenGL, fontconfig, ICU, D-Bus, Glib, and similar. A
fully static glibc build is not attempted.

Static build prerequisites:

- Linux: `build-essential cmake ninja-build python3 perl curl pkg-config`, the
  XCB/Wayland development packages (the script prints the exact names if one is
  missing), and `wayland-scanner`.
- macOS: Xcode command line tools plus `brew install cmake ninja python3 perl`.
- Windows: Visual Studio 2022 C++ x64 tools, CMake, Ninja
  (`choco install ninja`), Python 3 (`python` or `py`), and the Rust MSVC
  toolchain (`rustup default stable-msvc`). Run the script from a normal
  PowerShell window; it locates and imports `vcvars64.bat` itself.

The CI workflow uses the same static default on all three platforms; see
[Continuous integration](#continuous-integration).

A manual dynamic build looks like:

```bash
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build build
```

A manual static build after the script has produced a static Qt:

```bash
cmake -S . -B build-static -G Ninja -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_PREFIX_PATH="$HOME/.local/qt6-static" \
  -DFM_STATIC_QT=ON -DFM_LIBUSB_UDEV=OFF -DFM_STATIC_RUNTIME=ON \
  -DFM_RUST_TARGET_DIR=target-static
cmake --build build-static
```

CMake options:

| Option | Description |
| --- | --- |
| `-DFM_STATIC_QT=ON` | Import the platform and style plugins from a static Qt installation |
| `-DFM_LIBUSB_UDEV=OFF` | Build/link libusb without libudev |
| `-DFM_STATIC_RUNTIME=ON` | Link the C++ runtime statically (`-static-libstdc++`/`-static-libgcc` with GCC, `/MT` with MSVC) |
| `-DFM_RUST_LIB=<path>` | Link a prebuilt Rust static library instead of invoking Cargo |
| `-DFM_RUST_TARGET_DIR=<path>` | Cargo target directory (default `./target`) |

## Using the GUI

- The app connects to `06d3:21b0` automatically on startup. Use **Connect** / **Reconnect** to retry.
- Navigate with the path bar, the **Up** action, or the drive selector.
- Double-click a folder to enter it. Double-click a file to download it.
- Use the context menu to open, download, delete, or toggle the read-only attribute.
- **New Folder**, **Upload**, and **Download** operate on the current directory.
- The status bar shows `D:` free space and transfer progress.
- The **Dark mode** / **Light mode** action (or `Ctrl+Shift+D`) switches palettes and is remembered between sessions.

The GUI uses the built-in Qt Fusion style with palette-based colors only — no stylesheets or custom themes. Native file dialogs are disabled so that their colors follow the selected theme on all platforms.

### Linux privileges (polkit)

On Linux the GUI requests root privileges through `pkexec` before creating the window, because the MakerCmd USB interface may not be accessible to a normal user. A polkit authentication agent must be registered in the desktop session; it displays the usual authentication dialog.

- `--no-elevate` (or `FM_EXPLORE_NO_ELEVATE=1`) starts without requesting root privileges.
- `--print-elevation` prints the exact `pkexec` command and exits without running it.
- `--version`, `--help`, and `--smoke-test` never request elevation.

`pkexec` starts the program with a sanitized environment, so the launcher forwards only what is needed to attach to the user session: `DISPLAY`, `WAYLAND_DISPLAY`, `XAUTHORITY`, `XDG_RUNTIME_DIR`, the session bus address, locale, and Qt scaling/input-method settings. Library search paths and Qt plugin paths are deliberately not forwarded.

Running a GUI as root is inherently sensitive: downloaded files and application settings created by the elevated process are owned by root. Some Wayland compositors reject root clients; in that case use `--no-elevate` with appropriate udev permissions, or force the X11 backend with `QT_QPA_PLATFORM=xcb` when XWayland is available. If `pkexec` is not installed, the GUI prints a warning and continues without elevation.

Only the GUI elevates. The command-line tool stays at the current user's privileges; use udev permissions or run it with `sudo` when needed.

After a Linux build, the launcher unit test can be run with:

```bash
ctest --test-dir build --output-on-failure
```

## Building and using the CLI

```bash
cargo build --release
./target/release/fmaker-explore --connect
```

Or install it:

```bash
cargo install --path .
```

Examples:

```bash
fmaker-explore --listpartition
fmaker-explore --listdir 'D:\WcdmaMp\'
fmaker-explore --listfile 'D:\WcdmaMp\file.bin'
fmaker-explore --pull 'D:\WcdmaMp\file.bin' ./file.bin
fmaker-explore --push ./file.bin 'D:\WcdmaMp\file.bin'
fmaker-explore --mkdir 'D:\WcdmaMp\newdir'
fmaker-explore --delete 'D:\WcdmaMp\file.bin'
fmaker-explore --setreadonly 'D:\WcdmaMp\file.bin' on
fmaker-explore --checkdavailable
```

Use `--overwrite` to replace an existing local file, or `--no-resume` to discard an existing partial download. The upload command refuses to replace an existing remote file. Remote paths must be absolute, use a drive letter, and contain only ASCII characters.

### CLI options

| Option | Description |
| --- | --- |
| `--vid VID` | USB vendor ID (default `0x06d3`) |
| `--pid PID` | USB product ID (default `0x21b0`) |
| `--timeout SECONDS` | USB transfer timeout (default `5`) |
| `--maker-wait SECONDS` | Time to wait for Maker Mode (default `60`) |
| `--retries COUNT` | Retry count for interrupted transfers (default `3`) |
| `--no-resume` | Discard an existing partial download |
| `--overwrite` | Replace an existing local file |
| `-v`, `--verbose` | Print protocol details to stderr |

## Connection Workflow

1. Start the phone and connect it to the computer over USB.
2. Run the GUI or a CLI command.
3. The tool checks whether Maker Mode is already active.
4. If necessary, it switches the USB interface and waits for Maker Mode to become available.
5. The requested filesystem operation is then performed.

The default USB device ID is `06d3:21b0`. The CLI accepts `--vid` and `--pid` to select another compatible device.

## Platform notes

- **Wayland/Linux**: Qt selects the Wayland platform plugin automatically. The window uses the `fmaker-explore-gui` desktop file name as its application ID.
- **Windows**: the static CI package is self-contained. Dynamic builds need `windeployqt`.
- **macOS**: the static CI package contains a self-contained `.app`. Dynamic builds need `macdeployqt`.
- USB access permissions still apply (udev rules or equivalent, depending on the platform).

## Continuous integration

[`.github/workflows/build.yml`](.github/workflows/build.yml) runs the Rust tests and builds the GUI as a static package on Linux, Windows, and macOS. The static Qt installation is cached between runs; the first run for a cache key builds Qt 6.12 from source and takes considerably longer.

Every run uploads workflow artifacts:

- Linux: `fmaker-explore-gui-linux-x86_64-static.tar.gz`
- Windows: `fmaker-explore-gui-windows-x86_64-static.zip`
- macOS: `fmaker-explore-gui-macos-static.zip`

Pushing a tag also publishes these files to a GitHub Release automatically:

```bash
git tag v0.1.0
git push origin v0.1.0
```

Branch pushes and pull requests build and upload workflow artifacts without creating a release.

## Project layout

- `src/protocol.rs` — MakerCmd framing, path validation, and shared helpers.
- `src/client.rs` — USB transport, Maker Mode entry, and filesystem operations.
- `src/cli.rs` — command-line interface and action dispatch.
- `src/ffi.rs` — C ABI used by the Qt front-end.
- `src/error.rs` — error types shared across the crate.
- `gui/` — Qt 6 Widgets application.
- `gui/src/linuxlauncher.*` — Linux pkexec/polkit elevation and environment forwarding.
- `gui/tests/` — launcher unit tests (run with CTest on Linux).
- `scripts/` — the three local build scripts: `build-linux.sh`, `build-macos.sh`, and `build-windows.ps1`
