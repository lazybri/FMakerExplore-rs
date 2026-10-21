use std::ffi::{CStr, CString, c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use crate::client::{FileInfo, MakerCmdClient};
use crate::error::Error;
use crate::protocol::encode_remote_path;

const MAX_INSPECTED_ENTRIES: usize = 500;

/// Opaque handle used by the C API. Instances are single-threaded: the caller
/// must not issue two calls concurrently through the same handle.
pub struct FmClient {
    client: MakerCmdClient,
}

pub type FmProgressCallback = Option<unsafe extern "C" fn(*mut c_void, u64, i64)>;

#[unsafe(no_mangle)]
pub extern "C" fn fm_client_new(
    vid: u16,
    pid: u16,
    timeout_ms: u32,
    retries: u32,
    maker_wait_ms: u32,
    verbose: i32,
) -> *mut FmClient {
    let client = MakerCmdClient::new(
        vid,
        pid,
        Duration::from_millis(u64::from(timeout_ms.max(1))),
        retries,
        Duration::from_millis(u64::from(maker_wait_ms.max(1000))),
        verbose != 0,
    );
    Box::into_raw(Box::new(FmClient { client }))
}

/// # Safety
///
/// `client` must be null or a pointer returned by [`fm_client_new`] that has not
/// already been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fm_client_free(client: *mut FmClient) {
    if client.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(client));
    }
}

/// # Safety
///
/// `client` must be null or a valid pointer returned by [`fm_client_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fm_client_close(client: *mut FmClient) {
    if client.is_null() {
        return;
    }
    unsafe { &mut *client }.client.close();
}

/// # Safety
///
/// `client` must be null or a valid pointer returned by [`fm_client_new`].
/// `user` must remain valid until the callback is cleared or the client is freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fm_client_set_progress_callback(
    client: *mut FmClient,
    callback: FmProgressCallback,
    user: *mut c_void,
) {
    if client.is_null() {
        return;
    }
    let client = unsafe { &mut *client };
    match callback {
        Some(callback) => {
            let user = user as usize;
            client
                .client
                .set_progress_handler(Some(Box::new(move |current, total| {
                    let total = total
                        .map(|value| value.min(i64::MAX as u64) as i64)
                        .unwrap_or(-1);
                    unsafe { callback(user as *mut c_void, current, total) };
                })));
        }
        None => client.client.set_progress_handler(None),
    }
}

/// # Safety
///
/// `client` must be null or a valid pointer returned by [`fm_client_new`].
/// `command`, `arg1`, and `arg2` must be null or valid NUL-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fm_call(
    client: *mut FmClient,
    command: *const c_char,
    arg1: *const c_char,
    arg2: *const c_char,
) -> *mut c_char {
    let outcome = catch_unwind(AssertUnwindSafe(|| -> Result<Value, Error> {
        if client.is_null() {
            return Err(Error::Fs("client is null".to_string()));
        }
        let command =
            read_string(command)?.ok_or_else(|| Error::Fs("missing command".to_string()))?;
        let arg1 = read_string(arg1)?;
        let arg2 = read_string(arg2)?;
        let client = unsafe { &mut *client };
        dispatch(&mut client.client, &command, arg1, arg2)
    }));

    let payload = match outcome {
        Ok(Ok(data)) => json!({ "ok": true, "data": data }),
        Ok(Err(error)) => error_payload(&error),
        Err(_) => json!({
            "ok": false,
            "error": "internal Rust panic",
            "kind": "panic",
            "status": null,
            "os_error": null,
        }),
    };
    into_c_string(payload)
}

