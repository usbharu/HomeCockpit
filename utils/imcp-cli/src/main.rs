use std::{
    cell::RefCell,
    collections::VecDeque,
    convert::Infallible,
    io::{self, BufRead, IsTerminal, Read},
    process,
    rc::Rc,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
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

    /// 起動時にmasterから送信するエンコード済みフレーム。複数指定できます。
    #[arg(long = "send", value_name = "HEX", action = ArgAction::Append)]
    send: Vec<String>,

    /// シリアル接続中に標準入力から `send HEX` を受け取り送信します。
    #[arg(long, requires = "port", conflicts_with = "stdin")]
    control_stdin: bool,

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

const MASTER_RETRY_INTERVAL: Duration = Duration::from_millis(250);
const MASTER_MAX_RETRIES: u8 = 3;

#[derive(Clone)]
struct FrameQueueSender(FrameQueue);

struct FrameQueueReceiver(FrameQueue);

#[derive(Debug)]
struct FrameQueueEmpty;

#[derive(Debug, Default)]
struct MasterRetryState {
    expected_ack: Option<u8>,
    attempts: u8,
    next_retry_at: Option<Instant>,
}

impl MasterRetryState {
    fn is_due(&self, now: Instant) -> bool {
        self.next_retry_at.is_some_and(|deadline| now >= deadline)
    }

    fn clear(&mut self) {
        self.expected_ack = None;
        self.attempts = 0;
        self.next_retry_at = None;
    }

    fn observe_ack(&mut self, address: u8) {
        if self.expected_ack == Some(address) {
            self.clear();
        }
    }

    fn observe_transmit(&mut self, frame: &Frame, now: Instant) {
        match frame.payload() {
            FramePayload::SetAddress { .. } => {
                self.expected_ack = Some(0x00);
                self.attempts = self.attempts.saturating_add(1);
                self.next_retry_at = Some(now + MASTER_RETRY_INTERVAL);
            }
            FramePayload::Join(_) | FramePayload::Set(_) => {
                self.expected_ack = Some(frame.to_address().as_byte());
                self.next_retry_at = None;
            }
            _ => {}
        }
    }
}

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

fn decode_single_wire_bytes(bytes: &[u8]) -> Result<Frame, String> {
    let mut rx_buffer = vec![0; MAX_ENCODED_FRAME_SIZE * 2];
    let mut frame_buffer = vec![0; MAX_ENCODED_FRAME_SIZE * 2];
    let mut parser = FrameParser::new(&mut rx_buffer, &mut frame_buffer);
    parser
        .write_data(bytes)
        .map_err(|error| format!("invalid frame for --send: {error:?}"))?;
    let frame = match parser.next_frame() {
        Some(Ok(frame)) => frame,
        Some(Err(error)) => return Err(format!("invalid frame for --send: {error:?}")),
        None => return Err("--send must contain one complete IMCP frame".to_string()),
    };
    if parser.next_frame().is_some() {
        return Err("--send must contain exactly one IMCP frame".to_string());
    }
    Ok(frame)
}

fn decode_single_wire_frame(hex_data: &str) -> Result<Frame, String> {
    let bytes = hex::decode(hex_data.trim())
        .map_err(|error| format!("invalid frame hex for --send: {error}"))?;
    decode_single_wire_bytes(&bytes)
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
    let format = master_args.format;
    let mut retry_state = MasterRetryState::default();
    let send_frames = master_args
        .send
        .iter()
        .map(|frame| decode_single_wire_frame(frame))
        .collect::<Result<Vec<_>, _>>()?;

    if master_args.stdin {
        run_master_stdin(
            &mut imcp,
            &mut wire_parser,
            &queue,
            format,
            &send_frames,
            &mut retry_state,
        )
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

        enqueue_frames(&queue, &send_frames);
        {
            let mut transmit = |bytes: &[u8]| transmit_serial(&mut *port, format, bytes);
            flush_master_tx(
                &mut imcp,
                &queue,
                &mut retry_state,
                Instant::now(),
                &mut transmit,
            )?;
        }

        let control_receiver = if master_args.control_stdin {
            Some(spawn_control_reader())
        } else {
            None
        };
        let mut serial_buf = vec![0; 1024];
        loop {
            if let Some(receiver) = control_receiver.as_ref() {
                for line in receiver.try_iter() {
                    match line {
                        Ok(line) => {
                            let trimmed = line.trim();
                            if trimmed.is_empty() {
                                continue;
                            }
                            let frame_hex = trimmed.strip_prefix("send ").unwrap_or(trimmed);
                            match decode_single_wire_frame(frame_hex) {
                                Ok(frame) => enqueue_frames(&queue, &[frame]),
                                Err(error) => eprintln!("control input error: {error}"),
                            }
                        }
                        Err(error) => eprintln!("control stdin error: {error}"),
                    }
                }
                let mut transmit = |bytes: &[u8]| transmit_serial(&mut *port, format, bytes);
                flush_master_tx(
                    &mut imcp,
                    &queue,
                    &mut retry_state,
                    Instant::now(),
                    &mut transmit,
                )?;
            }

            match port.read(serial_buf.as_mut_slice()) {
                Ok(bytes_read) if bytes_read > 0 => {
                    log::debug!("read uart: {} {:?}", bytes_read, &serial_buf[..bytes_read]);
                    let mut transmit = |bytes: &[u8]| transmit_serial(&mut *port, format, bytes);
                    process_master_bytes(
                        &mut imcp,
                        &mut wire_parser,
                        &queue,
                        &serial_buf[..bytes_read],
                        format,
                        &mut retry_state,
                        &mut transmit,
                    )?;
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::TimedOut => {}
                Err(error) => return Err(format!("port read error: {error}")),
            }

            if retry_state.is_due(Instant::now()) {
                let mut transmit = |bytes: &[u8]| transmit_serial(&mut *port, format, bytes);
                let can_flush_queue =
                    retry_master_frame(&mut imcp, &mut retry_state, Instant::now(), &mut transmit)?;
                if can_flush_queue {
                    flush_master_tx(
                        &mut imcp,
                        &queue,
                        &mut retry_state,
                        Instant::now(),
                        &mut transmit,
                    )?;
                }
            }
        }
    }
}

fn run_master_stdin(
    imcp: &mut Imcp<'_, '_, FrameQueueReceiver, FrameQueueSender>,
    wire_parser: &mut FrameParser<'_, '_>,
    queue: &FrameQueue,
    format: OutputFormat,
    send_frames: &[Frame],
    retry_state: &mut MasterRetryState,
) -> Result<(), String> {
    if io::stdin().is_terminal() {
        return Err("--stdin requires a non-interactive stdin pipeline".to_string());
    }

    let mut transmit = |bytes: &[u8]| {
        print_bytes(format, "tx", bytes);
        Ok(())
    };
    enqueue_frames(queue, send_frames);
    flush_master_tx(imcp, queue, retry_state, Instant::now(), &mut transmit)?;

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
        process_master_bytes(
            imcp,
            wire_parser,
            queue,
            &bytes,
            format,
            retry_state,
            &mut transmit,
        )?;
    }

    Ok(())
}

fn process_master_bytes(
    imcp: &mut Imcp<'_, '_, FrameQueueReceiver, FrameQueueSender>,
    wire_parser: &mut FrameParser<'_, '_>,
    queue: &FrameQueue,
    bytes: &[u8],
    format: OutputFormat,
    retry_state: &mut MasterRetryState,
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
            Ok(Some(_)) => {
                if let FramePayload::Ack(address) = frame.payload() {
                    retry_state.observe_ack(*address);
                }
            }
            Ok(None) => {}
            Err(error) => print_error(format, "protocol", &error),
        }

        flush_master_tx(imcp, queue, retry_state, Instant::now(), transmit)?;
    }

    Ok(())
}

