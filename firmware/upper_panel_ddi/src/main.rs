#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_rp::gpio::{Input, Level, Output};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embassy_time::Timer;
use {defmt_rtt as _, panic_probe as _};

static RESULT: Mutex<CriticalSectionRawMutex, [[Level; 5]; 4]> = Mutex::new([[Level::Low; 5]; 4]);

#[embassy_executor::main]
async fn main(spawner: Spawner) {
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

    spawner.spawn(scan_matrix(inputs, outputs).unwrap());

    let mut old: [[Level; 5]; 4] = [[Level::Low; 5]; 4];

    loop {
        if let Ok(g) = RESULT.try_lock() {
            for (r_index, (g_row, o_row)) in g.iter().zip(old.iter()).enumerate() {
                for (c_index, (g_col, o_col)) in g_row.iter().zip(o_row.iter()).enumerate() {
                    if g_col != o_col {
                        info!("r:{} c{} {} → {}", r_index, c_index, o_col, g_col);
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
