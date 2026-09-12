#![no_std]
#![no_main]

use core::panic::PanicInfo;
use cortex_m::asm;
use cortex_m_rt::{ExceptionFrame, exception};
use defmt::info;
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_rp::gpio::{Input, Pull};
use embassy_time::Timer;

fn read_bits(inputs: &[Input<'_>; 5]) -> u8 {
    (inputs[0].is_high() as u8)
        | ((inputs[1].is_high() as u8) << 1)
        | ((inputs[2].is_high() as u8) << 2)
        | ((inputs[3].is_high() as u8) << 3)
        | ((inputs[4].is_high() as u8) << 4)
}

#[panic_handler]
fn panic(info: &PanicInfo<'_>) -> ! {
    defmt::error!("panic: {}", defmt::Display2Format(info));
    loop {
        asm::bkpt();
    }
}

#[exception]
unsafe fn HardFault(frame: &ExceptionFrame) -> ! {
    defmt::error!("hard fault");
    defmt::error!(
        "r0={=u32:x} r1={=u32:x} r2={=u32:x} r3={=u32:x}",
        frame.r0(),
        frame.r1(),
        frame.r2(),
        frame.r3()
    );
    defmt::error!(
        "r12={=u32:x} lr={=u32:x} pc={=u32:x} xpsr={=u32:x}",
        frame.r12(),
        frame.lr(),
        frame.pc(),
        frame.xpsr()
    );
    loop {
        asm::bkpt();
    }
}

#[exception]
unsafe fn DefaultHandler(irqn: i16) {
    defmt::warn!("unexpected interrupt: {}", irqn);
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("pico2 input monitor start");

    let p = embassy_rp::init(Default::default());

    let inputs = [
        Input::new(p.PIN_10, Pull::Down),
        Input::new(p.PIN_11, Pull::Down),
        Input::new(p.PIN_12, Pull::Down),
        Input::new(p.PIN_13, Pull::Down),
        Input::new(p.PIN_14, Pull::Down),
    ];

    info!("input monitor start: inputs gpio10-gpio14");

    loop {
        info!("inputs gpio10-gpio14={=u8:05b}", read_bits(&inputs));
        Timer::after_millis(50).await;
    }
}