/// # Safety
///
/// `value` must be null or a string previously returned by [`fm_call`] that has
/// not already been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fm_free_string(value: *mut c_char) {
    if value.is_null() {
        return;
    }
    unsafe {
        drop(CString::from_raw(value));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn fm_version() -> *const c_char {
    const VERSION: &[u8] = concat!(env!("CARGO_PKG_VERSION"), "\0").as_bytes();
    VERSION.as_ptr().cast()
}

fn read_string(value: *const c_char) -> Result<Option<String>, Error> {
    if value.is_null() {
        return Ok(None);
    }
    let text = unsafe { CStr::from_ptr(value) }
        .to_str()
        .map_err(|error| Error::Fs(format!("argument is not valid UTF-8: {error}")))?;
    Ok(Some(text.to_string()))
}

fn required(value: Option<String>, name: &str) -> Result<String, Error> {
    value.ok_or_else(|| Error::Fs(format!("missing argument: {name}")))
}

fn dispatch(
    client: &mut MakerCmdClient,
    command: &str,
    arg1: Option<String>,
    arg2: Option<String>,
) -> Result<Value, Error> {
    match command {
        "connect" => {
            client.close();
            client.connect()?;
            Ok(json!(true))
        }
        "disconnect" => {
            client.close();
            Ok(json!(true))
        }
        "list_partitions" => Ok(json!(client.list_partitions()?)),
        "list_directory" => {
            let path = required(arg1, "remote path")?;
            Ok(json!(client.list_directory(&path)?))
        }
        "list_entries" => {
            let path = required(arg1, "remote path")?;
            list_entries(client, &path)
        }
        "list_file" => {
            let path = required(arg1, "remote path")?;
            Ok(json!(client.list_file(&path)?))
        }
        "mkdir" => {
            let path = required(arg1, "remote path")?;
            client.mkdir(&path)?;
            Ok(json!(true))
        }
        "delete" => {
            let path = required(arg1, "remote path")?;
            client.delete_file(&path)?;
            Ok(json!(true))
        }
        "set_readonly" => {
            let path = required(arg1, "remote path")?;
            let enabled = match arg2.as_deref().map(str::to_ascii_lowercase).as_deref() {
                Some("1" | "on" | "true" | "yes" | "set") => true,
                Some("0" | "off" | "false" | "no" | "clear") => false,
                _ => {
                    return Err(Error::Fs(
                        "set_readonly expects \"on\" or \"off\"".to_string(),
                    ));
                }
            };
            client.set_readonly(&path, enabled)?;
            Ok(json!(true))
        }
        "checkdavailable" => Ok(json!(client.check_d_available()?)),
        "pull" => {
            let remote = required(arg1, "remote path")?;
            let local = required(arg2, "local path")?;
            let bytes = client.pull(&remote, Path::new(&local), true, true)?;
            Ok(json!({ "bytes": bytes }))
        }
        "push" => {
            let local = required(arg1, "local path")?;
            let remote = required(arg2, "remote path")?;
            let bytes = client.push(Path::new(&local), &remote)?;
            Ok(json!({ "bytes": bytes }))
        }
        "version" => Ok(json!(env!("CARGO_PKG_VERSION"))),
        other => Err(Error::Fs(format!("unknown command: {other}"))),
    }
}

fn list_entries(client: &mut MakerCmdClient, remote_path: &str) -> Result<Value, Error> {
    let names = client.list_directory(remote_path)?;
    let mut base = encode_remote_path(remote_path)?;
    if !base.ends_with('\\') {
        base.push('\\');
    }

    let mut entries = Vec::with_capacity(names.len());
    let mut inspected = 0usize;
    for name in names {
        let trimmed = name.trim_end_matches(['\\', '/']).to_string();
        let mut is_dir = trimmed.len() != name.len();
        let full = format!("{base}{name}");

        let mut size: Option<u64> = None;
        let mut attributes: Vec<&'static str> = Vec::new();
        let mut year: Option<u16> = None;
        let mut inspect_error: Option<String> = None;

        if !is_dir && inspected < MAX_INSPECTED_ENTRIES {
            inspected += 1;
            match client.list_file(&full) {
                Ok(info) => {
                    let FileInfo {
                        size: file_size,
                        attributes: file_attributes,
                        year: file_year,
                        ..
                    } = info;
                    size = Some(file_size);
                    attributes = file_attributes;
                    year = Some(file_year);
                }
                Err(error @ Error::Remote { .. }) => {
                    // A stat failure is the only reliable directory hint the
                    // protocol gives us for an entry without a name suffix.
                    is_dir = true;
                    inspect_error = Some(error.to_string());
                }
                Err(error) => inspect_error = Some(error.to_string()),
            }
        }

        entries.push(json!({
            "name": trimmed,
            "path": full,
            "is_dir": is_dir,
            "inspected": size.is_some(),
            "size": size,
            "attributes": attributes,
            "year": year,
            "error": inspect_error,
        }));
    }

    let available = client.check_d_available().ok();
    Ok(json!({ "entries": entries, "available": available }))
}

fn error_payload(error: &Error) -> Value {
    let (kind, status, os_error) = match error {
        Error::Fs(_) => ("fs", None, None),
        Error::Device(_) => ("device", None, None),
        Error::Protocol(_) => ("protocol", None, None),
        Error::Transport(_) => ("transport", None, None),
        Error::TransferRestart(_) => ("restart", None, None),
        Error::Checksum { status, .. } => ("checksum", Some(*status), None),
        Error::Remote {
            status, os_error, ..
        } => ("remote", *status, *os_error),
        Error::Io(_) => ("io", None, None),
        Error::Json(_) => ("json", None, None),
    };
    json!({
        "ok": false,
        "error": error.to_string(),
        "kind": kind,
        "status": status,
        "os_error": os_error,
    })
}

fn into_c_string(value: Value) -> *mut c_char {
    match CString::new(value.to_string()) {
        Ok(text) => text.into_raw(),
        Err(_) => CString::new(
            r#"{"ok":false,"error":"response contained a NUL byte","kind":"internal","status":null,"os_error":null}"#,
        )
        .expect("static response is a valid C string")
        .into_raw(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_handles_version_and_unknown_commands() {
        let mut client = MakerCmdClient::new(
            0x06D3,
            0x21B0,
            Duration::from_secs(1),
            0,
            Duration::from_secs(1),
            false,
        );
        assert_eq!(
            dispatch(&mut client, "version", None, None).unwrap(),
            json!(env!("CARGO_PKG_VERSION"))
        );
        assert!(dispatch(&mut client, "does-not-exist", None, None).is_err());
    }

    #[test]
    fn remote_errors_are_reported_with_metadata() {
        let error = Error::remote("stat failed", Some(0x12), Some(-1));
        let payload = error_payload(&error);
        assert_eq!(payload["ok"], json!(false));
        assert_eq!(payload["kind"], json!("remote"));
        assert_eq!(payload["status"], json!(0x12));
        assert_eq!(payload["os_error"], json!(-1));
    }

    #[test]
    fn strings_round_trip_through_the_c_api_types() {
        let raw = into_c_string(json!({"ok": true, "data": null}));
        let text = unsafe { CStr::from_ptr(raw) }.to_str().unwrap().to_string();
        unsafe { fm_free_string(raw) };
        assert_eq!(text, r#"{"data":null,"ok":true}"#);
    }
}
