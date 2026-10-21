<#
.SYNOPSIS
Build the FMakerExplore Qt 6 GUI on Windows. Static linking is the default.

.DESCRIPTION
By default this script builds a static Qt 6 (qtbase only) when a static Qt
installation is not already present, then builds the GUI with:
  - a static Qt,
  - a Rust core built with the static MSVC runtime,
  - the bundled libusb,
  - the static MSVC runtime for the C++ application.

Static Qt builds are Release-only and can take 20-40 minutes the first time.
Use -Dynamic to build against the system/aqt Qt quickly.

.PARAMETER Dynamic
Use a dynamically linked Qt instead of building/using static Qt.

.PARAMETER Configuration
Debug or Release (default Release). Static mode requires Release.

.PARAMETER BuildDir
Build directory relative to the repository root. Defaults to build-static or build.

.PARAMETER QtPrefix
Static Qt installation for static mode, or CMAKE_PREFIX_PATH for -Dynamic.
Defaults to $env:QT_STATIC_PREFIX or %LOCALAPPDATA%\fmakerexplore\qt6-static.

.PARAMETER QtVersion
Qt version to build when static Qt is missing (default 6.12.0).

.PARAMETER QtWork
Directory for downloaded Qt sources and build trees.
Defaults to %LOCALAPPDATA%\fmakerexplore\qt-static.

.PARAMETER RebuildQt
Reconfigure and rebuild the static Qt tree even if the prefix already exists.

.PARAMETER Clean
Remove the build directory (and target-static in static mode) first.

.PARAMETER Test
Run CTest if tests are configured.

.PARAMETER Jobs
Parallel build jobs. Defaults to the CPU count.

.PARAMETER Package
Create dist\fmaker-explore-gui-windows-x86_64[-static].zip.

.PARAMETER Run
Start the GUI after a successful build.

.EXAMPLE
.\scripts\build-windows.ps1

.EXAMPLE
.\scripts\build-windows.ps1 -Dynamic -Configuration Release -Clean -Package

.EXAMPLE
.\scripts\build-windows.ps1 -QtPrefix C:\Qt\6.12.0\static -RebuildQt
#>
[CmdletBinding()]
param(
    [switch]$Dynamic,
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release",
    [string]$BuildDir = "",
    [string]$QtPrefix = "",
    [string]$QtVersion = "6.12.0",
    [string]$QtWork = "",
    [switch]$RebuildQt,
    [switch]$Clean,
    [switch]$Test,
    [int]$Jobs = 0,
    [switch]$Package,
    [switch]$Run
)

$ErrorActionPreference = "Stop"
if ($env:OS -ne "Windows_NT") {
    throw "scripts/build-windows.ps1 must be run on Windows"
}
$Root = Split-Path -Parent $PSScriptRoot
$staticMode = -not $Dynamic

if (-not $BuildDir) {
    $BuildDir = if ($staticMode) { "build-static" } else { "build" }
}
$BuildPath = Join-Path $Root $BuildDir
$RustTarget = if ($staticMode) { Join-Path $Root "target-static" } else { Join-Path $Root "target" }

if ($Jobs -le 0) {
    $Jobs = [Environment]::ProcessorCount
}

if ($staticMode -and $Configuration -eq "Debug") {
    throw "Static Qt builds are Release-only. Use -Dynamic -Configuration Debug for a debug build."
}
if ($QtVersion -notmatch '^[0-9]+\.[0-9]+\.[0-9]+$') {
    throw "-QtVersion must look like 6.12.0"
}

function Import-MsvcEnvironment {
    if (Get-Command cl -ErrorAction SilentlyContinue) {
        return $true
    }

    $programFilesX86 = ${env:ProgramFiles(x86)}
    if (-not $programFilesX86) {
        $programFilesX86 = $env:ProgramFiles
    }
    if (-not $programFilesX86) {
        return $false
    }

    $vswhere = Join-Path $programFilesX86 "Microsoft Visual Studio\Installer\vswhere.exe"
    if (-not (Test-Path $vswhere)) {
        return $false
    }

    $installation = & $vswhere -latest -products "*" -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (-not $installation) {
        return $false
    }
    $installation = ($installation | Select-Object -First 1).Trim()

    $vcvars = Join-Path $installation "VC\Auxiliary\Build\vcvars64.bat"
    if (-not (Test-Path $vcvars)) {
        return $false
    }

    $command = '"' + $vcvars + '" >nul 2>&1 && set'
    $environment = & cmd.exe /c $command
    foreach ($line in $environment) {
        $separator = $line.IndexOf('=')
        if ($separator -gt 0) {
            $name = $line.Substring(0, $separator)
            $value = $line.Substring($separator + 1)
            Set-Item -Path "env:$name" -Value $value
        }
    }
    return [bool](Get-Command cl -ErrorAction SilentlyContinue)
}

