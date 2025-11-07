use std::{
    io::{self, BufRead, IsTerminal},
    process,
    time::Duration,
};

use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum, command};
use imcp::{
    frame::{Address, Frame, FramePayload},
    parser::FrameParser,
};
use log::LevelFilter;

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
    ///pack: オプションからFrameを作成します。
    Pack(PackArgs),
    //unpack: 標準入力または--dataオプションをパースします。
    Unpack(UnpackArgs),
    /// watch: シリアルポートを監視し、受信データをunpackします。
    Watch(WatchArgs),
}

#[derive(Args, Debug)]
struct PackArgs {
    #[arg(short, long,value_parser=clap_num::maybe_hex::<u8>)]
    from: u8,
    #[arg(short, long,value_parser=clap_num::maybe_hex::<u8>,conflicts_with = "broadcast",
        // "broadcast" が指定されていない場合は、この "to" が必須
        required_unless_present = "broadcast")]
    to: Option<u8>,
    #[arg(short, long)]
    broadcast: bool,

    #[arg(short = 'p', long = "packet-type", value_enum)]
    packet_type: PacketType,

    #[arg(long)]
    id: Option<u32>,
    #[arg(long)]
    address: Option<u8>,
    #[arg(long)]
    data: Option<String>,
}

#[derive(Args, Debug)]
struct UnpackArgs {
    #[arg(long)]
    data: Option<String>,
}

#[derive(Args, Debug)]
struct WatchArgs {
    /// 監視するシリアルポート (例: "COM3" or "/dev/ttyUSB0")
    #[arg(short, long)]
    port: String,

    /// ボーレート (デフォルト: 9600)
    #[arg(short, long, default_value_t = 9600)]
    baud: u32,

    #[arg(short, long)]
    list: bool,
}

#[derive(ValueEnum, Clone, Debug)]
#[value(rename_all = "lower")]
enum PacketType {
    Ping,
    Pong,
    Ack,
    Join,
    SetAddress,
    Data,
    Set,
}

fn main() {
    let cli = GlobalOptions::parse();

    let log_level = match cli.verbose {
        0 => LevelFilter::Warn,  // デフォルト
        1 => LevelFilter::Info,  // -v
        2 => LevelFilter::Debug, // -vv
        _ => LevelFilter::Trace, // -vvv 以上
    };

    // ロガーを初期化（env_loggerの例）
    env_logger::Builder::new().filter_level(log_level).init();

    match cli.command {
        Commands::Pack(pack_args) => {
            pack(pack_args);
        }
        Commands::Unpack(unpack_args) => unpack(unpack_args),
        Commands::Watch(watch_args) => watch(watch_args),
    }
}

fn handle_line(bytes: &[u8], frame_parser: &mut FrameParser) {
    // 0バイトなら何もしない
    if bytes.is_empty() {
        return;
    }

    frame_parser.write_data(bytes).unwrap();

    while let Some(frame) = frame_parser.next_frame() {
        match frame {
            Ok(a) => {
                
                println!("{:?}", a);
            }
            Err(e) => {
                log::warn!("{:?}", e)
            }
        }
    }
}