fn enqueue_frames(queue: &FrameQueue, frames: &[Frame]) {
    queue.borrow_mut().extend(frames.iter().cloned());
}

fn spawn_control_reader() -> mpsc::Receiver<Result<String, String>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let result = line.map_err(|error| error.to_string());
            if sender.send(result).is_err() {
                break;
            }
        }
    });
    receiver
}

fn transmit_serial(
    port: &mut dyn serialport::SerialPort,
    format: OutputFormat,
    bytes: &[u8],
) -> Result<(), String> {
    port.write_all(bytes)
        .map_err(|error| format!("port write error: {error}"))?;
    port.flush()
        .map_err(|error| format!("port flush error: {error}"))?;
    print_bytes(format, "tx", bytes);
    Ok(())
}

fn flush_master_tx(
    imcp: &mut Imcp<'_, '_, FrameQueueReceiver, FrameQueueSender>,
    queue: &FrameQueue,
    retry_state: &mut MasterRetryState,
    now: Instant,
    transmit: &mut impl FnMut(&[u8]) -> Result<(), String>,
) -> Result<(), String> {
    if retry_state.expected_ack.is_some() {
        return Ok(());
    }

    while queue_has_frames(queue) {
        let encoded = block_on(imcp.write_tick())
            .map_err(|error| format!("failed to encode master response: {error:?}"))?;
        let frame = decode_single_wire_bytes(encoded.as_slice())?;
        transmit(encoded.as_slice())?;
        retry_state.observe_transmit(&frame, now);
        if frame_requires_ack(&frame) {
            break;
        }
    }
    Ok(())
}