function Find-QtPrefix {
    $prefix = $env:QTDIR
    if (-not $prefix) {
        $prefix = $env:QT_ROOT_DIR
    }
    if (-not $prefix) {
        $qtpaths = Get-Command qtpaths6 -ErrorAction SilentlyContinue
        if ($qtpaths) {
            $output = & $qtpaths.Source --query QT_INSTALL_PREFIX
            if ($LASTEXITCODE -eq 0 -and $output) {
                $prefix = ($output | Select-Object -First 1).Trim()
            }
        }
    }
    return $prefix
}

function Ensure-Python {
    if (Get-Command python -ErrorAction SilentlyContinue) {
        return $true
    }
    $pyLauncher = Get-Command py -ErrorAction SilentlyContinue
    if (-not $pyLauncher) {
        return $false
    }

    $shimDir = Join-Path $QtWork "python-shim"
    New-Item -ItemType Directory -Force -Path $shimDir | Out-Null
    $shim = Join-Path $shimDir "python.cmd"
    Set-Content -Path $shim -Value "@echo off`r`npy %*" -Encoding Ascii
    $env:PATH = "$shimDir;$env:PATH"
    return $true
}

function Test-StaticQtInstall {
    param([string]$Prefix)

    $library = Join-Path $Prefix "lib\Qt6Widgets.lib"
    if (-not (Test-Path $library)) {
        return $false
    }
    # A shared Qt install also has Qt6Widgets.lib (an import library) and a DLL.
    if (Test-Path (Join-Path $Prefix "bin\Qt6Widgets.dll")) {
        return $false
    }
    return ((Get-Item $library).Length -gt 2MB)
}

