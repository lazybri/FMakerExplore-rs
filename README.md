# Fuji Mitsu maker Explore

FMakerExplore is a filesystem utility for the Fuji and Mitsu MakerCmd interface. It connects to the handset over USB, enters Maker Mode when necessary, and provides access to files and directories on the phone. The tool is implemented in Rust and talks to the device through [libusb](https://libusb.info/) via [rusb](https://crates.io/crates/rusb).

## Features

- Connect to and probe the MakerCmd interface.
- List available drive letters and directory contents.
- Display file size, attributes, and timestamp data.
- Download files with checksum validation and resume support.
- Upload files with checksum validation and automatic retry handling.
- Create directories, delete files, and change the read-only attribute.
- Display the available space on the `D:` drive.

## Requirements

- Rust 1.85 or newer
- libusb 1.0 development files
- USB access to the phone (on Linux, install the usual udev rule or run with appropriate permissions)

Install the libusb development package for your platform, for example:

```bash
# Debian/Ubuntu
sudo apt install libusb-1.0-0-dev
# Fedora
sudo dnf install libusb1-devel
# macOS
brew install libusb
```

## Build

```bash
cargo build --release
```

The binary is written to `target/release/fmaker-explore`. You can also install it into your Cargo bin directory:

```bash
cargo install --path .
```

Run the test suite with:

```bash
cargo test
```

## Usage

The examples below assume `fmaker-explore` is on your `PATH`. From a checkout you can use `cargo run --release --` instead, or invoke the built binary directly.

Connect to the phone and enter Maker Mode:

```bash
fmaker-explore --connect
```

List the available drives:

```bash
fmaker-explore --listpartition
```

List a remote directory:

```bash
fmaker-explore --listdir 'D:\WcdmaMp\'
```

Display remote file information:

```bash
fmaker-explore --listfile 'D:\WcdmaMp\file.bin'
```

Download a file:

```bash
fmaker-explore --pull 'D:\WcdmaMp\file.bin' ./file.bin
```

Use `--overwrite` to replace an existing local file, or `--no-resume` to discard an existing partial download.

Upload a file:

```bash
fmaker-explore --push ./file.bin 'D:\WcdmaMp\file.bin'
```

The upload command refuses to replace an existing remote file.

Create a remote directory:

```bash
fmaker-explore --mkdir 'D:\WcdmaMp\newdir'
```

Delete a remote file:

```bash
fmaker-explore --delete 'D:\WcdmaMp\file.bin'
```

Enable or disable the read-only attribute:

```bash
fmaker-explore --setreadonly 'D:\WcdmaMp\file.bin' on
fmaker-explore --setreadonly 'D:\WcdmaMp\file.bin' off
```

Display the available space on `D:`:

```bash
fmaker-explore --checkdavailable
```

Use `--help` to display all available options. Remote paths must be absolute, use a drive letter, and contain only ASCII characters.

### Options

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
2. Run the required FMakerExplore command.
3. The tool checks whether Maker Mode is already active.
4. If necessary, it switches the USB interface and waits for Maker Mode to become available.
5. The requested filesystem operation is then performed.

The default USB device ID is `06d3:21b0`. Use `--vid` and `--pid` to select another compatible device.

## Project layout

- `src/protocol.rs` — MakerCmd framing, path validation, and shared helpers.
- `src/client.rs` — USB transport, Maker Mode entry, and filesystem operations.
- `src/cli.rs` — command-line interface and action dispatch.
- `src/error.rs` — error types shared across the crate.