fn watch(watch_args: WatchArgs) {
    if watch_args.list {
        let ports = serialport::available_ports().expect("No ports found.");

        for ele in ports {
            log::info!("{} {:?}", ele.port_name, ele.port_type);
        }
    }

    let port = serialport::new(&watch_args.port, watch_args.baud)
        .timeout(Duration::from_secs(1)) // タイムアウトを短く
        .dtr_on_open(true)
        .open();

    let mut rx_buffer = vec![0; 1024];
    let mut frame_buffer = vec![0; 1024];

    let mut frame_parser = FrameParser::new(&mut rx_buffer, &mut frame_buffer);

    match port {
        Ok(mut port) => {
            log::info!(
                "Watching port {} at {} baud...",
                watch_args.port, watch_args.baud
            );
            let mut serial_buf: Vec<u8> = vec![0; 1024]; // 読み取りバッファ

            loop {
                match port.read(serial_buf.as_mut_slice()) {
                    Ok(bytes_read) => {
                        log::debug!("read uart: {} {:?}", bytes_read, &serial_buf[..bytes_read]);
                        if bytes_read > 0 {
                            // 読み取った生のバイナリ (&[u8]) をそのまま渡す
                            handle_line(&serial_buf[..bytes_read], &mut frame_parser);
                        }
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::TimedOut => {
                        // タイムアウトは正常動作
                        continue;
                    }
                    Err(e) => {
                        log::warn!("Port reading error: {}", e);
                        break;
                    }
                }
            }
        }
        Err(e) => {
            log::error!("Error opening port {}: {}", watch_args.port, e);
            process::exit(1);
        }
    }
}

fn unpack(unpack_args: UnpackArgs) {
    // reader は Hex文字列 のイテレータ (Result<String, ...>)
    let reader: Box<dyn Iterator<Item = Result<String, std::io::Error>>>;

    if let Some(data_str) = unpack_args.data {
        // --data オプション (Hex文字列)
        reader = Box::new(std::iter::once(Ok(data_str)));
    } else {
        // 標準入力 (Hex文字列の行)
        if io::stdin().is_terminal() {
            log::error!("--data or pipeline is required.");
            process::exit(1);
        }
        reader = Box::new(io::stdin().lock().lines());
    }

    let mut rx_buffer = vec![0; 1024];
    let mut frame_buffer = vec![0; 1024];

    let mut frame_parser = FrameParser::new(&mut rx_buffer, &mut frame_buffer);

    // Hex文字列 をイテレート
    for ele in reader {
        match ele {
            Ok(line) => {
                let trimmed_line = line.trim();
                if trimmed_line.is_empty() {
                    continue; // 空行は無視
                }

                // ★ ここで Hex文字列 -> バイト列 にデコード
                match hex::decode(trimmed_line) {
                    Ok(bytes) => {
                        // デコードしたバイト列 (Vec<u8>) を共通ハンドラに渡す
                        handle_line(&bytes, &mut frame_parser);
                    }
                    Err(e) => {
                        // Hex のデコード自体に失敗
                        log::warn!("Hex decode error: {} (Received Line: {})", e, trimmed_line);
                    }
                }
            }
            Err(e) => {
                // 標準入力の読み取りエラー
                log::warn!("Stdin read error: {}", e);
            }
        }
    }
}

fn pack(pack_args: PackArgs) {
    let to_address = if pack_args.broadcast {
        Address::Broadcast
    } else {
        Address::Unicast(pack_args.to.expect("--to or --broadcast is required."))
    };

    let from_address = pack_args.from;

    let frame_payload = match pack_args.packet_type {
        PacketType::Ping => FramePayload::Ping,
        PacketType::Pong => FramePayload::Pong,
        PacketType::Ack => FramePayload::Ack(pack_args.address.expect("--address is required.")),
        PacketType::Join => FramePayload::Join(pack_args.id.expect("--id is required.")),
        PacketType::SetAddress => FramePayload::SetAddress {
            address: pack_args.address.expect("--address is required."),
            id: pack_args.id.expect("--id is required."),
        },
        PacketType::Data => FramePayload::Data(
            hex::decode(pack_args.data.expect("--data is required."))
                .expect("failed to parse hex.")
                .iter()
                .copied()
                .collect(),
        ),
        PacketType::Set => FramePayload::Set(
            hex::decode(pack_args.data.expect("--data is required."))
                .expect("failed to parse hex.")
                .iter()
                .copied()
                .collect(),
        ),
    };

    let frame = Frame::new(to_address, from_address, frame_payload);
    let mut buf = [0u8; 128];
    let size = frame.encode(&mut buf).unwrap();

    let v: Vec<u8> = Vec::from(&buf[0..size]);

    let he = hex::encode_upper(v);

    println!("{}", he);
}