function Build-StaticQt {
    param(
        [string]$Version,
        [string]$Prefix,
        [string]$Work,
        [int]$ParallelJobs
    )

    $parts = $Version.Split('.')
    $series = "$($parts[0]).$($parts[1])"
    $source = Join-Path $Work "qtbase-everywhere-src-$Version"
    $build = Join-Path $Work "build-$Version"
    $archive = Join-Path $Work "qtbase-everywhere-src-$Version.zip"
    $url = "https://download.qt.io/official_releases/qt/$series/$Version/submodules/qtbase-everywhere-src-$Version.zip"
    if ((Test-StaticQtInstall -Prefix $Prefix) -and -not $RebuildQt) {
        Write-Host "Using static Qt at $Prefix"
        return
    }
    if ((Test-Path (Join-Path $Prefix "lib\Qt6Widgets.lib")) -and
        (Test-Path (Join-Path $Prefix "bin\Qt6Widgets.dll"))) {
        throw "-QtPrefix points to a shared Qt installation ($Prefix). Use a dedicated directory for static Qt."
    }

    New-Item -ItemType Directory -Force -Path $Work | Out-Null
    New-Item -ItemType Directory -Force -Path $Prefix | Out-Null

    if (-not (Test-Path $source)) {
        if (-not (Test-Path $archive) -or (Get-Item $archive).Length -eq 0) {
            Write-Host "Downloading Qt $Version sources..."
            Invoke-WebRequest -Uri $url -OutFile $archive -UseBasicParsing
        }
        Write-Host "Extracting Qt sources..."
        Expand-Archive -Path $archive -DestinationPath $Work -Force
    }

    if ($RebuildQt -and (Test-Path $build)) {
        Remove-Item -Recurse -Force $build
    }
    New-Item -ItemType Directory -Force -Path $build | Out-Null

    Push-Location $build
    try {
        Write-Host "Configuring static Qt $Version..."
        & (Join-Path $source "configure.bat") `
            -static `
            -release `
            -opensource `
            -confirm-license `
            -nomake examples `
            -nomake tests `
            -prefix $Prefix `
            -qt-zlib `
            -qt-libpng `
            -qt-libjpeg `
            -qt-freetype `
            -qt-harfbuzz `
            -qt-pcre `
            -static-runtime `
            -no-feature-textmarkdownreader
        if ($LASTEXITCODE -ne 0) {
            throw "Qt configure failed with exit code $LASTEXITCODE"
        }

        Write-Host "Building static Qt ($ParallelJobs jobs)..."
        & cmake --build . --config Release --parallel $ParallelJobs
        if ($LASTEXITCODE -ne 0) {
            throw "Qt build failed with exit code $LASTEXITCODE"
        }

        Write-Host "Installing static Qt into $Prefix..."
        & cmake --install . --config Release
        if ($LASTEXITCODE -ne 0) {
            throw "Qt install failed with exit code $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
    }

    if (-not (Test-StaticQtInstall -Prefix $Prefix)) {
        throw "Static Qt installation did not produce a static lib\Qt6Widgets.lib in $Prefix"
    }
}

foreach ($tool in @("cmake", "cargo", "rustc")) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        Write-Host "error: $tool was not found in PATH" -ForegroundColor Red
        Write-Host "Install CMake and the Rust MSVC toolchain, then reopen the terminal." -ForegroundColor Yellow
        exit 1
    }
}

$msvcReady = Import-MsvcEnvironment

if ($staticMode) {
    if (-not $QtPrefix) {
        $QtPrefix = $env:QT_STATIC_PREFIX
    }
    if (-not $QtPrefix) {
        $base = if ($env:LOCALAPPDATA) { $env:LOCALAPPDATA } else { $env:USERPROFILE }
        $QtPrefix = Join-Path $base "fmakerexplore\qt6-static"
    }
    if (-not $QtWork) {
        $QtWork = $env:QT_STATIC_WORK
    }
    if (-not $QtWork) {
        $base = if ($env:LOCALAPPDATA) { $env:LOCALAPPDATA } else { $env:USERPROFILE }
        $QtWork = Join-Path $base "fmakerexplore\qt-static"
    }

    if (-not $msvcReady) {
        Write-Host "error: the MSVC developer environment was not found." -ForegroundColor Red
        Write-Host "Run this script from an 'x64 Native Tools Command Prompt for VS', or install Visual Studio C++ tools." -ForegroundColor Yellow
        exit 1
    }
    if (-not (Get-Command ninja -ErrorAction SilentlyContinue)) {
        throw "Ninja is required for static builds. Install it with: choco install ninja"
    }
    if (-not (Ensure-Python)) {
        throw "Python is required to configure static Qt. Install Python and ensure 'python' or 'py' is in PATH."
    }

    if ($Clean) {
        if (Test-Path $BuildPath) { Remove-Item -Recurse -Force $BuildPath }
        if (Test-Path $RustTarget) { Remove-Item -Recurse -Force $RustTarget }
    }

    Build-StaticQt -Version $QtVersion -Prefix $QtPrefix -Work $QtWork -ParallelJobs $Jobs

    if (-not $env:RUSTFLAGS) {
        $env:RUSTFLAGS = "-C target-feature=+crt-static"
    } elseif ($env:RUSTFLAGS -notmatch "crt-static") {
        $env:RUSTFLAGS = "$($env:RUSTFLAGS) -C target-feature=+crt-static"
    }

    $cmakeArgs = @(
        "-S", $Root,
        "-B", $BuildPath,
        "-G", "Ninja",
        "-DCMAKE_BUILD_TYPE=Release",
        "-DCMAKE_PREFIX_PATH=$QtPrefix",
        "-DFM_STATIC_QT=ON",
        "-DFM_STATIC_RUNTIME=ON",
        "-DFM_LIBUSB_UDEV=OFF",
        "-DFM_RUST_TARGET_DIR=$RustTarget"
    )

    & cmake @cmakeArgs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    & cmake --build $BuildPath --parallel $Jobs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} else {
    if (-not $QtPrefix) {
        $QtPrefix = Find-QtPrefix
    }

    if ($Clean -and (Test-Path $BuildPath)) {
        Remove-Item -Recurse -Force $BuildPath
    }

    $ninja = Get-Command ninja -ErrorAction SilentlyContinue
    $cl = Get-Command cl -ErrorAction SilentlyContinue
    $useNinja = ($null -ne $ninja) -and ($null -ne $cl)

    $cmakeArgs = @(
        "-S", $Root,
        "-B", $BuildPath,
        "-DCMAKE_BUILD_TYPE=$Configuration",
        "-DFM_RUST_TARGET_DIR=$RustTarget"
    )
    if ($QtPrefix) {
        $cmakeArgs += "-DCMAKE_PREFIX_PATH=$QtPrefix"
    }

    if ($useNinja) {
        $cmakeArgs = @("-G", "Ninja") + $cmakeArgs
    } else {
        Write-Host "Ninja or the MSVC developer environment was not detected; prebuilding the Rust library."
        $cargoArgs = @("build")
        if ($Configuration -ne "Debug") {
            $cargoArgs += "--release"
        }
        & cargo @cargoArgs
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

        $hostInfo = & rustc -vV
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
        $hostLine = ($hostInfo | Select-String -Pattern "^host:").Line
        if (-not $hostLine) {
            throw "Could not determine the Rust host triple from rustc -vV."
        }
        $profileDir = if ($Configuration -eq "Debug") { "debug" } else { "release" }
        $libName = if ($hostLine -match "msvc") { "fmaker_explore.lib" } else { "libfmaker_explore.a" }
        $libPath = Join-Path $Root "target\$profileDir\$libName"
        if (-not (Test-Path $libPath)) {
            throw "Rust static library was not produced at $libPath"
        }
        $cmakeArgs += "-DFM_RUST_LIB=$libPath"
    }

    & cmake @cmakeArgs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    & cmake --build $BuildPath --parallel $Jobs --config $Configuration
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

$candidates = @(
    (Join-Path $BuildPath "fmaker-explore-gui.exe"),
    (Join-Path $BuildPath "$Configuration\fmaker-explore-gui.exe")
)
$exe = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $exe) {
    $exe = Get-ChildItem -Path $BuildPath -Filter "fmaker-explore-gui.exe" -Recurse -ErrorAction SilentlyContinue |
        Select-Object -First 1 -ExpandProperty FullName
}
if (-not $exe) {
    throw "Could not locate fmaker-explore-gui.exe in $BuildPath"
}
Write-Host "Built: $exe"

if ($Test) {
    if (Test-Path (Join-Path $BuildPath "CTestTestfile.cmake")) {
        & ctest --test-dir $BuildPath --output-on-failure
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    } else {
        Write-Host "No CTest tests are configured on Windows."
    }
}

if ($Package) {
    $dist = Join-Path $Root "dist"
    $stageName = if ($staticMode) { "windows-x86_64-static" } else { "windows-x86_64" }
    $stage = Join-Path $dist $stageName
    if (Test-Path $stage) {
        Remove-Item -Recurse -Force $stage
    }
    New-Item -ItemType Directory -Force -Path $stage | Out-Null
    Copy-Item -Path $exe -Destination $stage
    Copy-Item -Path (Join-Path $Root "README.md") -Destination $stage

    if (-not $staticMode) {
        $deployPath = $null
        $deploy = Get-Command windeployqt -ErrorAction SilentlyContinue
        if ($deploy) {
            $deployPath = $deploy.Source
        }
        if (-not $deployPath -and $QtPrefix) {
            $candidate = Join-Path $QtPrefix "bin\windeployqt.exe"
            if (Test-Path $candidate) {
                $deployPath = $candidate
            }
        }
        if (-not $deployPath) {
            throw "windeployqt was not found. Add the Qt bin directory to PATH or set QTDIR/QT_ROOT_DIR."
        }

        $deployMode = if ($Configuration -eq "Debug") { "--debug" } else { "--release" }
        & $deployPath $deployMode --no-translations (Join-Path $stage "fmaker-explore-gui.exe")
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }

    $zipName = if ($staticMode) {
        "fmaker-explore-gui-windows-x86_64-static.zip"
    } else {
        "fmaker-explore-gui-windows-x86_64.zip"
    }
    $zip = Join-Path $dist $zipName
    if (Test-Path $zip) {
        Remove-Item -Force $zip
    }
    Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $zip
    Write-Host "Packaged: $zip"
}

if ($Run) {
    & $exe
}
