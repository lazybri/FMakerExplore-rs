#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODE="static"
BUILD_TYPE="Release"
BUILD_DIR=""
RUST_TARGET=""
QT_PREFIX="${QT_STATIC_PREFIX:-$HOME/.local/qt6-static-macos}"
QT_PREFIX_SET=0
QT_VERSION="${QT_STATIC_VERSION:-6.12.0}"
QT_WORK="${QT_STATIC_WORK:-$HOME/.cache/fmaker-explore/qt-static-macos}"
REBUILD_QT=0
CLEAN=0
JOBS=""
RUN_TESTS=0
RUN_APP=0
PACKAGE=0

usage() {
    cat <<'EOF'
Build the FMakerExplore Qt 6 GUI on macOS. Static linking is the default.

Usage: scripts/build-macos.sh [options]

Options:
  --static         Build static Qt (if needed) and a static .app (default)
  --dynamic        Use Homebrew/system Qt and a dynamically linked .app
  --debug          Debug build (only valid with --dynamic)
  --clean          Remove the build directory and static Rust target
  --rebuild-qt     Force a reconfigure/rebuild of the static Qt tree
  --qt-prefix DIR  Static Qt installation (default: $QT_STATIC_PREFIX or
                   $HOME/.local/qt6-static-macos); with --dynamic this is
                   passed as CMAKE_PREFIX_PATH
  --qt-version VER Qt version to build when static Qt is missing (default: 6.12.0)
  --jobs N         Parallel jobs (default: CPU count)
  --test           Run CTest if any tests are configured
  --package        Create dist/fmaker-explore-gui-macos[-static].zip
  --run            Start the GUI after a successful build
  -h, --help       Show this help

Static Qt is downloaded and built (qtbase only, Cocoa platform plugin) when it
is not already present. This takes roughly 10-30 minutes. The static .app
includes Qt, the Rust core and libusb; macOS system frameworks remain dynamic.
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
        -h|--help) usage; exit 0 ;;
        *) echo "error: unknown option: $1" >&2; usage; exit 2 ;;
    esac
    shift
done

if ! [[ "$JOBS" =~ ^[1-9][0-9]*$ || -z "$JOBS" ]]; then
    echo "error: --jobs must be a positive integer" >&2
    exit 2
fi
JOBS="${JOBS:-$(sysctl -n hw.ncpu 2>/dev/null || getconf _NPROCESSORS_ONLN 2>/dev/null || echo 2)}"

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "error: scripts/build-macos.sh must be run on macOS" >&2
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

for tool in cmake cargo clang++; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "error: $tool was not found in PATH" >&2
        echo "  Install the Xcode command line tools: xcode-select --install" >&2
        echo "  Install CMake/Ninja/Qt:                brew install cmake ninja qt" >&2
        exit 1
    fi
done

if ! xcode-select -p >/dev/null 2>&1; then
    echo "error: the Xcode command line tools are not configured." >&2
    echo "       Run: xcode-select --install" >&2
    exit 1
fi

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

    for tool in cmake ninja python3 perl clang++ curl tar; do
        if ! command -v "$tool" >/dev/null 2>&1; then
            echo "error: $tool is required to build static Qt" >&2
            echo "  Install with: brew install cmake ninja python3 perl" >&2
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
            -no-framework \
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
    local app="$1"
    local mode="$2"
    local qt_prefix="${3:-}"
    local dist zip deploy
    dist="$ROOT/dist"
    if [[ "$mode" == "static" ]]; then
        zip="$dist/fmaker-explore-gui-macos-static.zip"
    else
        zip="$dist/fmaker-explore-gui-macos.zip"
    fi

    if [[ ! -d "$app" ]]; then
        echo "error: macOS application bundle not found at $app" >&2
        exit 1
    fi

    mkdir -p "$dist"
    if [[ "$mode" == "dynamic" ]]; then
        if command -v macdeployqt >/dev/null 2>&1; then
            deploy="$(command -v macdeployqt)"
        elif [[ -n "$qt_prefix" && -x "$qt_prefix/bin/macdeployqt" ]]; then
            deploy="$qt_prefix/bin/macdeployqt"
        else
            echo "error: macdeployqt was not found; add the Qt bin directory to PATH" >&2
            exit 1
        fi
        "$deploy" "$app" -always-overwrite
    fi

    rm -f "$zip"
    ditto -c -k --keepParent "$app" "$zip"
    echo "Packaged: $zip"
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

APP="$BUILD_DIR/fmaker-explore-gui.app"
BINARY="$APP/Contents/MacOS/fmaker-explore-gui"
if [[ ! -d "$APP" || ! -x "$BINARY" ]]; then
    echo "error: the GUI application bundle was not produced at $APP" >&2
    exit 1
fi

echo "Built: $BINARY"

if [[ $RUN_TESTS == 1 ]]; then
    if [[ -f "$BUILD_DIR/CTestTestfile.cmake" ]]; then
        ctest --test-dir "$BUILD_DIR" --output-on-failure
    else
        echo "No CTest tests are configured on macOS."
    fi
fi

if [[ $PACKAGE == 1 ]]; then
    package_app "$APP" "$MODE" "$QT_PREFIX"
fi

if [[ $RUN_APP == 1 ]]; then
    exec "$BINARY"
fi