fn frame_requires_ack(frame: &Frame) -> bool {
    matches!(
        frame.payload(),
        FramePayload::Join(_) | FramePayload::SetAddress { .. } | FramePayload::Set(_)
    )
}

fn retry_master_frame(
    imcp: &mut Imcp<'_, '_, FrameQueueReceiver, FrameQueueSender>,
    retry_state: &mut MasterRetryState,
    now: Instant,
    transmit: &mut impl FnMut(&[u8]) -> Result<(), String>,
) -> Result<bool, String> {
    if retry_state.attempts < MASTER_MAX_RETRIES {
        let encoded = block_on(imcp.write_tick())
            .map_err(|error| format!("failed to encode master retry: {error:?}"))?;
        let frame = decode_single_wire_bytes(encoded.as_slice())?;
        transmit(encoded.as_slice())?;
        retry_state.observe_transmit(&frame, now);
        return Ok(false);
    }

    // Let the core state machine consume its exhausted SetAddress pending
    // frame. It reports an empty queue after clearing that state.
    retry_state.clear();
    match block_on(imcp.write_tick()) {
        Err(imcp::error::ImcpError::ReceiveError(FrameQueueEmpty)) => Ok(true),
        Ok(encoded) => {
            let frame = decode_single_wire_bytes(encoded.as_slice())?;
            transmit(encoded.as_slice())?;
            retry_state.observe_transmit(&frame, now);
            Ok(!frame_requires_ack(&frame))
        }
        Err(error) => Err(format!("failed to expire master retry: {error:?}")),
    }
}

fn queue_has_frames(queue: &FrameQueue) -> bool {
    !queue.borrow().is_empty()
}

#[cfg(test)]
mod tests {
    use super::{
        Commands, GlobalOptions, MasterArgs, MasterRetryState, OutputFormat, PackArgs,
        flush_master_tx, process_master_bytes, retry_master_frame,
    };
    use clap::Parser;
    use imcp::{
        Imcp,
        frame::{Address, Frame, FramePayload, MAX_ENCODED_FRAME_SIZE},
        parser::FrameParser,
    };
    use std::{
        cell::RefCell,
        collections::VecDeque,
        rc::Rc,
        time::{Duration, Instant},
    };

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
        let parsed = GlobalOptions::try_parse_from([
            "imcp-cli",
            "master",
            "--stdin",
            "--format",
            "json",
            "--send",
            "FE020100000003FF",
        ])
        .expect("master args should parse");

