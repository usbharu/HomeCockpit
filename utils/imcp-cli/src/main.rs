use std::{
    cell::RefCell,
    collections::VecDeque,
    convert::Infallible,
    io::{self, BufRead, IsTerminal, Read, Write},
    process,
    rc::Rc,
    time::Duration,
};

use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};
use futures::executor::block_on;
use imcp::{
    Imcp,
    channel::{Receiver, Sender},
    frame::{Address, Frame, FramePayload, MAX_ENCODED_FRAME_SIZE, MAX_PAYLOAD_SIZE},
    parser::FrameParser,
};
use log::LevelFilter;
use serde_json::json;

#[derive(Parser, Debug)]
#[command(version, about = "imcp-cli", long_about = None)]
struct GlobalOptions {
    #[command(subcommand)]
    command: Commands,

    #[arg(short, long, action = ArgAction::Count)]
    verbose: u8,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// オプションからIMCPフレームを作成します。
    Pack(PackArgs),
    /// 標準入力または--dataオプションの16進数をIMCPフレームとして解析します。
    Unpack(UnpackArgs),
    /// シリアルポートを監視し、受信したIMCPフレームを解析します。
    Watch(WatchArgs),
    /// IMCP masterとしてJOINやPINGなどに応答します。
    Master(MasterArgs),
}

#[derive(Args, Debug)]
struct PackArgs {
    #[arg(short, long, value_parser = clap_num::maybe_hex::<u8>)]
    from: u8,
    #[arg(
        short,
        long,
        value_parser = clap_num::maybe_hex::<u8>,
        conflicts_with = "broadcast",
        required_unless_present = "broadcast"
    )]
    to: Option<u8>,
    #[arg(short, long)]
    broadcast: bool,

    #[arg(short = 'p', long = "packet-type", value_enum)]
    packet_type: PacketType,

    #[arg(long, value_parser = clap_num::maybe_hex::<u32>)]
    id: Option<u32>,
    #[arg(long, value_parser = clap_num::maybe_hex::<u8>)]
    address: Option<u8>,
    #[arg(long)]
    data: Option<String>,
}

#[derive(Args, Debug)]
struct UnpackArgs {
    #[arg(long)]
    data: Option<String>,
    #[arg(long, value_enum, default_value_t = OutputFormat::Debug)]
    format: OutputFormat,
}

#[derive(Args, Debug)]
struct WatchArgs {
    /// 監視するシリアルポート (例: "COM3" or "/dev/ttyUSB0")
    #[arg(short, long, required_unless_present = "list")]
    port: Option<String>,

    /// ボーレート (デフォルト: 9600)
    #[arg(short, long, default_value_t = 9600)]
    baud: u32,

    /// 利用可能なシリアルポートを表示して終了します。
    #[arg(short, long)]
    list: bool,

    #[arg(long, value_enum, default_value_t = OutputFormat::Debug)]
    format: OutputFormat,
}

#[derive(Args, Debug)]
struct MasterArgs {
    /// 接続するシリアルポート。--stdin指定時は不要です。
    #[arg(
        short,
        long,
        conflicts_with = "stdin",
        required_unless_present = "stdin"
    )]
    port: Option<String>,

    /// ボーレート (デフォルト: 9600)
    #[arg(short, long, default_value_t = 9600)]
    baud: u32,

    /// 16進数1行ずつ標準入力から受け取り、masterの応答を標準出力へ出します。
    #[arg(long, conflicts_with = "port")]
    stdin: bool,

    #[arg(long, value_enum, default_value_t = OutputFormat::Debug)]
    format: OutputFormat,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
#[value(rename_all = "lower")]
enum OutputFormat {
    Debug,
    Json,
}

#[derive(ValueEnum, Clone, Debug)]
#[value(rename_all = "lower")]
enum PacketType {
    Ping,
    Pong,
    Ack,
    Join,
    #[value(name = "set-address", alias = "setaddress")]
    SetAddress,
    Data,
    Set,
}

