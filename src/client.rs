use std::env;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use rusb::{DeviceHandle, UsbContext};
use serde::Serialize;
use serde_json::json;

use crate::error::Error;
use crate::protocol::{
    EP_CONTROL_REPLY, EP_IN, EP_OUT, FrameReader, READ_SIZE, STATUS_CHECKSUM_ERROR,
    STATUS_FILE_ERROR, STATUS_FS_ERROR, STATUS_INFO, STATUS_LAST, STATUS_MORE, STATUS_NO_SPACE,
    STATUS_OK, STATUS_UPLOAD_MORE, UPLOAD_CHUNK_SIZE, checksum, format_size, hex_compact,
    hex_spaced, mask_packet, normalize_remote_path, unmask_packet,
};

const PROBE_COMMAND: &[u8] = b"\xC6\x13\x00z:\\905ifsaccess_probe";
const PROBE_TIMEOUT: Duration = Duration::from_millis(1000);
const ENTER_PACKET: &[u8] = b"\xFF\x56\x55\x42\x00\x03\xC1\x01\x00\xFE";

#[derive(Debug, Serialize)]
pub struct FileInfo {
    pub path: String,
    pub size: u64,
    pub attributes_raw: u8,
    pub attributes: Vec<&'static str>,
    pub year: u16,
    pub timestamp_raw: String,
}

pub type ProgressHandler = Box<dyn FnMut(u64, Option<u64>) + Send>;

pub struct MakerCmdClient {
    vid: u16,
    pid: u16,
    timeout: Duration,
    retries: u32,
    maker_wait: Duration,
    verbose: bool,
    handle: Option<DeviceHandle<rusb::Context>>,
    claimed_interfaces: Vec<u8>,
    reader: FrameReader,
    progress: Option<ProgressHandler>,
}

impl MakerCmdClient {
    pub fn new(
        vid: u16,
        pid: u16,
        timeout: Duration,
        retries: u32,
        maker_wait: Duration,
        verbose: bool,
    ) -> Self {
        Self {
            vid,
            pid,
            timeout: timeout.max(Duration::from_millis(1)),
            retries,
            maker_wait: maker_wait.max(Duration::from_secs(1)),
            verbose,
            handle: None,
            claimed_interfaces: Vec::new(),
            reader: FrameReader::new(),
            progress: None,
        }
    }

    pub fn set_progress_handler(&mut self, handler: Option<ProgressHandler>) {
        self.progress = handler;
    }

    fn report_progress(&mut self, current: u64, total: Option<u64>) {
        if let Some(handler) = self.progress.as_mut() {
            handler(current, total);
            return;
        }
        match total {
            Some(total) => eprint!("\rSent {} / {}", format_size(current), format_size(total)),
            None => eprint!("\rReceived {}", format_size(current)),
        }
        let _ = io::stderr().flush();
    }

    fn end_progress_line(&self) {
        if self.progress.is_none() {
            eprintln!();
        }
    }

    pub fn log(&self, message: impl AsRef<str>) {
        if self.verbose {
            eprintln!("[debug] {}", message.as_ref());
        }
    }

    fn transport(description: &str, error: rusb::Error) -> Error {
        Error::Transport(format!("USB {description} failed: {error}"))
    }

    fn require_handle(&self) -> Result<&DeviceHandle<rusb::Context>, Error> {
        self.handle
            .as_ref()
            .ok_or_else(|| Error::Device("USB device is not connected".to_string()))
    }

    pub fn connect(&mut self) -> Result<(), Error> {
        let context = rusb::Context::new()
            .map_err(|error| Error::Device(format!("USB discovery failed: {error}")))?;
        let devices = context
            .devices()
            .map_err(|error| Error::Device(format!("USB discovery failed: {error}")))?;

        let mut found = None;
        for device in devices.iter() {
            let Ok(descriptor) = device.device_descriptor() else {
                continue;
            };
            if descriptor.vendor_id() == self.vid && descriptor.product_id() == self.pid {
                found = Some(device);
                break;
            }
        }
        let device = found.ok_or_else(|| {
            Error::Device(format!(
                "D905i not found (VID=0x{:04x}, PID=0x{:04x}). \
                 Connect the phone and check USB permissions.",
                self.vid, self.pid
            ))
        })?;

        let handle = device
            .open()
            .map_err(|error| Error::Device(format!("cannot open D905i: {error}")))?;

        match handle.active_configuration() {
            Ok(configuration) => {
                self.log(format!("USB configuration {configuration} is active"));
            }
            Err(error) => {
                self.log(format!("USB configuration is not active: {error}"));
                let configuration = handle
                    .device()
                    .config_descriptor(0)
                    .map_err(|error| {
                        Error::Device(format!("cannot inspect USB configuration: {error}"))
                    })?
                    .number();
                handle
                    .set_active_configuration(configuration)
                    .map_err(|error| {
                        Error::Device(format!("cannot activate D905i USB configuration: {error}"))
                    })?;
                let active = handle.active_configuration().map_err(|error| {
                    Error::Device(format!("cannot inspect USB configuration: {error}"))
                })?;
                self.log(format!("activated USB configuration {active}"));
            }
        }

        self.handle = Some(handle);
        self.claimed_interfaces.clear();
        self.claim_interfaces();
        self.reader.clear();
        self.log("USB device found");

        match self.request(PROBE_COMMAND, Some(PROBE_TIMEOUT)) {
            Ok(response) if response.starts_with(&[0xC6, 0x13, 0xA1]) => {
                self.log("phone is already in Maker Mode");
                return Ok(());
            }
            Ok(_) => {}
            Err(_) => self.reader.clear(),
        }

        self.enter_maker_mode()
    }

