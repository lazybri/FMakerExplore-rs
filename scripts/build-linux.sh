#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODE="static"
BUILD_TYPE="Release"
BUILD_DIR=""
RUST_TARGET=""
QT_PREFIX="${QT_STATIC_PREFIX:-$HOME/.local/qt6-static}"
QT_PREFIX_SET=0
QT_VERSION="${QT_STATIC_VERSION:-6.12.0}"
QT_WORK="${QT_STATIC_WORK:-$HOME/.cache/fmaker-explore/qt-static}"
REBUILD_QT=0
CLEAN=0
JOBS=""
RUN_TESTS=0
RUN_APP=0
NO_ELEVATE=0
PACKAGE=0
KEEP_DEBUG=0

usage() {
    cat <<'EOF'
Build the FMakerExplore Qt 6 GUI on Linux. Static linking is the default.

Usage: scripts/build-linux.sh [options]

Options:
  --static         Build static Qt (if needed) and a static GUI (default)
  --dynamic        Use the system Qt and a dynamically linked GUI
  --debug          Debug build (only valid with --dynamic)
  --clean          Remove the build directory and Rust target directory
  --rebuild-qt     Force a reconfigure/rebuild of the static Qt tree
  --qt-prefix DIR  Static Qt installation (default: $QT_STATIC_PREFIX or
                   $HOME/.local/qt6-static); with --dynamic this is passed as
                   CMAKE_PREFIX_PATH
  --qt-version VER Qt version to build when static Qt is missing (default: 6.12.0)
  --jobs N         Parallel jobs (default: CPU count)
  --test           Run CTest tests after building
  --package        Create a tar.gz under ./dist
  --run            Start the GUI after a successful build
  --no-elevate     With --run, set FM_EXPLORE_NO_ELEVATE=1
  --keep-debug     Do not strip the static release binary
  -h, --help       Show this help

Static Qt is downloaded and built (qtbase only, with XCB/Wayland) when it is
not already present. This takes roughly 10-30 minutes. The static GUI includes
Qt, the Rust core, libusb and the C++ runtime; glibc and system libraries stay
dynamic.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --static) MODE="static" ;;
        --dynamic) MODE="dynamic" ;;
        --debug) BUILD_TYPE="Debug" ;;
        --clean) CLEAN=1 ;;
        --rebuild-qt) REBUILD_QT=1 ;;
        --qt-prefix)
            shift
            [[ $# -gt 0 ]] || { echo "error: --qt-prefix needs a value" >&2; exit 2; }
            QT_PREFIX="$1"
            QT_PREFIX_SET=1
            ;;
        --qt-version)
            shift
            [[ $# -gt 0 ]] || { echo "error: --qt-version needs a value" >&2; exit 2; }
            QT_VERSION="$1"
            ;;
        --jobs)
            shift
            [[ $# -gt 0 ]] || { echo "error: --jobs needs a value" >&2; exit 2; }
            JOBS="$1"
            ;;
        --test) RUN_TESTS=1 ;;
        --package) PACKAGE=1 ;;
        --run) RUN_APP=1 ;;
        --no-elevate) NO_ELEVATE=1 ;;
        --keep-debug) KEEP_DEBUG=1 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "error: unknown option: $1" >&2; usage; exit 2 ;;
    esac
    shift
done

if ! [[ "$JOBS" =~ ^[1-9][0-9]*$ || -z "$JOBS" ]]; then
    echo "error: --jobs must be a positive integer" >&2
    exit 2
fi
JOBS="${JOBS:-$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 2)}"

if [[ "$(uname -s)" != "Linux" ]]; then
    echo "error: scripts/build-linux.sh must be run on Linux" >&2
    exit 1
fi

if [[ $MODE == "static" && $BUILD_TYPE == "Debug" ]]; then
    echo "error: static builds are Release-only; use --dynamic --debug" >&2
    exit 2
fi