type FrameQueue = Rc<RefCell<VecDeque<Frame>>>;

#[derive(Clone)]
struct FrameQueueSender(FrameQueue);

struct FrameQueueReceiver(FrameQueue);

#[derive(Debug)]
struct FrameQueueEmpty;

impl Sender for FrameQueueSender {
    type Error = Infallible;

    async fn send(&mut self, frame: Frame) -> Result<(), Self::Error> {
        self.0.borrow_mut().push_back(frame);
        Ok(())
    }
}

impl Receiver for FrameQueueReceiver {
    type Error = FrameQueueEmpty;

    async fn receive(&mut self) -> Result<Frame, Self::Error> {
        self.0.borrow_mut().pop_front().ok_or(FrameQueueEmpty)
    }
}

fn main() {
    let cli = GlobalOptions::parse();

    let log_level = match cli.verbose {
        0 => LevelFilter::Warn,
        1 => LevelFilter::Info,
        2 => LevelFilter::Debug,
        _ => LevelFilter::Trace,
    };

    env_logger::Builder::new().filter_level(log_level).init();

    let result = match cli.command {
        Commands::Pack(pack_args) => pack(pack_args),
        Commands::Unpack(unpack_args) => unpack(unpack_args),
        Commands::Watch(watch_args) => watch(watch_args),
        Commands::Master(master_args) => master(master_args),
    };

    if let Err(error) = result {
        eprintln!("error: {error}");
        process::exit(1);
    }
}

fn print_frame(format: OutputFormat, direction: &str, frame: &Frame) {
    match format {
        OutputFormat::Debug => println!("{} {:?}", direction.to_uppercase(), frame),
        OutputFormat::Json => println!(
            "{}",
            json!({
                "event": "frame",
                "direction": direction,
                "to": frame.to_address().as_byte(),
                "to_kind": address_kind(frame.to_address()),
                "from": frame.from_address(),
                "frame_type": frame_type_name(frame.payload()),
                "payload": payload_json(frame.payload()),
            })
        ),
    }
}

fn print_bytes(format: OutputFormat, direction: &str, bytes: &[u8]) {
    let encoded = hex::encode_upper(bytes);
    match format {
        OutputFormat::Debug => println!("{} {}", direction.to_uppercase(), encoded),
        OutputFormat::Json => println!(
            "{}",
            json!({
                "event": "bytes",
                "direction": direction,
                "hex": encoded,
            })
        ),
    }
}

fn print_error(format: OutputFormat, direction: &str, error: &impl std::fmt::Debug) {
    match format {
        OutputFormat::Debug => eprintln!("{} error: {:?}", direction.to_uppercase(), error),
        OutputFormat::Json => println!(
            "{}",
            json!({
                "event": "error",
                "direction": direction,
                "error": format!("{:?}", error),
            })
        ),
    }
}

fn address_kind(address: Address) -> &'static str {
    match address {
        Address::Unicast(_) => "unicast",
        Address::Broadcast => "broadcast",
    }
}

fn frame_type_name(payload: &FramePayload) -> &'static str {
    match payload {
        FramePayload::Ping => "ping",
        FramePayload::Pong => "pong",
        FramePayload::Ack(_) => "ack",
        FramePayload::Join(_) => "join",
        FramePayload::SetAddress { .. } => "set_address",
        FramePayload::Data(_) => "data",
        FramePayload::Set(_) => "set",
    }
}

fn payload_json(payload: &FramePayload) -> serde_json::Value {
    match payload {
        FramePayload::Ping | FramePayload::Pong => json!({}),
        FramePayload::Ack(address) => json!({ "address": address }),
        FramePayload::Join(id) => json!({ "id": id }),
        FramePayload::SetAddress { address, id } => json!({
            "address": address,
            "id": id,
        }),
        FramePayload::Data(data) | FramePayload::Set(data) => {
            json!({ "data": hex::encode_upper(data.as_slice()) })
        }
    }
}

