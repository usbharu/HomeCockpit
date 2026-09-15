#![cfg(unix)]

use std::{
    io::{self, BufRead},
    process::ExitCode,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use clap::Parser;
use upper_panel_ddi_mock::pty::{Pty, Readiness};
use upper_panel_ddi_mock::{Command, DEFAULT_DEVICE_ID, DeviceNotice, MockDevice, parse_command};

const IO_POLL_INTERVAL: Duration = Duration::from_millis(20);
const TRANSMIT_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Parser)]
#[command(version, about = "Run an Upper Panel DDI mock on a POSIX PTY")]
struct Options {
    /// Stable HCP device ID, in decimal or 0x-prefixed hexadecimal notation.
    #[arg(long, default_value_t = DEFAULT_DEVICE_ID, value_parser = parse_device_id)]
    device_id: u64,
}

fn parse_device_id(value: &str) -> Result<u64, String> {
    let parsed = if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        u64::from_str_radix(hex, 16)
    } else {
        value.parse::<u64>()
    };
    parsed.map_err(|error| format!("invalid device ID '{value}': {error}"))
}

fn main() -> ExitCode {
    let options = Options::parse();
    match run(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(options: Options) -> Result<(), String> {
    let mut pty = Pty::open().map_err(|error| format!("failed to create PTY: {error}"))?;
    let mut rx_buffer = [0u8; 256];
    let mut parser_buffer = [0u8; 256];
    let mut device = MockDevice::new(options.device_id, &mut rx_buffer, &mut parser_buffer);
    device.restart().map_err(|error| error.to_string())?;
    let commands = spawn_stdin_reader();

    println!("Upper Panel DDI mock");
    println!("  Device ID : {:016X}", options.device_id);
    println!("  PTY path  : {}", pty.slave_path().display());
    println!("  Manager   : serial / 115200 baud / auto or direct-device");
    print_help();

    let mut peer_connected = false;
    let mut next_transmit = Instant::now();
    let mut read_buffer = [0u8; 512];

    loop {
        match drain_commands(&commands, &mut device)? {
            CommandLoop::Continue => {}
            CommandLoop::Quit => return Ok(()),
        }

        match pty.wait_readable(IO_POLL_INTERVAL) {
            Ok(Readiness::Readable) => match pty.read(&mut read_buffer) {
                Ok(0) => mark_disconnected(&mut peer_connected, &mut device)?,
                Ok(length) => {
                    if !peer_connected {
                        peer_connected = true;
                        println!("Manager connected");
                    }
                    match device.receive_bytes(&read_buffer[..length]) {
                        Ok(notices) => print_notices(notices),
                        Err(error) => eprintln!("warning: {error}"),
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => {
                    if is_disconnect_error(&error) {
                        mark_disconnected(&mut peer_connected, &mut device)?;
                    } else {
                        return Err(format!("failed to read PTY: {error}"));
                    }
                }
            },
            Ok(Readiness::Disconnected) => {
                mark_disconnected(&mut peer_connected, &mut device)?;
            }
            Ok(Readiness::Timeout) => {}
            Err(error) => return Err(format!("failed to poll PTY: {error}")),
        }

        if Instant::now() >= next_transmit {
            next_transmit = Instant::now() + TRANSMIT_INTERVAL;
            if let Some(bytes) = device
                .next_wire_frame()
                .map_err(|error| error.to_string())?
            {
                match pty.write_all(&bytes) {
                    Ok(()) => {
                        if !peer_connected {
                            peer_connected = true;
                            println!("Manager connected; waiting for address assignment");
                        }
                    }
                    Err(error) if is_disconnect_error(&error) => {
                        mark_disconnected(&mut peer_connected, &mut device)?;
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(error) => return Err(format!("failed to write PTY: {error}")),
                }
            }
        }
    }
}

fn spawn_stdin_reader() -> Receiver<String> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            match line {
                Ok(line) => {
                    if sender.send(line).is_err() {
                        return;
                    }
                }
                Err(error) => {
                    eprintln!("warning: failed to read command: {error}");
                    return;
                }
            }
        }
        let _ = sender.send("quit".to_string());
    });
    receiver
}

enum CommandLoop {
    Continue,
    Quit,
}

fn drain_commands(
    commands: &Receiver<String>,
    device: &mut MockDevice<'_>,
) -> Result<CommandLoop, String> {
    loop {
        let line = match commands.try_recv() {
            Ok(line) => line,
            Err(TryRecvError::Empty) => return Ok(CommandLoop::Continue),
            Err(TryRecvError::Disconnected) => return Ok(CommandLoop::Quit),
        };
        let command = match parse_command(&line) {
            Ok(command) => command,
            Err(error) => {
                eprintln!("warning: {error}");
                continue;
            }
        };
        match command {
            Command::Press(id) => match device.press(id) {
                Ok(()) => println!("queued press control-id={id}"),
                Err(error) => eprintln!("warning: {error}"),
            },
            Command::Release(id) => match device.release(id) {
                Ok(()) => println!("queued release control-id={id}"),
                Err(error) => eprintln!("warning: {error}"),
            },
            Command::Tap(id) => match device.tap(id) {
                Ok(()) => println!("queued tap control-id={id}"),
                Err(error) => eprintln!("warning: {error}"),
            },
            Command::Status => {
                let address = device.address().map_or_else(
                    || "unassigned".to_string(),
                    |value| format!("0x{value:02X}"),
                );
                let pressed = device
                    .pressed_controls()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>();
                let pressed = if pressed.is_empty() {
                    "none".to_string()
                } else {
                    pressed.join(", ")
                };
                println!("address={address} pressed=[{pressed}]");
            }
            Command::Help => print_help(),
            Command::Quit => return Ok(CommandLoop::Quit),
        }
    }
}

fn print_help() {
    println!("Commands: press <0-39>, release <0-39>, tap <0-39>, status, help, quit");
}

fn print_notices(notices: Vec<DeviceNotice>) {
    for notice in notices {
        match notice {
            DeviceNotice::AddressAssigned(address) => {
                println!("IMCP address assigned: 0x{address:02X}");
            }
            DeviceNotice::DisplayData { data, accepted } => {
                println!(
                    "DisplayData seq={} target={:?} payload={:?} {}",
                    data.seq,
                    data.target,
                    data.payload,
                    if accepted { "accepted" } else { "stale" }
                );
            }
            DeviceNotice::DeviceHelloRequested => println!("DeviceHello requested; queued reply"),
        }
    }
}

fn mark_disconnected(peer_connected: &mut bool, device: &mut MockDevice<'_>) -> Result<(), String> {
    if *peer_connected {
        println!("Manager disconnected; restarting IMCP Join");
        *peer_connected = false;
        device.restart().map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn is_disconnect_error(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::BrokenPipe
        || error.kind() == io::ErrorKind::UnexpectedEof
        || error.raw_os_error() == Some(libc::EIO)
        || error.raw_os_error() == Some(libc::ENXIO)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_decimal_and_hex_device_ids() {
        assert_eq!(parse_device_id("42"), Ok(42));
        assert_eq!(parse_device_id("0x2A"), Ok(42));
        assert_eq!(parse_device_id("0XDD10"), Ok(0xDD10));
        assert!(parse_device_id("DD10").is_err());
        assert_eq!(DEFAULT_DEVICE_ID, 0xDD10_0000_0000_0001);
    }
}