    fn claim_interfaces(&mut self) {
        let Some(handle) = self.handle.as_mut() else {
            return;
        };

        let mut numbers = Vec::new();
        if let Ok(configuration) = handle.device().active_config_descriptor() {
            for interface in configuration.interfaces() {
                for descriptor in interface.descriptors() {
                    let has_endpoint = descriptor.endpoint_descriptors().any(|endpoint| {
                        matches!(endpoint.address(), EP_OUT | EP_IN | EP_CONTROL_REPLY)
                    });
                    let number = descriptor.interface_number();
                    if has_endpoint && !numbers.contains(&number) {
                        numbers.push(number);
                    }
                }
            }
        }

        let mut claimed = Vec::new();
        let mut notes = Vec::new();
        for number in numbers {
            match handle.claim_interface(number) {
                Ok(()) => {
                    claimed.push(number);
                    notes.push(format!("claimed USB interface {number}"));
                }
                Err(rusb::Error::Busy) => {
                    #[cfg(target_os = "linux")]
                    {
                        let detached = handle.set_auto_detach_kernel_driver(true).is_ok();
                        if detached && handle.claim_interface(number).is_ok() {
                            claimed.push(number);
                            notes.push(format!(
                                "claimed USB interface {number} after detaching the kernel driver"
                            ));
                            continue;
                        }
                    }
                    notes.push(format!("USB interface {number} is busy"));
                }
                Err(error) => {
                    notes.push(format!("cannot claim USB interface {number}: {error}"));
                }
            }
        }

        self.claimed_interfaces = claimed;
        for note in notes {
            self.log(note);
        }
    }

    pub fn close(&mut self) {
        if let Some(handle) = self.handle.take() {
            for number in std::mem::take(&mut self.claimed_interfaces) {
                let _ = handle.release_interface(number);
            }
        }
        self.claimed_interfaces.clear();
        self.reader.clear();
    }

    pub fn reconnect(&mut self) -> Result<(), Error> {
        self.close();
        thread::sleep(Duration::from_millis(250));
        self.connect()
    }

    fn drain_control_reply(&mut self) -> Result<(), Error> {
        let mut buffer = [0u8; 256];
        match self
            .require_handle()?
            .read_bulk(EP_CONTROL_REPLY, &mut buffer, self.timeout)
        {
            Ok(_) => Ok(()),
            Err(error @ rusb::Error::Timeout) => {
                self.log(format!("USB control reply read failed: {error}"));
                Ok(())
            }
            Err(error) => Err(Self::transport("control reply read", error)),
        }
    }

    fn enter_maker_mode(&mut self) -> Result<(), Error> {
        eprintln!("Entering Maker Mode...");
        self.require_handle()?
            .write_control(0x41, 0x62, 0x00, 0x00, &[0x02, 0xC0], self.timeout)
            .map_err(|error| Self::transport("mode capability request", error))?;
        self.drain_control_reply()?;
        self.require_handle()?
            .write_control(0x41, 0x60, 0xC0, 0x00, &[], self.timeout)
            .map_err(|error| Self::transport("endpoint mode switch", error))?;
        self.drain_control_reply()?;

        self.write_raw(ENTER_PACKET)?;
        let mut buffer = [0u8; 256];
        match self
            .require_handle()?
            .read_bulk(EP_IN, &mut buffer, self.timeout)
        {
            Ok(_) => {}
            Err(error @ rusb::Error::Timeout) => {
                self.log(format!("mode-entry acknowledgement timed out: {error}"));
            }
            Err(error) => return Err(Self::transport("mode-entry acknowledgement read", error)),
        }

        let deadline = Instant::now() + self.maker_wait;
        let mut last_error: Option<Error> = None;
        while Instant::now() < deadline {
            match self.request(PROBE_COMMAND, Some(PROBE_TIMEOUT)) {
                Ok(response) if response.starts_with(&[0xC6, 0x13, 0xA1]) => {
                    eprintln!("Maker Mode connected.");
                    return Ok(());
                }
                Ok(response) => {
                    last_error = Some(Error::Protocol(format!(
                        "Maker Mode probe returned unexpected payload: {}",
                        hex_spaced(&response)
                    )));
                }
                Err(error) => last_error = Some(error),
            }
            thread::sleep(Duration::from_secs(1));
        }

        let detail = last_error
            .map(|error| format!(": {error}"))
            .unwrap_or_default();
        Err(Error::Device(format!(
            "phone did not enter Maker Mode within {:.0}s{detail}",
            self.maker_wait.as_secs_f64()
        )))
    }

