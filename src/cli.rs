use std::path::Path;
use std::time::Duration;

use clap::{ArgGroup, Parser};

use crate::client::{MakerCmdClient, expand_user};
use crate::error::Error;
use crate::protocol::{DEFAULT_PID, DEFAULT_VID, format_size, normalize_remote_path};

#[derive(Parser, Debug)]
#[command(name = "fmaker-explore", version, about = "MakerCmd file tool")]
#[command(group(ArgGroup::new("action").required(true)))]
pub struct Cli {
    #[arg(long, group = "action", help = "connect")]
    pub connect: bool,

    #[arg(
        long,
        group = "action",
        num_args = 2,
        value_names = ["LOCAL", "REMOTE"],
        help = "upload file"
    )]
    pub push: Option<Vec<String>>,

    #[arg(
        long,
        group = "action",
        num_args = 2,
        value_names = ["REMOTE", "LOCAL"],
        help = "download file"
    )]
    pub pull: Option<Vec<String>>,

    #[arg(
        long,
        group = "action",
        value_name = "REMOTE_DIR",
        help = "create directory"
    )]
    pub mkdir: Option<String>,

    #[arg(
        long,
        group = "action",
        value_name = "REMOTE_FILE",
        help = "delete file"
    )]
    pub delete: Option<String>,

    #[arg(long, group = "action", help = "show D: free space")]
    pub checkdavailable: bool,

    #[arg(
        long,
        group = "action",
        value_name = "REMOTE_DIR",
        help = "list directory"
    )]
    pub listdir: Option<String>,

    #[arg(
        long,
        group = "action",
        value_name = "REMOTE_FILE",
        help = "show file info"
    )]
    pub listfile: Option<String>,

    #[arg(long, group = "action", help = "list drives")]
    pub listpartition: bool,

    #[arg(
        long,
        group = "action",
        num_args = 1..=2,
        value_names = ["REMOTE_FILE", "STATE"],
        help = "set readonly"
    )]
    pub setreadonly: Option<Vec<String>>,

    #[arg(long, value_name = "VID", default_value_t = DEFAULT_VID, value_parser = parse_int, help = "USB VID")]
    pub vid: u16,

    #[arg(long, value_name = "PID", default_value_t = DEFAULT_PID, value_parser = parse_int, help = "USB PID")]
    pub pid: u16,

    #[arg(
        long,
        value_name = "SECONDS",
        default_value_t = 5.0,
        help = "USB timeout"
    )]
    pub timeout: f64,

    #[arg(
        long,
        value_name = "SECONDS",
        default_value_t = 60.0,
        help = "connection timeout"
    )]
    pub maker_wait: f64,

    #[arg(long, value_name = "COUNT", default_value_t = 3, help = "retry count")]
    pub retries: i64,

    #[arg(long, help = "disable resume")]
    pub no_resume: bool,

    #[arg(long, help = "overwrite local file")]
    pub overwrite: bool,

    #[arg(short, long, help = "debug output")]
    pub verbose: bool,
}

pub fn parse_int(value: &str) -> Result<u16, String> {
    let trimmed = value.trim();
    let (negative, body) = if let Some(rest) = trimmed.strip_prefix('-') {
        (true, rest)
    } else if let Some(rest) = trimmed.strip_prefix('+') {
        (false, rest)
    } else {
        (false, trimmed)
    };
    let (radix, digits) =
        if let Some(rest) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
            (16, rest)
        } else if let Some(rest) = body.strip_prefix("0o").or_else(|| body.strip_prefix("0O")) {
            (8, rest)
        } else if let Some(rest) = body.strip_prefix("0b").or_else(|| body.strip_prefix("0B")) {
            (2, rest)
        } else {
            (10, body)
        };

    let parsed =
        i64::from_str_radix(digits, radix).map_err(|_| format!("invalid integer: {value:?}"))?;
    let parsed = if negative { -parsed } else { parsed };
    u16::try_from(parsed).map_err(|_| format!("invalid integer: {value:?}"))
}

pub fn parse_readonly(values: &[String]) -> Result<(String, bool), Error> {
    if values.is_empty() || values.len() > 2 {
        return Err(Error::Fs(
            "--setreadonly expects PATH and optional on/off".to_string(),
        ));
    }
    let mut enabled = true;
    if values.len() == 2 {
        match values[1].to_lowercase().as_str() {
            "1" | "on" | "true" | "yes" | "set" => enabled = true,
            "0" | "off" | "false" | "no" | "clear" => enabled = false,
            _ => {
                return Err(Error::Fs(format!(
                    "invalid readonly state: {:?}",
                    values[1]
                )));
            }
        }
    }
    Ok((values[0].clone(), enabled))
}

