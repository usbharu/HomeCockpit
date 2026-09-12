#![no_std]
#![no_main]

mod packetization;
mod transport;

use defmt::{debug, info, warn};
use embassy_executor::Spawner;
use embassy_futures::select::select;
use embassy_rp::{
    Peri, bind_interrupts,
    gpio::{Input, Level, Output},
    pac::UART0,
    peripherals::{UART0, USB},
    uart::{BufferedUart, Config},
    usb::{Driver as UsbDriver, InterruptHandler as UsbInterruptHandler},
};
#[cfg(feature = "rp2040")]
use embassy_rp::{
    clocks::RoscRng,
    flash::{Blocking, Flash},
    peripherals::FLASH,
};
#[cfg(feature = "rp235x")]
use embassy_rp::{
    otp,
    peripherals::{FLASH, TRNG},
    trng::{Config as TrngConfig, InterruptHandler as TrngInterruptHandler, Trng},
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, mutex::Mutex};
use embassy_time::Timer;
use embassy_usb::class::cdc_acm::{CdcAcmClass, State as CdcState};
use embassy_usb::{Builder, Config as UsbConfig, UsbDevice};
use hcp::{Capabilities, DeviceKind, Version};
use homecockpit_firmware_base::{
    DeviceDescriptor, DeviceRuntimeState, FEATURE_CONTROL_EVENTS, build_button_control_event,
    build_device_hello_packet, control_id_from_matrix_position, encode_set_frame,
    try_assign_address_from_frame,
};
use imcp::{Imcp, frame::Frame};
use imcp_embassy::{EmbassyReceiver, EmbassySender, new};
use imcp_embedded::{ImcpEmbedded, RpUartCarrierSense};
use static_cell::StaticCell;
use transport::{ImcpTransport, ReadEvent, USB_MAX_PACKET_SIZE, WriteEvent};
use {defmt_rtt as _, panic_probe as _};

const BAUD_RATE: u32 = 115200;
// Development-only USB identity. Replace before shipping production firmware.
const USB_VENDOR_ID: u16 = 0xc0de;
const USB_PRODUCT_ID: u16 = 0xcafe;
const CONTROL_MATRIX_COLUMNS: u8 = 5;
const CONTROL_MATRIX_ROWS: u8 = 8;
#[cfg(feature = "rp2040")]
const FLASH_SIZE: usize = 2 * 1024 * 1024;

static RESULT: Mutex<CriticalSectionRawMutex, [[Level; 5]; 8]> = Mutex::new([[Level::Low; 5]; 8]);
static DEVICE_STATE: Mutex<CriticalSectionRawMutex, DeviceRuntimeState> =
    Mutex::new(DeviceRuntimeState::new());

static FRAME_CHANNEL: Channel<CriticalSectionRawMutex, Frame, 5> = Channel::new();

static RX_BUFFER_CELL: StaticCell<[u8; 128]> = StaticCell::new();
static PARSER_FRAME_BUFFER_CELL: StaticCell<[u8; 64]> = StaticCell::new();
static USB_RX_BUFFER_CELL: StaticCell<[u8; USB_MAX_PACKET_SIZE]> = StaticCell::new();
static USB_CONFIG_DESCRIPTOR_CELL: StaticCell<[u8; 256]> = StaticCell::new();
static USB_BOS_DESCRIPTOR_CELL: StaticCell<[u8; 256]> = StaticCell::new();
static USB_CONTROL_BUFFER_CELL: StaticCell<[u8; 64]> = StaticCell::new();
static CDC_STATE_CELL: StaticCell<CdcState> = StaticCell::new();

bind_interrupts!(struct Irqs {
    UART0_IRQ => embassy_rp::uart::BufferedInterruptHandler<UART0>;
    USBCTRL_IRQ => UsbInterruptHandler<USB>;
    #[cfg(feature = "rp235x")]
    TRNG_IRQ => TrngInterruptHandler<TRNG>;
});

#[derive(Clone, Copy)]
struct DeviceIdentity {
    device_id: u64,
    join_id: u32,
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let mut tx_buffer: [u8; 64] = [0; 64];
    let mut rx_buffer: [u8; 64] = [0; 64];

