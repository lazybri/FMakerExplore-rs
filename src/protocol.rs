use crate::error::Error;

pub const DEFAULT_VID: u16 = 0x06D3;
pub const DEFAULT_PID: u16 = 0x21B0;
pub const EP_OUT: u8 = 0x03;
pub const EP_IN: u8 = 0x82;
pub const EP_CONTROL_REPLY: u8 = 0x81;
pub const READ_SIZE: usize = 4096;
pub const UPLOAD_CHUNK_SIZE: usize = 1500;
pub const REQUEST_MAGIC: &[u8; 3] = b"\x55\x56\x42";
pub const RESPONSE_MAGIC: &[u8; 3] = b"\x56\x55\x42";

pub const STATUS_MORE: u8 = 0x90;
pub const STATUS_LAST: u8 = 0x91;
pub const STATUS_INFO: u8 = 0x92;
pub const STATUS_FILE_ERROR: u8 = 0xA1;
pub const STATUS_OK: u8 = 0x00;
pub const STATUS_UPLOAD_MORE: u8 = 0x20;
pub const STATUS_CHECKSUM_ERROR: u8 = 0x21;
pub const STATUS_NO_SPACE: u8 = 0x11;
pub const STATUS_FS_ERROR: u8 = 0x12;

pub fn mask_packet(packet: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(packet.len() + 4);
    out.push(0xFF);
    for &byte in packet {
        if matches!(byte, 0xFD..=0xFF) {
            out.push(0xFD);
            out.push(byte ^ 0x10);
        } else {
            out.push(byte);
        }
    }
    out.push(0xFE);
    out
}

pub fn unmask_packet(frame: &[u8]) -> Result<Vec<u8>, Error> {
    if frame.len() < 2 || frame[0] != 0xFF || frame[frame.len() - 1] != 0xFE {
        return Err(Error::Protocol(
            "invalid MakerCmd frame delimiters".to_string(),
        ));
    }

    let end = frame.len() - 1;
    let mut out = Vec::with_capacity(frame.len());
    let mut pos = 1;
    while pos < end {
        let byte = frame[pos];
        if byte == 0xFD {
            if pos + 1 >= end {
                return Err(Error::Protocol(
                    "truncated MakerCmd escape sequence".to_string(),
                ));
            }
            out.push(frame[pos + 1] ^ 0x10);
            pos += 2;
        } else {
            out.push(byte);
            pos += 1;
        }
    }
    Ok(out)
}

pub fn encode_remote_path(path: &str) -> Result<String, Error> {
    let normalized = path.replace('/', "\\");
    let bytes = normalized.as_bytes();
    let is_absolute =
        bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\';
    if !is_absolute {
        return Err(Error::Fs(format!(
            "remote path must be absolute (for example D:\\Data\\file.bin): {path:?}"
        )));
    }
    if !normalized.is_ascii() {
        return Err(Error::Fs(
            "MakerCmd paths must currently contain ASCII characters only".to_string(),
        ));
    }
    Ok(normalized)
}

pub fn normalize_remote_path(path: &str) -> Result<String, Error> {
    encode_remote_path(path)
}

pub fn format_size(value: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut amount = value as f64;
    for unit in UNITS {
        if amount < 1024.0 || unit == "GiB" {
            return if unit == "B" {
                format!("{amount:.0} B")
            } else {
                format!("{amount:.2} {unit}")
            };
        }
        amount /= 1024.0;
    }
    unreachable!("the unit list is never empty")
}

pub fn checksum(data: &[u8]) -> u8 {
    data.iter().fold(0u8, |acc, byte| acc.wrapping_add(*byte))
}

pub fn hex_spaced(data: &[u8]) -> String {
    data.iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn hex_compact(data: &[u8]) -> String {
    data.iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

pub struct FrameReader {
    buffer: Vec<u8>,
}

impl Default for FrameReader {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameReader {
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    pub fn feed(&mut self, data: &[u8]) -> Option<Vec<u8>> {
        self.buffer.extend_from_slice(data);
        loop {
            let Some(start) = self.buffer.iter().position(|&byte| byte == 0xFF) else {
                self.buffer.clear();
                return None;
            };
            if start > 0 {
                self.buffer.drain(..start);
            }

            let mut escaped = false;
            let mut restart = false;
            for pos in 1..self.buffer.len() {
                let byte = self.buffer[pos];
                if escaped {
                    escaped = false;
                    continue;
                }
                match byte {
                    0xFD => escaped = true,
                    0xFE => {
                        let frame = self.buffer[..=pos].to_vec();
                        self.buffer.drain(..=pos);
                        return Some(frame);
                    }
                    0xFF => {
                        // A nested start delimiter discards the partial frame and
                        // restarts parsing from that delimiter.
                        self.buffer.drain(..pos);
                        restart = true;
                        break;
                    }
                    _ => {}
                }
            }
            if !restart {
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masking_escapes_delimiter_bytes() {
        assert_eq!(
            mask_packet(b"\x01\xFD\xFE\xFF\x02"),
            b"\xFF\x01\xFD\xED\xFD\xEE\xFD\xEF\x02\xFE"
        );
    }

    #[test]
    fn masking_round_trips() {
        let packet: Vec<u8> = (0..=255u8).collect();
        let masked = mask_packet(&packet);
        assert_eq!(unmask_packet(&masked).unwrap(), packet);
    }

    #[test]
    fn unmasking_rejects_bad_delimiters() {
        assert!(unmask_packet(b"\x00\xFE").is_err());
        assert!(unmask_packet(b"\xFF\xFE").is_ok());
    }

    #[test]
    fn frame_reader_handles_split_frames() {
        let mut reader = FrameReader::new();
        assert!(reader.feed(b"\x00\xFF\x56").is_none());
        assert_eq!(reader.feed(b"\xFE").unwrap(), b"\xFF\x56\xFE");
        assert!(reader.feed(b"").is_none());
    }

    #[test]
    fn frame_reader_restarts_at_nested_delimiter() {
        let mut reader = FrameReader::new();
        assert_eq!(
            reader.feed(b"\xFE\xFF\x01\xFF\x02\xFE").unwrap(),
            b"\xFF\x02\xFE"
        );
    }

    #[test]
    fn frame_reader_waits_for_complete_frame() {
        let mut reader = FrameReader::new();
        assert!(reader.feed(b"\xFF\x01").is_none());
        assert_eq!(reader.feed(b"\x02\xFE").unwrap(), b"\xFF\x01\x02\xFE");
    }

    #[test]
    fn paths_are_normalized_and_validated() {
        assert_eq!(
            normalize_remote_path("D:/WcdmaMp/file.bin").unwrap(),
            "D:\\WcdmaMp\\file.bin"
        );
        assert_eq!(normalize_remote_path("D:\\").unwrap(), "D:\\");
        assert!(normalize_remote_path("relative/file.bin").is_err());
        assert!(normalize_remote_path("D:\\\u{00e9}").is_err());
    }

    #[test]
    fn sizes_are_formatted_like_the_reference_tool() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1024), "1.00 KiB");
        assert_eq!(format_size(1536), "1.50 KiB");
        assert_eq!(format_size(1024 * 1024), "1.00 MiB");
        assert_eq!(format_size(3 * 1024 * 1024 * 1024), "3.00 GiB");
    }

    #[test]
    fn checksum_wraps_at_eight_bits() {
        assert_eq!(checksum(b""), 0);
        assert_eq!(checksum(&[0xFF, 0x02]), 0x01);
    }
}
