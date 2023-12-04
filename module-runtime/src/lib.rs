#![no_std]
#![macro_use]
#![feature(type_alias_impl_trait)]
#![allow(stable_features, unknown_lints, async_fn_in_trait)]

pub use cortex_m;
pub use cortex_m_rt;
pub use defmt;
pub use defmt_rtt;
pub use embassy_embedded_hal;
pub use embassy_executor;
pub use embassy_lora;
pub use embassy_stm32;
pub use embassy_time;
pub use embedded_storage;
pub use futures;
pub use heapless;
pub use panic_probe;

use embassy_lora::iv::Stm32wlInterfaceVariant;
use embassy_stm32::gpio::{AnyPin, Level, Output, Pin, Speed};
use embassy_stm32::rcc::*;
use embassy_stm32::spi::Spi;
use embassy_stm32::time::Hertz;
use embassy_stm32::usart::{self, Uart};
use embassy_stm32::{bind_interrupts, peripherals};
use embassy_time::Delay;
use lora_phy::mod_params::*;
use lora_phy::sx1261_2::SX1261_2;
use lora_phy::LoRa;

bind_interrupts!(struct Irqs{
    LPUART1 => usart::InterruptHandler<peripherals::LPUART1>;
    SUBGHZ_RADIO => embassy_lora::iv::InterruptHandler;
});

pub struct ModuleInterface {
    pub uart: Uart<'static, peripherals::LPUART1, peripherals::DMA1_CH3, peripherals::DMA1_CH4>,
    pub lora: LoRa<
        SX1261_2<
            Spi<'static, peripherals::SUBGHZSPI, peripherals::DMA1_CH1, peripherals::DMA1_CH2>,
            Stm32wlInterfaceVariant<Output<'static, AnyPin>>,
        >,
        Delay,
    >,
    pub led: Output<'static, AnyPin>,
}

pub async fn init() -> ModuleInterface {
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
    let p = embassy_stm32::init(config);

    let spi = Spi::new_subghz(p.SUBGHZSPI, p.DMA1_CH1, p.DMA1_CH2);
    // Set CTRL1 and CTRL3 for high-power transmission, while CTRL2 acts as an RF switch between tx and rx
    let ctrl2 = Output::new(p.PC5.degrade(), Level::High, Speed::High);
    let iv = Stm32wlInterfaceVariant::new(Irqs, None, Some(ctrl2)).unwrap();

    let lora = LoRa::new(
        SX1261_2::new(BoardType::Stm32wlSx1262, spi, iv),
        false,
        Delay,
    )
    .await
    .unwrap();

    let mut lpuart1_config = usart::Config::default();
    lpuart1_config.baudrate = 115200;
    let lpuart1 = Uart::new(
        p.LPUART1,
        p.PA3,
        p.PA2,
        Irqs,
        p.DMA1_CH3,
        p.DMA1_CH4,
        lpuart1_config,
    )
    .unwrap();

    let led = Output::new(p.PC13, Level::High, Speed::Low).degrade();

    ModuleInterface {
        uart: lpuart1,
        lora,
        led,
    }
}