    let p = embassy_rp::init(Default::default());
    #[cfg(feature = "rp2040")]
    let device_identity = initialize_device_identity(p.FLASH);
    #[cfg(feature = "rp235x")]
    let device_identity = initialize_device_identity(p.FLASH, p.TRNG);

    let usb_driver = UsbDriver::new(p.USB, Irqs);
    let mut usb_config = UsbConfig::new(USB_VENDOR_ID, USB_PRODUCT_ID);
    usb_config.manufacturer = Some("HomeCockpit");
    usb_config.product = Some("Upper Panel DDI");
    usb_config.max_power = 100;
    usb_config.max_packet_size_0 = USB_MAX_PACKET_SIZE as u8;

    let usb_config_descriptor = USB_CONFIG_DESCRIPTOR_CELL.init([0; 256]);
    let usb_bos_descriptor = USB_BOS_DESCRIPTOR_CELL.init([0; 256]);
    let usb_control_buf = USB_CONTROL_BUFFER_CELL.init([0; 64]);
    let cdc_state = CDC_STATE_CELL.init(CdcState::new());
    let mut usb_builder = Builder::new(
        usb_driver,
        usb_config,
        usb_config_descriptor,
        usb_bos_descriptor,
        &mut [],
        usb_control_buf,
    );
    let cdc_class = CdcAcmClass::new(&mut usb_builder, cdc_state, USB_MAX_PACKET_SIZE as u16);
    let (usb_sender, usb_receiver) = cdc_class.split();
    let usb_receiver =
        usb_receiver.into_buffered(USB_RX_BUFFER_CELL.init([0; USB_MAX_PACKET_SIZE]));
    let usb_device = usb_builder.build();

    spawner.spawn(usb_task(usb_device).expect("failed spawn usb_task"));

    let outputs: [Output<'static>; 8] = [
        Output::new(p.PIN_2, Level::Low),
        Output::new(p.PIN_3, Level::Low),
        Output::new(p.PIN_4, Level::Low),
        Output::new(p.PIN_5, Level::Low),
        Output::new(p.PIN_6, Level::Low),
        Output::new(p.PIN_7, Level::Low),
        Output::new(p.PIN_8, Level::Low),
        Output::new(p.PIN_9, Level::Low),
    ];

    let inputs: [Input<'static>; 5] = [
        Input::new(p.PIN_10, embassy_rp::gpio::Pull::Down),
        Input::new(p.PIN_11, embassy_rp::gpio::Pull::Down),
        Input::new(p.PIN_12, embassy_rp::gpio::Pull::Down),
        Input::new(p.PIN_13, embassy_rp::gpio::Pull::Down),
        Input::new(p.PIN_14, embassy_rp::gpio::Pull::Down),
    ];

    spawner.spawn(scan_matrix(inputs, outputs).expect("failed spawn scan_matrix"));

    let mut old: [[Level; 5]; 8] = [[Level::Low; 5]; 8];

    let mut config = Config::default();
    config.baudrate = BAUD_RATE;

    let uart = BufferedUart::new(
        p.UART0,
        p.PIN_0,
        p.PIN_1, // TX, RX ピン
        Irqs,
        &mut tx_buffer,
        &mut rx_buffer, // DMAチャンネル
        config,
    );

    let imcp_embedded = ImcpEmbedded::new(
        RpUartCarrierSense::new(uart, UART0),
        None::<Output>,
        BAUD_RATE,
    )
    .expect("failed init imcp embedded");

    let rx_buffer = RX_BUFFER_CELL.init([0; 128]);
    let parser_frame_buffer = PARSER_FRAME_BUFFER_CELL.init([0; 64]);

    let sender = FRAME_CHANNEL.sender();
    let sender2 = sender;

    let (tx_sender, tx_receiver) = new(sender, FRAME_CHANNEL.receiver());

    let imcp = Imcp::new_client(tx_receiver, tx_sender, rx_buffer, parser_frame_buffer);
    let imcp_transport = ImcpTransport::new(imcp_embedded, usb_sender, usb_receiver);

    spawner
        .spawn(imcp_task(imcp, imcp_transport, device_identity).expect("failed spawn imcp_task"));

