#![no_std]
#![macro_use]
#![feature(type_alias_impl_trait)]
#![allow(stable_features, unknown_lints, async_fn_in_trait, dead_code)]

pub use cortex_m;
pub use cortex_m_rt;
pub use defmt;
pub use defmt_rtt;
pub use embassy_executor;
pub use embassy_futures;
pub use embassy_stm32;
pub use embassy_sync;
pub use embassy_time;
pub use futures;
pub use gateway_host_schema;
pub use heapless;
pub use host::*;
pub use lora::*;
pub use lora_phy;
pub use ota::*;
pub use panic_probe;
pub use postcard;
pub use serde;

use self::iv::{Stm32wlInterfaceVariant, SubghzSpiDevice};
use embassy_stm32::crc::{self, Crc};
use embassy_stm32::gpio::{AnyPin, Level, Output, Pin, Speed};
use embassy_stm32::i2c::{self, I2c};
use embassy_stm32::rcc::*;
use embassy_stm32::spi::Spi;
use embassy_stm32::time::Hertz;
use embassy_stm32::usart::{self, Uart};
use embassy_stm32::{bind_interrupts, peripherals};
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Delay, Timer};
use lora_phy::sx1261_2::{Sx126xVariant, TcxoCtrlVoltage, SX1261_2};
use lora_phy::LoRa;
use lora_phy::{mod_params::*, sx1261_2};

mod host;
mod iv;
mod lora;
mod ota;

const LORA_FREQUENCY_IN_HZ: u32 = 869_525_000; // warning: set this appropriately for the region

