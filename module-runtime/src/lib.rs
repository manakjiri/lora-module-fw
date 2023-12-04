
#![no_std]


pub use embassy_stm32;
pub use embassy_executor;
pub use embassy_embedded_hal;
pub use embedded_storage;
pub use embassy_time;
pub use cortex_m;
pub use cortex_m_rt;
pub use futures;
pub use heapless;
pub use panic_probe;
pub use defmt;
pub use defmt_rtt;

use embassy_stm32::rcc::*;
use embassy_stm32::time::Hertz;

pub fn init() -> embassy_stm32::Peripherals {
    let mut config = embassy_stm32::Config::default();
    config.rcc.hse = Some(Hse {
        freq: Hertz(32_000_000),
        mode: HseMode::Bypass,
        prescaler: HsePrescaler::DIV1,
    });
    config.rcc.mux = ClockSrc::PLL1_R;
    config.rcc.pll = Some(Pll {
        source: PllSource::HSE,
        prediv: PllPreDiv::DIV2,
        mul: PllMul::MUL6,
        divp: None,
        divq: Some(PllQDiv::DIV2), // PLL1_Q clock (32 / 2 * 6 / 2), used for RNG
        divr: Some(PllRDiv::DIV2), // sysclk 48Mhz clock (32 / 2 * 6 / 2)
    });
    embassy_stm32::init(config)
}