    loop {
        if let Ok(g) = RESULT.try_lock() {
            for (r_index, (g_row, o_row)) in g.iter().zip(old.iter()).enumerate() {
                for (c_index, (g_col, o_col)) in g_row.iter().zip(o_row.iter()).enumerate() {
                    if g_col != o_col {
                        info!("r:{} c{} {} → {}", r_index, c_index, o_col, g_col);
                        enqueue_control_event(
                            &sender2,
                            r_index as u8,
                            c_index as u8,
                            bool::from(*g_col),
                        );
                    }
                }
            }
            old = *g;
        }
        Timer::after_millis(5).await;
    }
}

type UsbDriverType = UsbDriver<'static, USB>;
type UsbDeviceType = UsbDevice<'static, UsbDriverType>;

#[embassy_executor::task]
async fn usb_task(mut usb: UsbDeviceType) -> ! {
    usb.run().await
}

#[embassy_executor::task]
async fn scan_matrix(inputs: [Input<'static>; 5], mut outputs: [Output<'static>; 8]) {
    loop {
        for (index, ele) in outputs.iter_mut().enumerate() {
            ele.set_high();

            Timer::after_millis(1).await;

            for (index2, ele) in inputs.iter().enumerate() {
                if let Ok(mut g) = RESULT.try_lock() {
                    g[index][index2] = ele.get_level();
                }
            }

            ele.set_low();
            Timer::after_millis(1).await;
        }
    }
}
#[embassy_executor::task]
async fn imcp_task(
    mut imcp: Imcp<
        'static,
        'static,
        EmbassyReceiver<'static, CriticalSectionRawMutex, 5>,
        EmbassySender<'static, CriticalSectionRawMutex, 5>,
    >,
    mut imcp_transport: ImcpTransport,
    device_identity: DeviceIdentity,
) {
    let mut read_buffer = [0u8; USB_MAX_PACKET_SIZE];
    let tx_sender = FRAME_CHANNEL.sender();

    reset_device_runtime_state().await;
    imcp.restart_client(device_identity.join_id)
        .await
        .unwrap_or_else(|e| warn!("client restart error {:?}", e));

    loop {
        match select(imcp_transport.read(&mut read_buffer), imcp.write_tick()).await {
            embassy_futures::select::Either::First(ReadEvent::Data(s)) => {
                debug!("imcp rx {} bytes: {:?}", s, &read_buffer[..s]);
                let frame = imcp.read_tick(&read_buffer[..s]).await.unwrap_or_else(|e| {
                    warn!("failed parse frame {:?}, bytes: {:?}", e, &read_buffer[..s]);
                    None
                });
                if let Some(frame) = frame {
                    handle_incoming_frame(&tx_sender, &frame, device_identity.device_id);
                }
            }
            embassy_futures::select::Either::First(ReadEvent::UsbConnected) => {
                info!("usb cdc connected; switching IMCP transport");
                reset_device_runtime_state().await;
                imcp.restart_client(device_identity.join_id)
                    .await
                    .unwrap_or_else(|e| warn!("usb restart error {:?}", e));
            }
            embassy_futures::select::Either::First(ReadEvent::UsbDisconnected) => {
                info!("usb cdc disconnected; falling back to UART");
                reset_device_runtime_state().await;
                imcp.restart_client(device_identity.join_id)
                    .await
                    .unwrap_or_else(|e| warn!("UART restart error {:?}", e));
            }
            embassy_futures::select::Either::First(ReadEvent::UartError) => {
                warn!("UART transport read error")
            }
            embassy_futures::select::Either::First(ReadEvent::UsbError) => {
                warn!("USB transport read error")
            }
            embassy_futures::select::Either::Second(Ok(v)) => {
                match imcp_transport.write_frame(&v).await {
                    WriteEvent::Sent => info!("write {:?}", v),
                    WriteEvent::UsbDisconnected => {
                        info!("usb cdc disconnected during write; falling back to UART");
                        reset_device_runtime_state().await;
                        imcp.restart_client(device_identity.join_id)
                            .await
                            .unwrap_or_else(|e| warn!("UART restart error {:?}", e));
                    }
                    WriteEvent::UartError => warn!("UART transport write error"),
                    WriteEvent::UsbError => warn!("USB transport write error"),
                }
            }
            embassy_futures::select::Either::Second(Err(e)) => warn!("write error {:?}", e),
        }

        Timer::after_millis(50).await;

        read_buffer = [0u8; USB_MAX_PACKET_SIZE];
    }
}