bind_interrupts!(struct Irqs{
    LPUART1 => usart::InterruptHandler<peripherals::LPUART1>;
    SUBGHZ_RADIO => self::iv::InterruptHandler;
    I2C1_EV => i2c::EventInterruptHandler<peripherals::I2C1>;
    I2C1_ER => i2c::ErrorInterruptHandler<peripherals::I2C1>;
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

pub struct Display {
    pub i2c: I2c<'static, peripherals::I2C1, peripherals::DMA1_CH5, peripherals::DMA1_CH6>,
}

impl Display {
    async fn write_command(&mut self, cmd: u8) -> Result<(), i2c::Error> {
        self.i2c.write(0b0111100, [0x00u8, cmd].as_slice()).await
    }

    pub async fn write_frame(&mut self, data: &[u8]) -> Result<(), i2c::Error> {
        for i in 0..8 {
            let mut buffer = [0u8; 128 + 1];
            buffer[0] = 0x40u8;
            buffer[1..].copy_from_slice(&data[i * 128..(i + 1) * 128]);
            self.write_command(0xB0 + i as u8).await?;
            self.write_command(0x00).await?;
            self.write_command(0x10).await?;
            self.i2c.write(0b0111100, &buffer).await?;
        }
        Ok(())
    }

    pub async fn init(&mut self) -> Result<(), i2c::Error> {
        self.write_command(0xAE).await?; //display off

        self.write_command(0x20).await?; //Set Memory Addressing Mode
        self.write_command(0x10).await?; // 00,Horizontal Addressing Mode; 01,Vertical Addressing Mode;
                                         // 10,Page Addressing Mode (RESET); 11,Invalid

        self.write_command(0xB0).await?; //Set Page Start Address for Page Addressing Mode,0-7

        self.write_command(0x00).await?; //---set low column address
        self.write_command(0x10).await?; //---set high column address

        self.write_command(0x40).await?; //--set start line address - CHECK

        self.write_command(0x81).await?; //--set contrast control register - CHECK
        self.write_command(0xFF).await?;

        self.write_command(0xA1).await?; //--set segment re-map 0 to 127 - CHECK
        self.write_command(0xA6).await?; //--set normal color
        self.write_command(0xA8).await?; //--set multiplex ratio(1 to 64) - CHECK
        self.write_command(0x3F).await?; //

        self.write_command(0xA4).await?; //0xa4,Output follows RAM content;0xa5,Output ignores RAM content

        self.write_command(0xD3).await?; //-set display offset - CHECK
        self.write_command(0x00).await?; //-not offset

        self.write_command(0xD5).await?; //--set display clock divide ratio/oscillator frequency
        self.write_command(0xF0).await?; //--set divide ratio

        self.write_command(0xD9).await?; //--set pre-charge period
        self.write_command(0x22).await?; //

        self.write_command(0xDA).await?; //--set com pins hardware configuration - CHECK
        self.write_command(0x12).await?;

        self.write_command(0xDB).await?; //--set vcomh
        self.write_command(0x20).await?; //0x20,0.77xVcc

        self.write_command(0x8D).await?; //--set DC-DC enable
        self.write_command(0x14).await?; //
        self.write_command(0xAF).await?; //--turn on SSD1306 panel
        Ok(())
    }
}

pub struct ModuleInterface {
    pub lora: ModuleLoRa,
    pub host: ModuleHost,
    pub display: Display,
}

pub async fn init(
    module_config: ModuleConfig,
    spawner: &embassy_executor::Spawner,
) -> ModuleInterface {
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

    let spi = SubghzSpiDevice(Spi::new_subghz(p.SUBGHZSPI, p.DMA1_CH1, p.DMA1_CH2));
    // Set CTRL1 and CTRL3 for high-power transmission, while CTRL2 acts as an RF switch between tx and rx
    let ctrl2 = Output::new(p.PC5.degrade(), Level::High, Speed::High);
    let config = sx1261_2::Config {
        chip: Sx126xVariant::Stm32wl,
        tcxo_ctrl: Some(TcxoCtrlVoltage::Ctrl1V7),
        use_dcdc: true,
        use_dio2_as_rfswitch: true,
    };
    let iv = Stm32wlInterfaceVariant::new(Irqs, None, Some(ctrl2)).unwrap();
    let mut lora = LoRa::new(SX1261_2::new(spi, iv, config), false, Delay)
        .await
        .unwrap();

    let lora_modulation = lora
        .create_modulation_params(
            SpreadingFactor::_5,
            Bandwidth::_250KHz,
            CodingRate::_4_7,
            LORA_FREQUENCY_IN_HZ,
        )
        .unwrap();

    let lora_tx_params = lora
        .create_tx_packet_params(4, false, false, false, &lora_modulation)
        .unwrap();

    let lora_rx_params = lora
        .create_rx_packet_params(
            4,
            false,
            PACKET_LENGTH as u8,
            false,
            false,
            &lora_modulation,
        )
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

    let led = match module_config.version {
        ModuleVersion::NucleoWL55JC => Output::new(p.PB15, Level::High, Speed::Low).degrade(),
        ModuleVersion::Lumia => Output::new(p.PC13, Level::High, Speed::Low).degrade(),
    };

    let crc = Crc::new(
        p.CRC,
        // same as https://nicoretti.github.io/crc/api/crc32/
        match crc::Config::new(crc::InputReverseConfig::Byte, true, 4294967295) {
            Ok(c) => c,
            Err(_) => unreachable!("CRC config is invalid"),
        },
        //TODO crc::Config::new(crc::InputReverseConfig::None, false, 0).unwrap(),
    );

    let i2c = I2c::new(
        p.I2C1,
        p.PB8,
        p.PB7,
        Irqs,
        p.DMA1_CH5,
        p.DMA1_CH6,
        Hertz(400_000),
        Default::default(),
    );

    spawner.spawn(status_led_task(led)).unwrap();

    ModuleInterface {
        host: ModuleHost { uart: lpuart1 },
        lora: ModuleLoRa {
            lora,
            lora_modulation,
            lora_tx_params,
            lora_rx_params,
            crc,
        },
        display: Display { i2c },
    }
}

pub enum LedCommand {
    FlashShort,
}

static STATUS_LED: Channel<ThreadModeRawMutex, LedCommand, 1> = Channel::new();

pub async fn status_led(cmd: LedCommand) {
    STATUS_LED.send(cmd).await;
}

#[embassy_executor::task]
async fn status_led_task(mut led: Output<'static, AnyPin>) {
    //let mut led = Output::new(pin, Level::Low, Speed::Low);
    led.set_low();
    loop {
        match STATUS_LED.receive().await {
            LedCommand::FlashShort => {
                led.set_high();
                Timer::after_millis(100).await;
                led.set_low();
            }
        }
    }
}
