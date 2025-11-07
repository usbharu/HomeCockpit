#![no_std]
#![no_main]

use defmt::{info, warn};
use embassy_executor::Spawner;
use embassy_futures::select::select;
use embassy_rp::{
    bind_interrupts,
    gpio::{Input, Level, Output},
    peripherals::UART0,
    uart::{BufferedUart, Config},
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, mutex::Mutex};
use embassy_time::Timer;
use embedded_io_async::{Read, Write};
use heapless::Vec;
use imcp::{Imcp, frame::Frame};
use imcp_embassy::{EmbassyReceiver, EmbassySender, new};
use imcp_embedded::ImcpEmbedded;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

const BAUD_RATE: u32 = 115200;

static RESULT: Mutex<CriticalSectionRawMutex, [[Level; 5]; 4]> = Mutex::new([[Level::Low; 5]; 4]);

static FRAME_CHANNEL: Channel<CriticalSectionRawMutex, Frame, 5> = Channel::new();

static RX_BUFFER_CELL: StaticCell<[u8; 128]> = StaticCell::new();
static PARSER_FRAME_BUFFER_CELL: StaticCell<[u8; 64]> = StaticCell::new();

bind_interrupts!(struct Irqs {
    UART0_IRQ => embassy_rp::uart::BufferedInterruptHandler<UART0>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let mut TX_BUFFER: [u8; 64] = [0; 64];
    let mut RX_BUFFER: [u8; 64] = [0; 64];

    let p = embassy_rp::init(Default::default());

    let outputs: [Output<'static>; 4] = [
        Output::new(p.PIN_2, Level::Low),
        Output::new(p.PIN_3, Level::Low),
        Output::new(p.PIN_4, Level::Low),
        Output::new(p.PIN_5, Level::Low),
    ];

    let inputs: [Input<'static>; 5] = [
        Input::new(p.PIN_6, embassy_rp::gpio::Pull::Down),
        Input::new(p.PIN_7, embassy_rp::gpio::Pull::Down),
        Input::new(p.PIN_8, embassy_rp::gpio::Pull::Down),
        Input::new(p.PIN_9, embassy_rp::gpio::Pull::Down),
        Input::new(p.PIN_10, embassy_rp::gpio::Pull::Down),
    ];

    spawner.spawn(scan_matrix(inputs, outputs).expect("failed spawn scan_matrix"));

    let mut old: [[Level; 5]; 4] = [[Level::Low; 5]; 4];

    let mut config = Config::default();
    config.baudrate = BAUD_RATE;

    let uart = BufferedUart::new(
        p.UART0,
        p.PIN_0,
        p.PIN_1, // TX, RX ピン
        Irqs,
        &mut TX_BUFFER,
        &mut RX_BUFFER, // DMAチャンネル
        config,
    );

    let imcp_embedded =
        ImcpEmbedded::new(uart, None::<Output>, BAUD_RATE).expect("failed init imcp embedded");

    let rx_buffer = RX_BUFFER_CELL.init([0; 128]);
    let parser_frame_buffer = PARSER_FRAME_BUFFER_CELL.init([0; 64]);

    let sender = FRAME_CHANNEL.sender();
    let sender2 = sender;

    let (tx_sender, tx_receiver) = new(sender, FRAME_CHANNEL.receiver());

    let imcp = Imcp::new_master(tx_receiver, tx_sender, rx_buffer, parser_frame_buffer);

    spawner.spawn(imcp_task(imcp, imcp_embedded).expect("failed spawn imcp_task"));

    loop {
        if let Ok(g) = RESULT.try_lock() {
            for (r_index, (g_row, o_row)) in g.iter().zip(old.iter()).enumerate() {
                for (c_index, (g_col, o_col)) in g_row.iter().zip(o_row.iter()).enumerate() {
                    if g_col != o_col {
                        info!("r:{} c{} {} → {}", r_index, c_index, o_col, g_col);
                        sender2
                            .try_send(Frame::new(
                                imcp::frame::Address::Broadcast,
                                0x00,
                                imcp::frame::FramePayload::Data(Vec::from_array([
                                    r_index as u8,
                                    c_index as u8,
                                    bool::from(*o_col).into(),
                                    bool::from(*g_col).into(),
                                ])),
                            ))
                            .unwrap_or_else(|e| warn!("failed send ping {:?}", e));
                    }
                }
            }
            old = *g;
        }
        Timer::after_millis(5).await;
    }
}

#[embassy_executor::task]
async fn scan_matrix(inputs: [Input<'static>; 5], mut outputs: [Output<'static>; 4]) {
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
    mut imcp_embedded: ImcpEmbedded<BufferedUart, Output<'static>>,
) {
    let mut read_buffer = [0u8; 16];

    loop {
        match select(imcp_embedded.read(&mut read_buffer), imcp.write_tick()).await {
            embassy_futures::select::Either::First(Ok(s)) => {
                imcp.read_tick(&read_buffer).await.unwrap_or_else(|e| {
                    warn!("failed parse frame{:?}", e);
                    None
                });
                info!("read: {}", s)
            }
            embassy_futures::select::Either::First(Err(e)) => warn!("read error {:?}", e),
            embassy_futures::select::Either::Second(Ok(v)) => {
                imcp_embedded.write(&v).await.unwrap_or_else(|e| {
                    warn!("uart write error {:?}", e);
                    0
                });
                info!("write {:?}", v);
                imcp_embedded
                    .flush()
                    .await
                    .unwrap_or_else(|e| warn!("uart flush error {:?}", e));
            }
            embassy_futures::select::Either::Second(Err(e)) => warn!("write error {:?}", e),
        }

        Timer::after_millis(50).await;

        read_buffer = [0u8; 16];
    }
}