    fn write_raw(&mut self, data: &[u8]) -> Result<(), Error> {
        let written = self
            .require_handle()?
            .write_bulk(EP_OUT, data, self.timeout)
            .map_err(|error| Self::transport("write", error))?;
        if written != data.len() {
            return Err(Error::Transport(format!(
                "short USB write: {written}/{} bytes",
                data.len()
            )));
        }
        Ok(())
    }

    fn read_frame(&mut self, timeout: Option<Duration>) -> Result<Vec<u8>, Error> {
        let timeout = timeout.unwrap_or(self.timeout);
        let deadline = Instant::now() + timeout;

        loop {
            if let Some(frame) = self.reader.feed(&[]) {
                return unmask_packet(&frame);
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(Error::Transport(
                    "timed out waiting for MakerCmd response".to_string(),
                ));
            }
            let remaining = deadline - now;
            let mut buffer = [0u8; READ_SIZE];
            let read = match self
                .require_handle()?
                .read_bulk(EP_IN, &mut buffer, remaining)
            {
                Ok(read) => read,
                Err(rusb::Error::Timeout) => {
                    return Err(Error::Transport(
                        "timed out waiting for MakerCmd response".to_string(),
                    ));
                }
                Err(error) => return Err(Self::transport("read", error)),
            };
            if let Some(frame) = self.reader.feed(&buffer[..read]) {
                return unmask_packet(&frame);
            }
        }
    }

    pub fn request(&mut self, payload: &[u8], timeout: Option<Duration>) -> Result<Vec<u8>, Error> {
        if payload.len() > 0xFFFF {
            return Err(Error::Protocol(
                "payload is too large for MakerCmd".to_string(),
            ));
        }
        let mut packet = Vec::with_capacity(5 + payload.len());
        packet.extend_from_slice(crate::protocol::REQUEST_MAGIC);
        packet.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        packet.extend_from_slice(payload);
        self.log(format!("> {}", hex_spaced(payload)));
        self.write_raw(&mask_packet(&packet))?;

        let raw = self.read_frame(timeout)?;
        if raw.len() < 5
            || (raw[..3] != *crate::protocol::RESPONSE_MAGIC
                && raw[..3] != *crate::protocol::REQUEST_MAGIC)
        {
            return Err(Error::Protocol(format!(
                "unexpected response header: {}",
                hex_spaced(&raw[..raw.len().min(5)])
            )));
        }
        let declared = u16::from_be_bytes([raw[3], raw[4]]) as usize;
        if raw.len() < 5 + declared {
            return Err(Error::Protocol(format!(
                "truncated response: declared {declared}, received {}",
                raw.len() - 5
            )));
        }
        let response = raw[5..5 + declared].to_vec();
        self.log(format!("< {}", hex_spaced(&response)));
        Ok(response)
    }

    fn expect_group(response: &[u8], group: &[u8]) -> Result<(), Error> {
        if response.len() < 3 || response[..2] != *group {
            return Err(Error::Protocol(format!(
                "unexpected MakerCmd response: {}",
                hex_spaced(response)
            )));
        }
        Ok(())
    }

    fn remote_error(response: &[u8], operation: &str) -> Error {
        let status = response.get(2).copied();
        let os_error = if response.len() >= 7
            && matches!(status, Some(STATUS_FILE_ERROR) | Some(STATUS_FS_ERROR))
        {
            Some(i32::from_be_bytes([
                response[3],
                response[4],
                response[5],
                response[6],
            ]))
        } else {
            None
        };
        let suffix = os_error
            .map(|value| format!(", Symbian error {value}"))
            .unwrap_or_default();
        let status_text = status
            .map(|value| format!("0x{value:02x}"))
            .unwrap_or_else(|| "missing".to_string());
        Error::remote(
            format!("{operation} failed: remote status {status_text}{suffix}"),
            status,
            os_error,
        )
    }