enum Action {
    Connect,
    Push(String, String),
    Pull(String, String),
    Mkdir(String),
    Delete(String),
    CheckDAvailable,
    ListDir(String),
    ListFile(String),
    ListPartitions,
    SetReadonly(Vec<String>),
}

impl Cli {
    fn action(&self) -> Action {
        if self.connect {
            Action::Connect
        } else if let Some(values) = &self.push {
            Action::Push(values[0].clone(), values[1].clone())
        } else if let Some(values) = &self.pull {
            Action::Pull(values[0].clone(), values[1].clone())
        } else if let Some(path) = &self.mkdir {
            Action::Mkdir(path.clone())
        } else if let Some(path) = &self.delete {
            Action::Delete(path.clone())
        } else if self.checkdavailable {
            Action::CheckDAvailable
        } else if let Some(path) = &self.listdir {
            Action::ListDir(path.clone())
        } else if let Some(path) = &self.listfile {
            Action::ListFile(path.clone())
        } else if self.listpartition {
            Action::ListPartitions
        } else if let Some(values) = &self.setreadonly {
            Action::SetReadonly(values.clone())
        } else {
            unreachable!("clap requires exactly one action")
        }
    }
}

pub fn run(cli: Cli) -> Result<(), Error> {
    let action = cli.action();
    let mut client = MakerCmdClient::new(
        cli.vid,
        cli.pid,
        Duration::from_secs_f64(cli.timeout),
        cli.retries as u32,
        Duration::from_secs_f64(cli.maker_wait),
        cli.verbose,
    );

    let result = (|| -> Result<(), Error> {
        client.connect()?;
        match action {
            Action::Connect => {
                println!("Connected to MakerCmd at {:04x}:{:04x}", cli.vid, cli.pid);
            }
            Action::Push(local, remote) => {
                let count = client.push(Path::new(&local), &remote)?;
                println!(
                    "Uploaded {count} bytes to {}",
                    normalize_remote_path(&remote)?
                );
            }
            Action::Pull(remote, local) => {
                let local_path = expand_user(Path::new(&local));
                let count =
                    client.pull(&remote, Path::new(&local), !cli.no_resume, cli.overwrite)?;
                println!("Downloaded {count} bytes to {}", local_path.display());
            }
            Action::Mkdir(remote) => {
                client.mkdir(&remote)?;
                println!("Created {}", normalize_remote_path(&remote)?);
            }
            Action::Delete(remote) => {
                client.delete_file(&remote)?;
                println!("Deleted {}", normalize_remote_path(&remote)?);
            }
            Action::CheckDAvailable => {
                let available = client.check_d_available()?;
                println!(
                    "D: available: {available} bytes ({})",
                    format_size(available)
                );
            }
            Action::ListDir(remote) => {
                for entry in client.list_directory(&remote)? {
                    println!("{entry}");
                }
            }
            Action::ListFile(remote) => {
                let info = client.list_file(&remote)?;
                println!("{}", serde_json::to_string_pretty(&info)?);
            }
            Action::ListPartitions => {
                for partition in client.list_partitions()? {
                    println!("{partition}");
                }
            }
            Action::SetReadonly(values) => {
                let (path, enabled) = parse_readonly(&values)?;
                client.set_readonly(&path, enabled)?;
                println!(
                    "Readonly {}: {}",
                    if enabled { "enabled" } else { "disabled" },
                    normalize_remote_path(&path)?
                );
            }
        }
        Ok(())
    })();

    client.close();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_accept_prefixed_bases() {
        assert_eq!(parse_int("0x06d3").unwrap(), 0x06D3);
        assert_eq!(parse_int("1747").unwrap(), 1747);
        assert_eq!(parse_int("0o755").unwrap(), 0o755);
        assert_eq!(parse_int("0b101").unwrap(), 5);
        assert!(parse_int("0x10000").is_err());
        assert!(parse_int("nope").is_err());
    }

    #[test]
    fn readonly_state_parsing_matches_the_reference_tool() {
        let path = "D:\\file.bin".to_string();
        assert_eq!(
            parse_readonly(std::slice::from_ref(&path)).unwrap(),
            (path.clone(), true)
        );
        assert_eq!(
            parse_readonly(&[path.clone(), "OFF".to_string()]).unwrap(),
            (path.clone(), false)
        );
        assert!(parse_readonly(&[path, "maybe".to_string()]).is_err());
    }
}
