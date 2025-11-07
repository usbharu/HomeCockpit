use clap::{Args, Parser, Subcommand, ValueEnum, command};
use imcp::frame::{Address, Frame, FramePayload};

#[derive(Parser, Debug)]
#[command(version, about = "imcp-cli", long_about = None)]
struct GlobalOptions {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    ///pack: オプションからFrameを作成します。
    Pack(PackArgs),
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

    match cli.command {
        Commands::Pack(pack_args) => {
            pack(pack_args);
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