        assert!(matches!(
            parsed.command,
            Commands::Master(MasterArgs {
                stdin: true,
                format: OutputFormat::Json,
                send,
                ..
            }) if send == vec!["FE020100000003FF".to_string()]
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
        let transmitted = RefCell::new(Vec::new());
        let mut transmit = |bytes: &[u8]| {
            transmitted.borrow_mut().push(bytes.to_vec());
            Ok(())
        };

        process_master_bytes(
            &mut imcp,
            &mut wire_parser,
            &queue,
            &join,
            super::OutputFormat::Debug,
            &mut MasterRetryState::default(),
            &mut transmit,
        )
        .expect("master should process join");

        assert_eq!(transmitted.borrow().len(), 1);
        let transmitted = transmitted.borrow();
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
        let transmitted = RefCell::new(Vec::new());
        let mut transmit = |bytes: &[u8]| {
            transmitted.borrow_mut().push(bytes.to_vec());
            Ok(())
        };

        process_master_bytes(
            &mut imcp,
            &mut wire_parser,
            &queue,
            &ping,
            super::OutputFormat::Debug,
            &mut MasterRetryState::default(),
            &mut transmit,
        )
        .expect("master should process ping");

        assert_eq!(transmitted.borrow().len(), 1);
        let pong = decode(&transmitted.borrow()[0]);
        assert_eq!(pong.to_address(), Address::Unicast(0x02));
        assert_eq!(pong.from_address(), 0x01);
        assert_eq!(pong.payload(), &FramePayload::Pong);
    }

    #[test]
    fn master_does_not_retry_pending_frame_while_flushing_queue() {
        let queue = Rc::new(RefCell::new(VecDeque::from([
            Frame::new(
                Address::Unicast(0x00),
                0x01,
                FramePayload::SetAddress {
                    address: 0x02,
                    id: 0xCAFE_BABE,
                },
            ),
            Frame::new(Address::Unicast(0x02), 0x01, FramePayload::Ping),
        ])));
        let sender = super::FrameQueueSender(Rc::clone(&queue));
        let receiver = super::FrameQueueReceiver(Rc::clone(&queue));
        let mut imcp = Imcp::new_master(
            receiver,
            sender,
            Box::leak(Box::new([0u8; 128])),
            Box::leak(Box::new([0u8; 128])),
        );
        let mut retry_state = MasterRetryState::default();
        let transmitted = RefCell::new(Vec::new());
        let mut transmit = |bytes: &[u8]| {
            transmitted.borrow_mut().push(bytes.to_vec());
            Ok(())
        };

        flush_master_tx(
            &mut imcp,
            &queue,
            &mut retry_state,
            Instant::now(),
            &mut transmit,
        )
        .expect("master should send the first pending frame");
        flush_master_tx(
            &mut imcp,
            &queue,
            &mut retry_state,
            Instant::now(),
            &mut transmit,
        )
        .expect("master should wait for the pending frame ACK");

        assert_eq!(transmitted.borrow().len(), 1);
        assert_eq!(queue.borrow().len(), 1);
    }

    #[test]
    fn master_retries_set_address_after_lost_ack() {
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
        let join = encode(&Frame::new(
            Address::Unicast(0x01),
            0x00,
            FramePayload::Join(0xCAFE_BABE),
        ));
        let mut retry_state = MasterRetryState::default();
        let transmitted = RefCell::new(Vec::new());
        let mut transmit = |bytes: &[u8]| {
            transmitted.borrow_mut().push(bytes.to_vec());
            Ok(())
        };
        let now = Instant::now();

        process_master_bytes(
            &mut imcp,
            &mut wire_parser,
            &queue,
            &join,
            super::OutputFormat::Debug,
            &mut retry_state,
            &mut transmit,
        )
        .expect("master should process join");
        assert_eq!(transmitted.borrow().len(), 1);

        process_master_bytes(
            &mut imcp,
            &mut wire_parser,
            &queue,
            &join,
            super::OutputFormat::Debug,
            &mut retry_state,
            &mut transmit,
        )
        .expect("master should process duplicate join");
        assert_eq!(transmitted.borrow().len(), 1);

        retry_state.next_retry_at = Some(now - Duration::from_millis(1));
        retry_master_frame(&mut imcp, &mut retry_state, Instant::now(), &mut transmit)
            .expect("master should retry set address");
        assert_eq!(transmitted.borrow().len(), 2);
        assert_eq!(transmitted.borrow()[0], transmitted.borrow()[1]);

        retry_state.next_retry_at = Some(now - Duration::from_millis(1));
        retry_master_frame(&mut imcp, &mut retry_state, Instant::now(), &mut transmit)
            .expect("master should send the final retry");
        assert_eq!(transmitted.borrow().len(), 3);

        retry_state.next_retry_at = Some(now - Duration::from_millis(1));
        let can_flush_queue =
            retry_master_frame(&mut imcp, &mut retry_state, Instant::now(), &mut transmit)
                .expect("master should expire the retry");
        assert!(can_flush_queue);
        assert!(retry_state.expected_ack.is_none());
    }
}