fn handle_line(
    bytes: &[u8],
    frame_parser: &mut FrameParser<'_, '_>,
    format: OutputFormat,
    direction: &str,
) {
    if bytes.is_empty() {
        return;
    }

    if let Err(error) = frame_parser.write_data(bytes) {
        print_error(format, direction, &error);
        return;
    }

    while let Some(frame) = frame_parser.next_frame() {
        match frame {
            Ok(frame) => print_frame(format, direction, &frame),
            Err(error) => print_error(format, direction, &error),
        }
    }
}

fn watch(watch_args: WatchArgs) -> Result<(), String> {
    if watch_args.list {
        let ports = serialport::available_ports()
            .map_err(|error| format!("failed to list serial ports: {error}"))?;
        for port in ports {
            println!("{} {:?}", port.port_name, port.port_type);
        }
        return Ok(());
    }

    let port_name = watch_args
        .port
        .ok_or_else(|| "--port is required unless --list is used".to_string())?;
    let mut port = serialport::new(&port_name, watch_args.baud)
        .timeout(Duration::from_secs(1))
        .dtr_on_open(true)
        .open()
        .map_err(|error| format!("failed to open port {port_name}: {error}"))?;

    let mut rx_buffer = vec![0; 1024];
    let mut frame_buffer = vec![0; 1024];
    let mut frame_parser = FrameParser::new(&mut rx_buffer, &mut frame_buffer);
    let mut serial_buf = vec![0; 1024];

    log::info!("Watching port {} at {} baud...", port_name, watch_args.baud);
    loop {
        match port.read(serial_buf.as_mut_slice()) {
            Ok(bytes_read) if bytes_read > 0 => {
                log::debug!("read uart: {} {:?}", bytes_read, &serial_buf[..bytes_read]);
                handle_line(
                    &serial_buf[..bytes_read],
                    &mut frame_parser,
                    watch_args.format,
                    "rx",
                );
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::TimedOut => {}
            Err(error) => return Err(format!("port read error: {error}")),
        }
    }
}

fn unpack(unpack_args: UnpackArgs) -> Result<(), String> {
    let reader: Box<dyn Iterator<Item = Result<String, io::Error>>> =
        if let Some(data_str) = unpack_args.data {
            Box::new(std::iter::once(Ok(data_str)))
        } else {
            if io::stdin().is_terminal() {
                return Err("--data or a non-interactive stdin pipeline is required".to_string());
            }
            Box::new(io::stdin().lock().lines())
        };

    let mut rx_buffer = vec![0; 1024];
    let mut frame_buffer = vec![0; 1024];
    let mut frame_parser = FrameParser::new(&mut rx_buffer, &mut frame_buffer);

    for line in reader {
        let line = line.map_err(|error| format!("stdin read error: {error}"))?;
        let trimmed_line = line.trim();
        if trimmed_line.is_empty() {
            continue;
        }

        match hex::decode(trimmed_line) {
            Ok(bytes) => handle_line(&bytes, &mut frame_parser, unpack_args.format, "rx"),
            Err(error) => print_error(unpack_args.format, "input", &error),
        }
    }

    Ok(())
}

fn pack(pack_args: PackArgs) -> Result<(), String> {
    let to_address = if pack_args.broadcast {
        Address::Broadcast
    } else {
        Address::Unicast(
            pack_args
                .to
                .ok_or_else(|| "--to or --broadcast is required".to_string())?,
        )
    };

    let frame_payload = match pack_args.packet_type {
        PacketType::Ping => FramePayload::Ping,
        PacketType::Pong => FramePayload::Pong,
        PacketType::Ack => FramePayload::Ack(
            pack_args
                .address
                .ok_or_else(|| "--address is required for ack".to_string())?,
        ),
        PacketType::Join => FramePayload::Join(
            pack_args
                .id
                .ok_or_else(|| "--id is required for join".to_string())?,
        ),
        PacketType::SetAddress => FramePayload::SetAddress {
            address: pack_args
                .address
                .ok_or_else(|| "--address is required for set-address".to_string())?,
            id: pack_args
                .id
                .ok_or_else(|| "--id is required for set-address".to_string())?,
        },
        PacketType::Data => FramePayload::Data(parse_payload(pack_args.data, "data")?),
        PacketType::Set => FramePayload::Set(parse_payload(pack_args.data, "set")?),
    };

    let frame = Frame::new(to_address, pack_args.from, frame_payload);
    let mut buffer = [0u8; MAX_ENCODED_FRAME_SIZE];
    let size = frame
        .encode(&mut buffer)
        .map_err(|error| format!("failed to encode frame: {error:?}"))?;
    println!("{}", hex::encode_upper(&buffer[..size]));
    Ok(())
}

fn parse_payload(
    data: Option<String>,
    name: &str,
) -> Result<heapless::Vec<u8, MAX_PAYLOAD_SIZE>, String> {
    let data = data.ok_or_else(|| format!("--data is required for {name}"))?;
    let bytes = hex::decode(&data).map_err(|error| format!("invalid --data hex: {error}"))?;
    heapless::Vec::from_slice(&bytes)
        .map_err(|_| format!("--data is too large; maximum payload is {MAX_PAYLOAD_SIZE} bytes"))
}

fn master(master_args: MasterArgs) -> Result<(), String> {
    let queue = Rc::new(RefCell::new(VecDeque::new()));
    let sender = FrameQueueSender(Rc::clone(&queue));
    let receiver = FrameQueueReceiver(Rc::clone(&queue));

    let mut imcp_rx_buffer = vec![0; 1024];
    let mut imcp_frame_buffer = vec![0; 1024];
    let mut imcp = Imcp::new_master(
        receiver,
        sender,
        &mut imcp_rx_buffer,
        &mut imcp_frame_buffer,
    );

    let mut wire_rx_buffer = vec![0; 1024];
    let mut wire_frame_buffer = vec![0; 1024];
    let mut wire_parser = FrameParser::new(&mut wire_rx_buffer, &mut wire_frame_buffer);

    if master_args.stdin {
        run_master_stdin(&mut imcp, &mut wire_parser, &queue, master_args.format)
    } else {
        let port_name = master_args
            .port
            .ok_or_else(|| "--port is required unless --stdin is used".to_string())?;
        let mut port = serialport::new(&port_name, master_args.baud)
            .timeout(Duration::from_millis(100))
            .dtr_on_open(true)
            .open()
            .map_err(|error| format!("failed to open port {port_name}: {error}"))?;

        log::info!(
            "Running as IMCP master on {} at {} baud...",
            port_name,
            master_args.baud
        );

        let mut serial_buf = vec![0; 1024];
        loop {
            match port.read(serial_buf.as_mut_slice()) {
                Ok(bytes_read) if bytes_read > 0 => {
                    log::debug!("read uart: {} {:?}", bytes_read, &serial_buf[..bytes_read]);
                    let mut transmit = |bytes: &[u8]| {
                        port.write_all(bytes)
                            .map_err(|error| format!("port write error: {error}"))?;
                        port.flush()
                            .map_err(|error| format!("port flush error: {error}"))?;
                        print_bytes(master_args.format, "tx", bytes);
                        Ok(())
                    };
                    process_master_bytes(
                        &mut imcp,
                        &mut wire_parser,
                        &queue,
                        &serial_buf[..bytes_read],
                        master_args.format,
                        &mut transmit,
                    )?;
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::TimedOut => {}
                Err(error) => return Err(format!("port read error: {error}")),
            }
        }
    }
}

fn run_master_stdin(
    imcp: &mut Imcp<'_, '_, FrameQueueReceiver, FrameQueueSender>,
    wire_parser: &mut FrameParser<'_, '_>,
    queue: &FrameQueue,
    format: OutputFormat,
) -> Result<(), String> {
    if io::stdin().is_terminal() {
        return Err("--stdin requires a non-interactive stdin pipeline".to_string());
    }

    for line in io::stdin().lock().lines() {
        let line = line.map_err(|error| format!("stdin read error: {error}"))?;
        let trimmed_line = line.trim();
        if trimmed_line.is_empty() {
            continue;
        }

        let bytes = match hex::decode(trimmed_line) {
            Ok(bytes) => bytes,
            Err(error) => {
                print_error(format, "input", &error);
                continue;
            }
        };
        let mut transmit = |bytes: &[u8]| {
            print_bytes(format, "tx", bytes);
            Ok(())
        };
        process_master_bytes(imcp, wire_parser, queue, &bytes, format, &mut transmit)?;
    }

    Ok(())
}

fn process_master_bytes(
    imcp: &mut Imcp<'_, '_, FrameQueueReceiver, FrameQueueSender>,
    wire_parser: &mut FrameParser<'_, '_>,
    queue: &FrameQueue,
    bytes: &[u8],
    format: OutputFormat,
    transmit: &mut impl FnMut(&[u8]) -> Result<(), String>,
) -> Result<(), String> {
    if let Err(error) = wire_parser.write_data(bytes) {
        print_error(format, "rx", &error);
        return Ok(());
    }

    while let Some(parsed_frame) = wire_parser.next_frame() {
        let frame = match parsed_frame {
            Ok(frame) => frame,
            Err(error) => {
                print_error(format, "rx", &error);
                continue;
            }
        };

        print_frame(format, "rx", &frame);

        // The CLI has already validated the wire frame with its diagnostic parser.
        // Re-encoding it lets the protocol state machine process exactly one frame,
        // even when a serial read contains several frames or an unrelated frame.
        let mut encoded = [0u8; MAX_ENCODED_FRAME_SIZE];
        let encoded_len = frame
            .encode(&mut encoded)
            .map_err(|error| format!("failed to re-encode received frame: {error:?}"))?;
        match block_on(imcp.read_tick(&encoded[..encoded_len])) {
            Ok(Some(_)) | Ok(None) => {}
            Err(error) => print_error(format, "protocol", &error),
        }

        while queue_has_frames(queue) {
            let encoded = block_on(imcp.write_tick())
                .map_err(|error| format!("failed to encode master response: {error:?}"))?;
            transmit(encoded.as_slice())?;
        }
    }

    Ok(())
}

fn queue_has_frames(queue: &FrameQueue) -> bool {
    !queue.borrow().is_empty()
}

#[cfg(test)]
mod tests {
    use super::{
        Commands, GlobalOptions, MasterArgs, OutputFormat, PackArgs, process_master_bytes,
    };
    use clap::Parser;
    use imcp::{
        Imcp,
        frame::{Address, Frame, FramePayload, MAX_ENCODED_FRAME_SIZE},
        parser::FrameParser,
    };
    use std::{cell::RefCell, collections::VecDeque, rc::Rc};

    fn encode(frame: &Frame) -> Vec<u8> {
        let mut buffer = [0u8; MAX_ENCODED_FRAME_SIZE];
        let size = frame.encode(&mut buffer).expect("test frame fits");
        buffer[..size].to_vec()
    }

    fn decode(bytes: &[u8]) -> Frame {
        let mut rx_buffer = vec![0; MAX_ENCODED_FRAME_SIZE];
        let mut frame_buffer = vec![0; MAX_ENCODED_FRAME_SIZE];
        let mut parser = FrameParser::new(&mut rx_buffer, &mut frame_buffer);
        parser.write_data(bytes).expect("test bytes fit");
        parser
            .next_frame()
            .expect("test frame exists")
            .expect("test frame is valid")
    }

    #[test]
    fn parses_pack_arguments_with_hex_addresses_and_id() {
        let parsed = GlobalOptions::try_parse_from([
            "imcp-cli",
            "pack",
            "--from",
            "0x01",
            "--to",
            "0x02",
            "--packet-type",
            "join",
            "--id",
            "0xCAFEBABE",
        ]);

        assert!(matches!(
            parsed,
            Ok(GlobalOptions {
                command: Commands::Pack(PackArgs {
                    from: 0x01,
                    to: Some(0x02),
                    id: Some(0xCAFE_BABE),
                    ..
                }),
                ..
            })
        ));
    }

    #[test]
    fn parses_master_stdin_json_arguments() {
        let parsed =
            GlobalOptions::try_parse_from(["imcp-cli", "master", "--stdin", "--format", "json"])
                .expect("master args should parse");

        assert!(matches!(
            parsed.command,
            Commands::Master(MasterArgs {
                stdin: true,
                format: OutputFormat::Json,
                ..
            })
        ));
    }

    #[test]
    fn master_emits_set_address_for_join() {
        let queue = Rc::new(RefCell::new(VecDeque::new()));
        let sender = super::FrameQueueSender(Rc::clone(&queue));
        let receiver = super::FrameQueueReceiver(Rc::clone(&queue));
        let imcp_rx_buffer = Box::leak(Box::new([0u8; 128]));
        let imcp_frame_buffer = Box::leak(Box::new([0u8; 128]));
        let mut imcp = Imcp::new_master(receiver, sender, imcp_rx_buffer, imcp_frame_buffer);
        let wire_rx_buffer = Box::leak(Box::new([0u8; 128]));
        let wire_frame_buffer = Box::leak(Box::new([0u8; 128]));
        let mut wire_parser = FrameParser::new(wire_rx_buffer, wire_frame_buffer);
        let join = encode(&Frame::new(
            Address::Unicast(0x01),
            0x00,
            FramePayload::Join(0xCAFE_BABE),
        ));
        let mut transmitted = Vec::new();
        let mut transmit = |bytes: &[u8]| {
            transmitted.push(bytes.to_vec());
            Ok(())
        };

        process_master_bytes(
            &mut imcp,
            &mut wire_parser,
            &queue,
            &join,
            super::OutputFormat::Debug,
            &mut transmit,
        )
        .expect("master should process join");

        assert_eq!(transmitted.len(), 1);
        assert_eq!(
            decode(&transmitted[0]).payload(),
            &FramePayload::SetAddress {
                address: 0x02,
                id: 0xCAFE_BABE,
            }
        );
    }

    #[test]
    fn master_emits_pong_for_ping() {
        let queue = Rc::new(RefCell::new(VecDeque::new()));
        let sender = super::FrameQueueSender(Rc::clone(&queue));
        let receiver = super::FrameQueueReceiver(Rc::clone(&queue));
        let mut imcp = Imcp::new_master(
            receiver,
            sender,
            Box::leak(Box::new([0u8; 128])),
            Box::leak(Box::new([0u8; 128])),
        );
        let mut wire_parser = FrameParser::new(
            Box::leak(Box::new([0u8; 128])),
            Box::leak(Box::new([0u8; 128])),
        );
        let ping = encode(&Frame::new(
            Address::Unicast(0x01),
            0x02,
            FramePayload::Ping,
        ));
        let mut transmitted = Vec::new();
        let mut transmit = |bytes: &[u8]| {
            transmitted.push(bytes.to_vec());
            Ok(())
        };

        process_master_bytes(
            &mut imcp,
            &mut wire_parser,
            &queue,
            &ping,
            super::OutputFormat::Debug,
            &mut transmit,
        )
        .expect("master should process ping");

        assert_eq!(transmitted.len(), 1);
        let pong = decode(&transmitted[0]);
        assert_eq!(pong.to_address(), Address::Unicast(0x02));
        assert_eq!(pong.from_address(), 0x01);
        assert_eq!(pong.payload(), &FramePayload::Pong);
    }
}