if [[ ! "$QT_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "error: --qt-version must look like 6.12.0" >&2
    exit 2
fi

case "$QT_PREFIX" in
    "~/"*) QT_PREFIX="$HOME/${QT_PREFIX#~/}" ;;
esac

for tool in cmake cargo; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "error: $tool was not found in PATH" >&2
        exit 1
    fi
done

build_static_qt() {
    local version="$1"
    local prefix="$2"
    local work="$3"
    local jobs="$4"
    local series source build archive url

    series="${version%.*}"
    source="$work/qtbase-everywhere-src-$version"
    build="$work/build-$version"
    archive="$work/qtbase-everywhere-src-$version.tar.xz"
    url="https://download.qt.io/official_releases/qt/$series/$version/submodules/qtbase-everywhere-src-$version.tar.xz"

    if [[ -f "$prefix/lib/libQt6Widgets.a" && $REBUILD_QT == 0 ]]; then
        echo "Using static Qt at $prefix"
        return 0
    fi

    for tool in cmake ninja python3 perl g++ curl tar pkg-config wayland-scanner; do
        if ! command -v "$tool" >/dev/null 2>&1; then
            echo "error: $tool is required to build static Qt" >&2
            echo "  Debian/Ubuntu: sudo apt install build-essential cmake ninja-build python3 perl curl pkg-config libwayland-dev libxcb1-dev libxcb-render-util0-dev libxkbcommon-dev libxkbcommon-x11-dev libfontconfig1-dev" >&2
            exit 1
        fi
    done

    for module in xcb xcb-renderutil xkbcommon xkbcommon-x11 wayland-client wayland-cursor wayland-egl fontconfig; do
        if ! pkg-config --exists "$module"; then
            echo "error: required pkg-config module '$module' was not found" >&2
            echo "  Debian/Ubuntu: sudo apt install libwayland-dev libxcb1-dev libxcb-render-util0-dev libxkbcommon-dev libxkbcommon-x11-dev libfontconfig1-dev" >&2
            exit 1
        fi
    done

    mkdir -p "$work" "$prefix"

    if [[ ! -d "$source" ]]; then
        if [[ ! -s "$archive" ]]; then
            echo "Downloading Qt $version sources..."
            curl -fL --retry 3 -o "$archive" "$url"
        fi
        echo "Extracting Qt sources..."
        tar -xJf "$archive" -C "$work"
    fi

    if [[ $REBUILD_QT == 1 ]]; then
        rm -rf "$build"
    fi
    mkdir -p "$build"

    echo "Configuring static Qt $version..."
    (
        cd "$build"
        "$source/configure" \
            -static \
            -release \
            -opensource \
            -confirm-license \
            -nomake examples \
            -nomake tests \
            -prefix "$prefix" \
            -qt-zlib \
            -qt-libpng \
            -qt-libjpeg \
            -qt-freetype \
            -qt-harfbuzz \
            -qt-pcre \
            -no-feature-textmarkdownreader
    )

    echo "Building static Qt (${jobs} jobs)..."
    cmake --build "$build" --parallel "$jobs"

    echo "Installing static Qt into $prefix..."
    cmake --install "$build"

    if [[ ! -f "$prefix/lib/libQt6Widgets.a" ]]; then
        echo "error: static Qt installation did not produce libQt6Widgets.a" >&2
        exit 1
    fi
}

detect_dynamic_qt() {
    local prefix="${QTDIR:-${QT_ROOT_DIR:-}}"
    if [[ -z "$prefix" ]] && command -v qtpaths6 >/dev/null 2>&1; then
        prefix="$(qtpaths6 --query QT_INSTALL_PREFIX 2>/dev/null || true)"
    fi
    if [[ -z "$prefix" ]] && command -v brew >/dev/null 2>&1; then
        prefix="$(brew --prefix qt 2>/dev/null || true)"
    fi
    printf '%s' "$prefix"
}

package_app() {
    local binary="$1"
    local mode="$2"
    local arch dist stage name archive
    arch="$(uname -m)"
    dist="$ROOT/dist"
    stage="$dist/.stage-linux-$arch-$mode"
    name="fmaker-explore-gui-linux-$arch"
    if [[ "$mode" == "static" ]]; then
        name+="-static"
    fi
    archive="$dist/$name.tar.gz"

    mkdir -p "$dist"
    rm -rf "$stage"
    mkdir -p "$stage"
    cp "$binary" "$stage/fmaker-explore-gui"
    cp "$ROOT/README.md" "$stage/README.md"
    rm -f "$archive"
    tar -C "$stage" -czf "$archive" .
    rm -rf "$stage"
    echo "Packaged: $archive"
}

if [[ $MODE == "static" ]]; then
    BUILD_DIR="$ROOT/build-static"
    RUST_TARGET="$ROOT/target-static"
    build_static_qt "$QT_VERSION" "$QT_PREFIX" "$QT_WORK" "$JOBS"
else
    BUILD_DIR="$ROOT/build"
    RUST_TARGET="$ROOT/target"
    if [[ $QT_PREFIX_SET == 0 ]]; then
        QT_PREFIX="$(detect_dynamic_qt)"
    fi
fi

if [[ $CLEAN == 1 ]]; then
    rm -rf "$BUILD_DIR"
    if [[ $MODE == "static" ]]; then
        rm -rf "$RUST_TARGET"
    fi
fi

CMAKE_ARGS=(
    -S "$ROOT"
    -B "$BUILD_DIR"
    -DCMAKE_BUILD_TYPE="$BUILD_TYPE"
    -DFM_RUST_TARGET_DIR="$RUST_TARGET"
)
if [[ $MODE == "static" ]]; then
    CMAKE_ARGS+=(
        -DCMAKE_PREFIX_PATH="$QT_PREFIX"
        -DFM_STATIC_QT=ON
        -DFM_LIBUSB_UDEV=OFF
        -DFM_STATIC_RUNTIME=ON
    )
else
    if [[ -n "$QT_PREFIX" ]]; then
        CMAKE_ARGS+=(-DCMAKE_PREFIX_PATH="$QT_PREFIX")
    fi
fi

GENERATOR=()
if command -v ninja >/dev/null 2>&1; then
    GENERATOR=(-G Ninja)
else
    echo "note: Ninja was not found; using the default CMake generator." >&2
fi

cmake "${GENERATOR[@]}" "${CMAKE_ARGS[@]}"
cmake --build "$BUILD_DIR" --parallel "$JOBS"

BINARY="$BUILD_DIR/fmaker-explore-gui"
if [[ ! -x "$BINARY" ]]; then
    echo "error: the GUI binary was not produced at $BINARY" >&2
    exit 1
fi

if [[ $MODE == "static" && $KEEP_DEBUG == 0 ]] && command -v strip >/dev/null 2>&1; then
    strip --strip-unneeded "$BINARY"
fi

echo "Built: $BINARY"
if [[ $MODE == "static" ]] && command -v readelf >/dev/null 2>&1; then
    echo "Direct dynamic dependencies:"
    readelf -d "$BINARY" | sed -n 's/.*NEEDED.*\[\(.*\)\]/\  \1/p'
fi

if [[ $RUN_TESTS == 1 ]]; then
    ctest --test-dir "$BUILD_DIR" --output-on-failure
fi

if [[ $PACKAGE == 1 ]]; then
    package_app "$BINARY" "$MODE"
fi

if [[ $RUN_APP == 1 ]]; then
    if [[ $NO_ELEVATE == 1 ]]; then
        FM_EXPLORE_NO_ELEVATE=1 exec "$BINARY"
    else
        exec "$BINARY"
    fi
fi