    fn query_with_reconnect(&mut self, payload: &[u8], operation: &str) -> Result<Vec<u8>, Error> {
        let mut last_error: Option<Error> = None;
        for attempt in 0..=self.retries {
            match self.request(payload, None) {
                Ok(response) => return Ok(response),
                Err(Error::Transport(message)) => {
                    let error = Error::Transport(message);
                    if attempt >= self.retries {
                        last_error = Some(error);
                        break;
                    }
                    eprintln!(
                        "{operation}: connection lost, reconnecting ({}/{})...",
                        attempt + 1,
                        self.retries
                    );
                    self.reconnect()?;
                    last_error = Some(error);
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_error
            .unwrap_or_else(|| Error::Transport(format!("{operation} failed without a response"))))
    }

    pub fn check_d_available(&mut self) -> Result<u64, Error> {
        let response = self.query_with_reconnect(b"\xC6\x13\x10", "space query")?;
        Self::expect_group(&response, b"\xC6\x13")?;
        if response[2] != STATUS_INFO || response.len() < 8 {
            return Err(Self::remote_error(&response, "space query"));
        }
        Ok(u32::from_be_bytes([response[4], response[5], response[6], response[7]]) as u64)
    }

    pub fn list_partitions(&mut self) -> Result<Vec<String>, Error> {
        let response = self.query_with_reconnect(b"\xC6\x13\x30", "partition list")?;
        if response.len() < 2 || response[..2] != [0xC6, 0x13] {
            return Err(Error::Protocol(format!(
                "unexpected partition response: {}",
                hex_spaced(&response)
            )));
        }
        let mut partitions = Vec::new();
        for &value in &response[2..] {
            let character = value as char;
            if character.is_alphabetic() {
                partitions.push(format!("{}:", character.to_uppercase()));
            }
        }
        Ok(partitions)
    }

    pub fn list_directory(&mut self, remote_path: &str) -> Result<Vec<String>, Error> {
        let mut path = normalize_remote_path(remote_path)?;
        if !path.ends_with('\\') {
            path.push('\\');
        }
        let mut initial = vec![0xC6, 0x13, 0x31];
        initial.extend_from_slice(path.as_bytes());

        let mut last_error: Option<Error> = None;
        for attempt in 0..=self.retries {
            match self.list_directory_once(&initial, &path) {
                Ok(entries) => return Ok(entries),
                Err(Error::Transport(message)) => {
                    let error = Error::Transport(message);
                    if attempt >= self.retries {
                        last_error = Some(error);
                        break;
                    }
                    eprintln!("Directory transfer interrupted; restarting it...");
                    last_error = Some(error);
                    self.reconnect()?;
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_error.unwrap_or_else(|| {
            Error::Transport("directory transfer failed without a response".to_string())
        }))
    }

    fn list_directory_once(&mut self, initial: &[u8], path: &str) -> Result<Vec<String>, Error> {
        let mut response = self.request(initial, None)?;
        let mut output = Vec::new();
        loop {
            Self::expect_group(&response, b"\xC6\x13")?;
            let status = response[2];
            if matches!(status, STATUS_FILE_ERROR | STATUS_FS_ERROR) {
                return Err(Self::remote_error(
                    &response,
                    &format!("list directory {path}"),
                ));
            }
            if !matches!(status, STATUS_MORE | STATUS_LAST) {
                return Err(Error::Protocol(format!(
                    "unexpected directory status 0x{status:02x}"
                )));
            }
            output.extend_from_slice(&response[3..]);
            if status == STATUS_LAST {
                let (text, _, _) = encoding_rs::SHIFT_JIS.decode(&output);
                return Ok(text
                    .split(',')
                    .filter(|entry| !entry.is_empty())
                    .map(str::to_owned)
                    .collect());
            }
            response = self.request(&[0xC6, 0x13, 0x20], None)?;
        }
    }

    pub fn list_file(&mut self, remote_path: &str) -> Result<FileInfo, Error> {
        let path = normalize_remote_path(remote_path)?;
        let mut payload = vec![0xC6, 0x13, 0x32];
        payload.extend_from_slice(path.as_bytes());
        let response = self.query_with_reconnect(&payload, "file information")?;
        Self::expect_group(&response, b"\xC6\x13")?;
        if response[2] == STATUS_FS_ERROR {
            return Err(Self::remote_error(&response, &format!("stat {path}")));
        }
        if response.len() < 10 {
            return Err(Error::Protocol(format!(
                "short file information response: {}",
                hex_spaced(&response)
            )));
        }

        let attributes_raw = response[2];
        let mut attributes = Vec::new();
        for (bit, name) in [
            (0x01, "archive"),
            (0x02, "hidden"),
            (0x04, "readonly"),
            (0x08, "system"),
        ] {
            if attributes_raw & bit != 0 {
                attributes.push(name);
            }
        }

        Ok(FileInfo {
            path,
            size: u32::from_be_bytes([response[3], response[4], response[5], response[6]]) as u64,
            attributes_raw,
            attributes,
            year: u16::from_be_bytes([response[7], response[8]]),
            timestamp_raw: hex_compact(&response[7..]),
        })
    }

    pub fn set_readonly(&mut self, remote_path: &str, enabled: bool) -> Result<(), Error> {
        let path = normalize_remote_path(remote_path)?;
        let mut payload = vec![0xC6, 0x13, 0x33, u8::from(enabled)];
        payload.extend_from_slice(path.as_bytes());
        let response = self.query_with_reconnect(&payload, "set readonly")?;
        Self::expect_group(&response, b"\xC6\x13")?;
        if response[2] != STATUS_OK {
            return Err(Self::remote_error(
                &response,
                &format!("set readonly on {path}"),
            ));
        }
        Ok(())
    }

    pub fn mkdir(&mut self, remote_path: &str) -> Result<(), Error> {
        let path = normalize_remote_path(remote_path)?;
        let mut payload = vec![0xC6, 0x14, 0x83, 0x00, 0x00, 0x00, 0x00, 0x00];
        payload.extend_from_slice(path.as_bytes());
        let response = self.query_with_reconnect(&payload, "mkdir")?;
        Self::expect_group(&response, b"\xC6\x14")?;
        if response[2] != STATUS_OK {
            return Err(Self::remote_error(&response, &format!("mkdir {path}")));
        }
        Ok(())
    }

    pub fn delete_file(&mut self, remote_path: &str) -> Result<(), Error> {
        let path = normalize_remote_path(remote_path)?;
        let mut payload = vec![0xC6, 0x14, 0x81, 0x00, 0x00, 0x00, 0x00, 0x00];
        payload.extend_from_slice(path.as_bytes());
        let response = self.query_with_reconnect(&payload, "delete file")?;
        Self::expect_group(&response, b"\xC6\x14")?;
        if response[2] != STATUS_OK {
            return Err(Self::remote_error(&response, &format!("delete {path}")));
        }
        Ok(())
    }

    fn validate_download_chunk(response: &[u8], operation: &str) -> Result<(u8, Vec<u8>), Error> {
        Self::expect_group(response, b"\xC6\x13")?;
        let status = response[2];
        if status == STATUS_FILE_ERROR {
            return Err(Self::remote_error(response, operation));
        }
        if !matches!(status, STATUS_MORE | STATUS_LAST) {
            return Err(Error::Protocol(format!(
                "unexpected download status 0x{status:02x}"
            )));
        }
        if response.len() < 4 {
            return Err(Error::Protocol(
                "download response has no checksum".to_string(),
            ));
        }
        let data = &response[4..];
        let expected = response[3];
        let actual = checksum(data);
        if expected != actual {
            return Err(Error::Checksum {
                status,
                message: format!(
                    "download checksum mismatch: expected 0x{expected:02x}, got 0x{actual:02x}"
                ),
            });
        }
        Ok((status, data.to_vec()))
    }

    fn merge_resumed_chunk(
        handle: &mut File,
        data: &[u8],
        verified: u64,
        old_size: &mut u64,
    ) -> Result<u64, Error> {
        let mut overlap = (*old_size).saturating_sub(verified).min(data.len() as u64) as usize;
        if overlap > 0 {
            handle.seek(SeekFrom::Start(verified))?;
            let mut existing = vec![0u8; overlap];
            let read = handle.read(&mut existing)?;
            if read != overlap || existing != data[..overlap] {
                handle.set_len(verified)?;
                *old_size = verified;
                overlap = 0;
            }
        }
        if overlap < data.len() {
            handle.seek(SeekFrom::Start(verified + overlap as u64))?;
            handle.write_all(&data[overlap..])?;
        }
        Ok(verified + data.len() as u64)
    }

    fn pull_once(
        &mut self,
        remote: &str,
        part_path: &Path,
        old_size: &mut u64,
    ) -> Result<u64, Error> {
        let mut handle = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(part_path)?;
        let mut initial = vec![0xC6, 0x13, 0x00];
        initial.extend_from_slice(remote.as_bytes());

        let mut response = self.request(&initial, None)?;
        let mut verified = 0u64;
        loop {
            let mut checksum_retries = 0u32;
            let (status, data) = loop {
                match Self::validate_download_chunk(&response, &format!("pull {remote}")) {
                    Ok(chunk) => break chunk,
                    Err(Error::Checksum { status, message }) => {
                        if status == STATUS_LAST || checksum_retries >= self.retries {
                            return Err(Error::TransferRestart(message));
                        }
                        checksum_retries += 1;
                        response = self.request(&[0xC6, 0x13, 0x21], None)?;
                    }
                    Err(error) => return Err(error),
                }
            };

            verified = Self::merge_resumed_chunk(&mut handle, &data, verified, old_size)?;
            self.report_progress(verified, None);
            if status == STATUS_LAST {
                handle.set_len(verified)?;
                handle.flush()?;
                handle.sync_all()?;
                break;
            }
            response = self.request(&[0xC6, 0x13, 0x20], None)?;
        }
        Ok(verified)
    }

    pub fn pull(
        &mut self,
        remote_path: &str,
        local_path: &Path,
        resume: bool,
        overwrite: bool,
    ) -> Result<u64, Error> {
        let remote = normalize_remote_path(remote_path)?;
        let local_path = expand_user(local_path);
        let part_path = sibling(&local_path, ".part");
        let meta_path = sibling(&local_path, ".part.json");

        if local_path.exists() && !overwrite {
            return Err(Error::Fs(format!(
                "local file already exists: {}; pass --overwrite to replace it",
                local_path.display()
            )));
        }
        if let Some(parent) = local_path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }

        if !resume {
            for stale in [&part_path, &meta_path] {
                match fs::remove_file(stale) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
        } else if meta_path.exists() {
            let meta = fs::read_to_string(&meta_path)
                .ok()
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .unwrap_or(serde_json::Value::Null);
            let recorded = meta.get("remote").and_then(|value| value.as_str());
            if recorded != Some(remote.as_str()) {
                return Err(Error::Fs(format!(
                    "partial file belongs to {}, not {}: {}",
                    recorded_repr(recorded),
                    repr(&remote),
                    part_path.display()
                )));
            }
        }

        fs::write(
            &meta_path,
            serde_json::to_vec(&json!({ "remote": remote }))?,
        )?;

        let mut last_error: Option<Error> = None;
        for attempt in 0..=self.retries {
            let mut old_size = if resume && part_path.exists() {
                fs::metadata(&part_path)?.len()
            } else {
                0
            };
            match self.pull_once(&remote, &part_path, &mut old_size) {
                Ok(verified) => {
                    self.end_progress_line();
                    replace_file(&part_path, &local_path)?;
                    let _ = fs::remove_file(&meta_path);
                    return Ok(verified);
                }
                Err(Error::Transport(message)) => {
                    self.end_progress_line();
                    let error = Error::Transport(message);
                    if attempt >= self.retries {
                        last_error = Some(error);
                        break;
                    }
                    eprintln!(
                        "Download interrupted; reconnecting and validating the partial file \
                         ({}/{})...",
                        attempt + 1,
                        self.retries
                    );
                    last_error = Some(error);
                    self.reconnect()?;
                }
                Err(Error::TransferRestart(message)) => {
                    self.end_progress_line();
                    let error = Error::TransferRestart(message);
                    if attempt >= self.retries {
                        last_error = Some(error);
                        break;
                    }
                    eprintln!(
                        "Final block was corrupt; restarting and validating the partial file \
                         ({}/{})...",
                        attempt + 1,
                        self.retries
                    );
                    last_error = Some(error);
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_error
            .unwrap_or_else(|| Error::Transport("download failed without a response".to_string())))
    }

    fn begin_push(&mut self, remote: &str, size: u64) -> Result<(), Error> {
        let mut payload = vec![0xC6, 0x14, 0x80, 0x00];
        payload.extend_from_slice(&(size as u32).to_be_bytes());
        payload.extend_from_slice(remote.as_bytes());
        let response = self.request(&payload, None)?;
        Self::expect_group(&response, b"\xC6\x14")?;
        if response[2] == STATUS_NO_SPACE {
            return Err(Error::remote(
                "push failed: insufficient free space on D:",
                Some(STATUS_NO_SPACE),
                None,
            ));
        }
        if response[2] != STATUS_UPLOAD_MORE {
            return Err(Self::remote_error(
                &response,
                &format!("open {remote} for upload"),
            ));
        }
        Ok(())
    }

    fn push_chunk(&mut self, data: &[u8], final_chunk: bool) -> Result<(), Error> {
        let opcode = if final_chunk { 0x91 } else { 0x90 };
        let mut payload = vec![0xC6, 0x14, opcode, checksum(data)];
        payload.extend_from_slice(data);

        for attempt in 0..=self.retries {
            let response = self.request(&payload, None)?;
            Self::expect_group(&response, b"\xC6\x14")?;
            let expected = if final_chunk {
                STATUS_OK
            } else {
                STATUS_UPLOAD_MORE
            };
            if response[2] == expected {
                return Ok(());
            }
            if response[2] == STATUS_CHECKSUM_ERROR && attempt < self.retries {
                self.log(format!(
                    "upload checksum rejected; retrying block {}",
                    attempt + 1
                ));
                continue;
            }
            return Err(Self::remote_error(&response, "upload block"));
        }
        unreachable!("the retry loop always returns")
    }

    fn push_once(&mut self, remote: &str, local_path: &Path, size: u64) -> Result<u64, Error> {
        self.begin_push(remote, size)?;
        if self.progress.is_some() {
            self.report_progress(0, Some(size));
        }
        let mut file = File::open(local_path)?;
        let mut sent = 0u64;
        if size == 0 {
            self.push_chunk(&[], true)?;
        } else {
            let mut buffer = vec![0u8; UPLOAD_CHUNK_SIZE];
            loop {
                let read = read_chunk(&mut file, &mut buffer)?;
                if read == 0 {
                    break;
                }
                let data = &buffer[..read];
                let final_chunk = sent + read as u64 == size;
                self.push_chunk(data, final_chunk)?;
                sent += read as u64;
                self.report_progress(sent, Some(size));
            }
        }
        Ok(sent)
    }

    pub fn push(&mut self, local_path: &Path, remote_path: &str) -> Result<u64, Error> {
        let local_path = expand_user(local_path);
        if !local_path.is_file() {
            return Err(Error::Fs(format!(
                "local file does not exist: {}",
                local_path.display()
            )));
        }
        let size = local_path.metadata()?.len();
        if size > u32::MAX as u64 {
            return Err(Error::Fs(
                "MakerCmd upload size is limited to 4 GiB - 1 byte".to_string(),
            ));
        }
        let remote = normalize_remote_path(remote_path)?;

        match self.list_file(&remote) {
            Ok(existing) => {
                return Err(Error::remote(
                    format!(
                        "push refused: remote file already exists ({} bytes): {remote}",
                        existing.size
                    ),
                    Some(STATUS_FS_ERROR),
                    Some(-11),
                ));
            }
            Err(Error::Remote {
                os_error: Some(-1), ..
            }) => {}
            Err(error) => return Err(error),
        }

        let mut last_error: Option<Error> = None;
        for restart in 0..=self.retries {
            match self.push_once(&remote, &local_path, size) {
                Ok(sent) => {
                    self.end_progress_line();
                    return Ok(sent);
                }
                Err(Error::Transport(message)) => {
                    self.end_progress_line();
                    let failure = Error::Transport(message);
                    eprintln!(
                        "Upload response was lost; reconnecting to inspect the remote file..."
                    );

                    let inspect = self.reconnect().and_then(|()| self.list_file(&remote));
                    let info = match inspect {
                        Ok(info) => Some(info),
                        Err(Error::Remote {
                            os_error: Some(-1), ..
                        }) => None,
                        Err(inspect_error @ Error::Remote { .. }) => {
                            return Err(Error::Fs(format!(
                                "upload failed ({failure}); cannot inspect partial file: \
                                 {inspect_error}"
                            )));
                        }
                        Err(inspect_error) => {
                            return Err(Error::Fs(format!(
                                "upload failed ({failure}); reconnect/inspection failed: \
                                 {inspect_error}"
                            )));
                        }
                    };

                    if let Some(info) = info {
                        if info.size == size {
                            eprintln!(
                                "Remote size equals the local size; treating the lost final \
                                 acknowledgement as success."
                            );
                            return Ok(size);
                        }
                        if info.size > size {
                            return Err(Error::Fs(format!(
                                "upload failed and remote size {} exceeds local size {size}; \
                                 refusing automatic deletion",
                                info.size
                            )));
                        }
                        eprintln!("Removing partial remote file ({} bytes)...", info.size);
                        self.delete_file(&remote)?;
                    }

                    last_error = Some(failure);
                    if restart >= self.retries {
                        break;
                    }
                    eprintln!(
                        "Restarting upload from byte 0 ({}/{})...",
                        restart + 1,
                        self.retries
                    );
                }
                Err(error) => return Err(error),
            }
        }

        let last_error = last_error
            .unwrap_or_else(|| Error::Transport("upload failed without a response".to_string()));
        Err(Error::Transport(format!(
            "upload failed after {} attempt(s); any confirmed partial file was removed: \
             {last_error}",
            self.retries + 1
        )))
    }
}

fn read_chunk(file: &mut File, buffer: &mut [u8]) -> io::Result<usize> {
    let mut total = 0;
    while total < buffer.len() {
        match file.read(&mut buffer[total..])? {
            0 => break,
            read => total += read,
        }
    }
    Ok(total)
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

fn repr(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn recorded_repr(value: Option<&str>) -> String {
    value.map(repr).unwrap_or_else(|| "None".to_string())
}

fn home_dir() -> Option<OsString> {
    env::var_os("HOME").or_else(|| env::var_os("USERPROFILE"))
}

pub fn expand_user(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if text == "~" {
        if let Some(home) = home_dir() {
            return PathBuf::from(home);
        }
    }
    if let Some(rest) = text.strip_prefix("~/").or_else(|| text.strip_prefix("~\\")) {
        if let Some(home) = home_dir() {
            return PathBuf::from(home).join(rest);
        }
    }
    path.to_path_buf()
}

fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(windows)]
    if to.exists() {
        let _ = fs::remove_file(to);
    }
    fs::rename(from, to)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_is_send_and_sync_safe_to_hand_to_another_thread() {
        fn assert_send<T: Send>() {}
        assert_send::<MakerCmdClient>();
    }

    #[test]
    fn sibling_paths_append_suffixes() {
        assert_eq!(
            sibling(Path::new("dir/file.bin"), ".part"),
            PathBuf::from("dir/file.bin.part")
        );
        assert_eq!(
            sibling(Path::new("dir/file.bin"), ".part.json"),
            PathBuf::from("dir/file.bin.part.json")
        );
    }

    #[test]
    fn resume_merges_only_matching_overlap() {
        let mut path = std::env::temp_dir();
        path.push(format!("fmaker-resume-test-{}", std::process::id()));
        fs::write(&path, b"abcd").unwrap();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let mut old_size = 4;
        // The first two bytes match, so they are kept and the rest is appended.
        let verified =
            MakerCmdClient::merge_resumed_chunk(&mut file, b"abcdEF", 0, &mut old_size).unwrap();
        assert_eq!(verified, 6);
        drop(file);
        assert_eq!(fs::read(&path).unwrap(), b"abcdEF");
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn resume_restarts_after_a_mismatch() {
        let mut path = std::env::temp_dir();
        path.push(format!("fmaker-resume-mismatch-{}", std::process::id()));
        fs::write(&path, b"xxxx").unwrap();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let mut old_size = 4;
        let verified =
            MakerCmdClient::merge_resumed_chunk(&mut file, b"abcd", 0, &mut old_size).unwrap();
        assert_eq!(verified, 4);
        assert_eq!(old_size, 0);
        drop(file);
        assert_eq!(fs::read(&path).unwrap(), b"abcd");
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn remote_errors_decode_signed_symbian_codes() {
        let error = MakerCmdClient::remote_error(
            &[0xC6, 0x13, STATUS_FS_ERROR, 0xFF, 0xFF, 0xFF, 0xFF],
            "stat D:\\x",
        );
        match error {
            Error::Remote {
                message,
                status,
                os_error,
            } => {
                assert_eq!(status, Some(STATUS_FS_ERROR));
                assert_eq!(os_error, Some(-1));
                assert_eq!(
                    message,
                    "stat D:\\x failed: remote status 0x12, Symbian error -1"
                );
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn file_info_serializes_in_protocol_order() {
        let info = FileInfo {
            path: "D:\\file.bin".to_string(),
            size: 5,
            attributes_raw: 0x05,
            attributes: vec!["archive", "readonly"],
            year: 2024,
            timestamp_raw: "07e800000000".to_string(),
        };
        let json = serde_json::to_string_pretty(&info).unwrap();
        let mut last = 0;
        for key in [
            "\"path\"",
            "\"size\"",
            "\"attributes_raw\"",
            "\"attributes\"",
            "\"year\"",
            "\"timestamp_raw\"",
        ] {
            let position = json.find(key).unwrap();
            assert!(position >= last, "{key} is out of order in {json}");
            last = position;
        }
    }

    #[test]
    fn download_chunks_validate_checksums() {
        let (status, data) =
            MakerCmdClient::validate_download_chunk(b"\xC6\x13\x91\x02\x01\x01", "pull D:\\x")
                .unwrap();
        assert_eq!(status, STATUS_LAST);
        assert_eq!(data, b"\x01\x01");

        let error = MakerCmdClient::validate_download_chunk(b"\xC6\x13\x90\x00\x01", "pull D:\\x")
            .unwrap_err();
        assert!(matches!(
            error,
            Error::Checksum {
                status: STATUS_MORE,
                ..
            }
        ));
    }
}
