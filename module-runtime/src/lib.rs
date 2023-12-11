#![no_std]
#![macro_use]
#![feature(type_alias_impl_trait)]
#![allow(stable_features, unknown_lints, async_fn_in_trait, dead_code)]

pub use cortex_m;
pub use cortex_m_rt;
pub use defmt;
pub use defmt_rtt;
pub use embassy_embedded_hal;
pub use embassy_executor;
pub use embassy_futures;
pub use embassy_lora;
pub use embassy_stm32;
pub use embassy_sync;
pub use embassy_time;
pub use embedded_storage;
pub use futures;
pub use heapless;
pub use lora_phy;
pub use panic_probe;
pub use postcard;
pub use serde;

use defmt::info;
use embassy_lora::iv::Stm32wlInterfaceVariant;
use embassy_stm32::crc::{self, Crc};
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

mod host;
pub mod ota;

const LORA_FREQUENCY_IN_HZ: u32 = 869_525_000; // warning: set this appropriately for the region
const HOST_UART_BUFFER_SIZE: usize = 256;

bind_interrupts!(struct Irqs{
    LPUART1 => usart::InterruptHandler<peripherals::LPUART1>;
    SUBGHZ_RADIO => embassy_lora::iv::InterruptHandler;
});

pub enum ModuleVersion {
    NucleoWL55JC,
    Lumia,
}

pub struct ModuleConfig {
    pub version: ModuleVersion,
}

impl ModuleConfig {
    pub fn new(version: ModuleVersion) -> Self {
        Self { version }
    }
}

pub struct ModuleInterface {
    uart: Uart<'static, peripherals::LPUART1, peripherals::DMA1_CH3, peripherals::DMA1_CH4>,

    lora: LoRa<
        SX1261_2<
            Spi<'static, peripherals::SUBGHZSPI, peripherals::DMA1_CH1, peripherals::DMA1_CH2>,
            Stm32wlInterfaceVariant<Output<'static, AnyPin>>,
        >,
        Delay,
    >,
    lora_modulation: ModulationParams,
    lora_tx_params: PacketParams,
    lora_rx_params: PacketParams,

    pub crc: crc::Crc<'static>,
    pub led: Output<'static, AnyPin>,
}

pub async fn init(module_config: ModuleConfig) -> ModuleInterface {
    let mut config = embassy_stm32::Config::default();
    config.rcc.hse = Some(Hse {
        freq: Hertz(32_000_000),
        mode: match module_config.version {
            ModuleVersion::NucleoWL55JC => HseMode::Bypass,
            ModuleVersion::Lumia => HseMode::Oscillator,
        },
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

    let mut lora = LoRa::new(
        SX1261_2::new(BoardType::Stm32wlSx1262, spi, iv),
        false,
        Delay,
    )
    .await
    .unwrap();

    let lora_modulation = lora
        .create_modulation_params(
            SpreadingFactor::_10,
            Bandwidth::_250KHz,
            CodingRate::_4_8,
            LORA_FREQUENCY_IN_HZ,
        )
        .unwrap();

    let lora_tx_params = lora
        .create_tx_packet_params(4, false, true, false, &lora_modulation)
        .unwrap();

    let lora_rx_params = lora
        .create_rx_packet_params(4, false, 128u8, true, false, &lora_modulation)
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

    let crc = Crc::new(
        p.CRC,
        // same as https://nicoretti.github.io/crc/api/crc32/
        match crc::Config::new(crc::InputReverseConfig::Byte, true, 4294967295) {
            Ok(c) => c,
            Err(_) => unreachable!("CRC config is invalid"),
        },
        //TODO crc::Config::new(crc::InputReverseConfig::None, false, 0).unwrap(),
    );

    ModuleInterface {
        uart: lpuart1,
        lora,
        lora_modulation,
        lora_tx_params,
        lora_rx_params,
        crc,
        led,
    }
}

impl ModuleInterface {
    pub async fn lora_transmit(&mut self, tx_buffer: &[u8]) -> Result<(), RadioError> {
        self.lora
            .prepare_for_tx(&self.lora_modulation, 14, false)
            .await?;
        self.lora
            .tx(
                &self.lora_modulation,
                &mut self.lora_tx_params,
                tx_buffer,
                500,
            )
            .await
    }

    pub async fn lora_receive(&mut self, rx_buffer: &mut [u8]) -> Result<usize, RadioError> {
        self.lora
            .prepare_for_rx(
                &self.lora_modulation,
                &self.lora_rx_params,
                None,
                None,
                false,
            )
            .await?;
        match self.lora.rx(&self.lora_rx_params, rx_buffer).await {
            Ok((received_len, status)) => {
                info!("RX rssi {} len {}", status.rssi, received_len);
                Ok(received_len as usize)
            }
            Err(err) => Err(err),
        }
    }

    pub async fn host_uart_read_until_idle(
        &mut self,
        buffer: &mut [u8],
    ) -> Result<usize, usart::Error> {
        let mut buff = [0u8; HOST_UART_BUFFER_SIZE];
        let len = self.uart.read_until_idle(&mut buff).await?;
        Ok(host::maxval_decode(&buff[..len], buffer, 254))
    }

    pub async fn host_uart_write(&mut self, buffer: &[u8]) -> Result<(), usart::Error> {
        let mut buff = [0u8; HOST_UART_BUFFER_SIZE];
        let len = host::maxval_encode(buffer, &mut buff, 254);
        self.uart.write(&buff[..len]).await
    }
}