async fn reset_device_runtime_state() {
    let mut state = DEVICE_STATE.lock().await;
    *state = DeviceRuntimeState::new();
}

fn enqueue_control_event(
    sender: &embassy_sync::channel::Sender<'static, CriticalSectionRawMutex, Frame, 5>,
    row: u8,
    column: u8,
    pressed: bool,
) {
    let frame = if let Ok(mut state) = DEVICE_STATE.try_lock() {
        let Some(address) = state.address() else {
            return;
        };
        let control_id = control_id_from_matrix_position(row, column, CONTROL_MATRIX_COLUMNS);
        match build_button_control_event(&mut state, control_id, pressed)
            .and_then(|packet| encode_set_frame(address, &packet))
        {
            Ok(frame) => Some(frame),
            Err(e) => {
                warn!("failed build control event {:?}", e);
                None
            }
        }
    } else {
        None
    };

    if let Some(frame) = frame
        && let Err(e) = sender.try_send(frame)
    {
        warn!("failed queue control event {:?}", e);
    }
}

fn device_descriptor() -> DeviceDescriptor {
    DeviceDescriptor {
        device_id: 0,
        device_kind: DeviceKind::UpperPanelDdi,
        firmware_version: Version {
            major: 0,
            minor: 1,
            patch: 0,
        },
        capabilities: Capabilities {
            displays: 0,
            controls: u16::from(CONTROL_MATRIX_ROWS) * u16::from(CONTROL_MATRIX_COLUMNS),
            features: FEATURE_CONTROL_EVENTS,
        },
    }
}

fn handle_incoming_frame(
    sender: &embassy_sync::channel::Sender<'static, CriticalSectionRawMutex, Frame, 5>,
    frame: &Frame,
    device_id: u64,
) {
    let address = if let Ok(mut state) = DEVICE_STATE.try_lock() {
        try_assign_address_from_frame(&mut state, frame)
    } else {
        None
    };

    if let Some(address) = address {
        let hello = build_device_hello_packet(DeviceDescriptor {
            device_id,
            ..device_descriptor()
        });
        match encode_set_frame(address, &hello) {
            Ok(frame) => {
                if let Err(e) = sender.try_send(frame) {
                    warn!("failed queue device hello {:?}", e);
                }
            }
            Err(e) => warn!("failed encode device hello {:?}", e),
        }
    }
}

#[cfg(feature = "rp2040")]
fn initialize_device_identity(flash: Peri<'static, FLASH>) -> DeviceIdentity {
    let device_id = read_rp2040_device_id(flash);
    let join_id = generate_rp2040_join_id();
    DeviceIdentity { device_id, join_id }
}

#[cfg(feature = "rp235x")]
fn initialize_device_identity(_flash: FLASH, trng: TRNG) -> DeviceIdentity {
    let device_id = read_rp235x_device_id();
    let join_id = generate_rp235x_join_id(trng);
    DeviceIdentity { device_id, join_id }
}

#[cfg(feature = "rp2040")]
fn read_rp2040_device_id(flash: Peri<'static, FLASH>) -> u64 {
    let mut flash = Flash::<_, Blocking, FLASH_SIZE>::new_blocking(flash);
    let mut unique_id = [0u8; 8];
    match flash.blocking_unique_id(&mut unique_id) {
        Ok(()) => u64::from_be_bytes(unique_id),
        Err(error) => {
            warn!("failed read flash unique id {:?}", error);
            0
        }
    }
}

#[cfg(feature = "rp235x")]
fn read_rp235x_device_id() -> u64 {
    match otp::get_chipid() {
        Ok(chip_id) => chip_id,
        Err(error) => {
            warn!("failed read chip id {:?}", error);
            0
        }
    }
}

#[cfg(feature = "rp2040")]
fn generate_rp2040_join_id() -> u32 {
    let mut rng = RoscRng;
    rng.next_u32()
}

#[cfg(feature = "rp235x")]
fn generate_rp235x_join_id(trng: TRNG) -> u32 {
    let mut trng = Trng::new(trng, Irqs, TrngConfig::default());
    trng.blocking_next_u32()
}
