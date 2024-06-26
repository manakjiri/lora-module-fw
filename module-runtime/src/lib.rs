#![no_std]
#![macro_use]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![allow(stable_features, unknown_lints, async_fn_in_trait, dead_code, unused_imports)]

pub use cortex_m;
pub use cortex_m_rt;
pub use defmt;
pub use defmt_rtt;
pub use embassy_boot;
pub use embassy_boot_stm32;
pub use embassy_embedded_hal;
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
use defmt::info;
use embassy_stm32::crc::{self, Crc};
use embassy_stm32::gpio::{AnyPin, Level, Output, Pin, Speed};
use embassy_stm32::rcc::*;
use embassy_stm32::spi::{self, Spi};
use embassy_stm32::time::Hertz;
use embassy_stm32::timer;
use embassy_stm32::usart::{self, Uart};
use embassy_stm32::exti::{self, Channel};
use embassy_stm32::{bind_interrupts, peripherals};
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::channel;
use embassy_time::{Delay, Timer};
use embedded_hal::digital::{OutputPin, PinState};
use lora_phy::mod_params::*;
use lora_phy::sx126x::{self, Sx126x, Sx126xVariant, TcxoCtrlVoltage};
use lora_phy::LoRa;

mod host;
mod iv;
mod lora;
mod ota;

const LORA_FREQUENCY_IN_HZ: u32 = 869_525_000; // warning: set this appropriately for the region

bind_interrupts!(struct Irqs{
    LPUART1 => usart::InterruptHandler<peripherals::LPUART1>;
    SUBGHZ_RADIO => self::iv::InterruptHandler;
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

/* pub struct ModuleUpdater {
    flash: Mutex<NoopRawMutex, BlockingAsync<Flash<'static, flash::Blocking>>>,
    pub updater: FirmwareUpdater<
        'static,
        Partition<'static, NoopRawMutex, BlockingAsync<Flash<'static, flash::Blocking>>>,
        Partition<'static, NoopRawMutex, BlockingAsync<Flash<'static, flash::Blocking>>>,
    >,
}

impl ModuleUpdater {
    fn new(f: embassy_stm32::peripherals::FLASH) -> Self {
        ModuleUpdater { flash, updater }
    }
} */

#[derive(Debug, defmt::Format)]
pub enum MemoryError {
    Spi(spi::Error),
}

pub struct ModuleMemory {
    spi: Spi<'static, peripherals::SPI2, peripherals::DMA1_CH5, peripherals::DMA1_CH6>,
    ncs: Output<'static>,
    hold: Output<'static>,
}

impl ModuleMemory {
    //FIXME will not disable ncs after an error accoured

    pub async fn read_jedec_id(&mut self, id: &mut [u8; 3]) -> Result<(), MemoryError> {
        let mut write = [0x9Fu8; 1];
        info!("spi write {:?}", &write);
        self.ncs.set_low();
        info!("cs {:?}", self.ncs.get_output_level());
        self.spi
            .transfer_in_place(&mut write)
            .await
            .map_err(MemoryError::Spi)?;
        self.spi.read(id).await.map_err(MemoryError::Spi)?;
        self.ncs.set_high();
        info!("cs {:?}", self.ncs.get_output_level());
        Ok(())
    }

    /* async fn write_enable(&mut self) -> Result<(), MemoryError> {
        let write = [0x06u8; 1];
        self.ncs.set_low();
        let ret = self.spi.write(&write).await.map_err(MemoryError::Spi);
        self.ncs.set_high();
        ret
    } */

    pub async fn read(&mut self, addr: usize, buff: &mut [u8]) -> Result<(), MemoryError> {
        let mut write = [0u8; 4];
        write[0] = 0x03;
        write[1..4].copy_from_slice(&addr.to_le_bytes()[0..3]);

        self.ncs.set_low();
        info!("spi write {:?}", &write);
        self.spi.write(&write).await.map_err(MemoryError::Spi)?;
        self.spi.read(buff).await.map_err(MemoryError::Spi)?;
        self.ncs.set_high();
        Ok(())
    }

    /* pub async fn write(&mut self, addr: usize, buff: &[u8]) -> Result<(), MemoryError> {
        let mut write = [0u8; 4];
        write[0] = 0x03;
        write[1..4].copy_from_slice(&addr.to_le_bytes()[0..3]);

        self.ncs.set_low();
        info!("spi write {:?}", &write);
        match self.spi.write(&write).await {
            Ok(()) => {}
            Err(e) => {
                self.ncs.set_high();
                return Err(MemoryError::Spi(e));
            }
        }

        let ret = self.spi.write(buff).await.map_err(MemoryError::Spi);
        self.ncs.set_high();
        ret
    } */
}

pub struct ModuleInterface {
    pub lora: ModuleLoRa,
    pub flash: peripherals::FLASH,
    pub memory: ModuleMemory,

    #[cfg(feature = "host_interface")]
    pub host: ModuleHost,

    pub io1: AnyPin,
    pub io2: AnyPin,
    pub io3: AnyPin,
    #[cfg(not(feature = "host_interface"))]
    pub io4: AnyPin,
    pub io5: AnyPin,
    pub io6: AnyPin,
    pub io7: AnyPin,
    pub io8: AnyPin,
    pub io9: AnyPin,
    pub io10: AnyPin,
    pub io11: AnyPin,

    pub io1_8_exti: exti::AnyChannel,
    pub io2_9_exti: exti::AnyChannel,
    pub io3_11_exti: exti::AnyChannel,
    pub io4_exti: exti::AnyChannel,
    pub io5_exti: exti::AnyChannel,
    pub io6_exti: exti::AnyChannel,
    pub io7_exti: exti::AnyChannel,
    pub io10_exti: exti::AnyChannel,

    pub vdd_switch: Output<'static>,
}

impl ModuleInterface {
    pub fn set_vdd_enable(&mut self, enabled: bool) {
        self.vdd_switch
            .set_state(match enabled {
                true => PinState::Low,
                false => PinState::High,
            })
            .unwrap();
    }
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
    config.rcc.sys = match module_config.version {
        ModuleVersion::NucleoWL55JC => Sysclk::PLL1_R, // 48 MHz
        ModuleVersion::Lumia => Sysclk::MSI // Default 1 MHz
    };
    config.rcc.pll = match module_config.version {
        ModuleVersion::NucleoWL55JC => {
            Some(Pll {
                source: PllSource::HSE,
                prediv: PllPreDiv::DIV2,
                mul: PllMul::MUL6,
                divp: None,
                divq: None, //Some(PllQDiv::DIV2), // PLL1_Q clock (32 / 2 * 6 / 2), used for RNG
                divr: Some(PllRDiv::DIV2), // sysclk 48Mhz clock (32 / 2 * 6 / 2)
            })
        }
        ModuleVersion::Lumia => None
    };
    let p = embassy_stm32::init(config);

    let vdd_switch = Output::new(p.PB2, Level::High, Speed::Low);

    let spi = SubghzSpiDevice(Spi::new_subghz(p.SUBGHZSPI, p.DMA1_CH1, p.DMA1_CH2));
    let ctrl2 = match module_config.version {
        ModuleVersion::Lumia => p.PA9.degrade(),
        ModuleVersion::NucleoWL55JC => {
            core::mem::forget(Output::new(p.PC4.degrade(), Level::High, Speed::High)); //ctrl1 !high power
            core::mem::forget(Output::new(p.PC3.degrade(), Level::High, Speed::High)); //ctrl3 always high
            p.PC5.degrade()
        }
    };
    // Set CTRL1 and CTRL3 for high-power transmission, while CTRL2 acts as an RF switch between tx and rx
    let ctrl2 = Output::new(ctrl2, Level::Low, Speed::High);
    let config = sx126x::Config {
        chip: Sx126xVariant::Stm32wl,
        tcxo_ctrl: Some(TcxoCtrlVoltage::Ctrl1V7),
        use_dcdc: true,
        use_dio2_as_rfswitch: false,
    };
    let iv = Stm32wlInterfaceVariant::new(Irqs, None, Some(ctrl2)).unwrap();
    let mut lora = LoRa::new(Sx126x::new(spi, iv, config), false, Delay)
        .await
        .unwrap();

    let lora_modulation = lora
        .create_modulation_params(
            SpreadingFactor::_5,
            Bandwidth::_250KHz,
            CodingRate::_4_5,
            LORA_FREQUENCY_IN_HZ,
        )
        .unwrap();

    #[cfg(feature = "host_interface")]
    let mut host_uart = {
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
        lpuart1
    };

    let led = match module_config.version {
        ModuleVersion::NucleoWL55JC => p.PB15.degrade(),
        ModuleVersion::Lumia => p.PC13.degrade(),
    };

    let crc = Crc::new(
        p.CRC,
        // same as https://nicoretti.github.io/crc/api/crc32/
        match crc::Config::new(
            crc::InputReverseConfig::Byte,
            true,
            crc::PolySize::Width32,
            4294967295,
            79764919,
        ) {
            Ok(c) => c,
            Err(_) => unreachable!("CRC config is invalid"),
        },
    );

    let mut spi_config = spi::Config::default();
    spi_config.frequency = Hertz(1_000_000);
    spi_config.mode = spi::MODE_0;
    spi_config.bit_order = spi::BitOrder::MsbFirst;
    let spi = Spi::new(
        p.SPI2, p.PA8, p.PA10, p.PA5, p.DMA1_CH5, p.DMA1_CH6, spi_config,
    );
    let ncs = Output::new(p.PA12, Level::High, Speed::VeryHigh);
    let hold = Output::new(p.PC14, Level::High, Speed::Low);

    spawner.spawn(status_led_task(led)).unwrap();

    let memory = ModuleMemory { spi, ncs, hold };

    ModuleInterface {
        lora: ModuleLoRa {
            lora,
            lora_modulation,
            crc,
            address: match module_config.version {
                ModuleVersion::NucleoWL55JC => 1,
                ModuleVersion::Lumia => 3,
            },
        },
        flash: p.FLASH,
        memory,
        vdd_switch,

        io1: p.PA7.degrade(),
        io2: p.PA6.degrade(),
        io3: p.PA4.degrade(),
        #[cfg(not(feature = "host_interface"))]
        io4: p.PA2.degrade(),
        io5: p.PA1.degrade(),
        io6: p.PA0.degrade(),
        io7: p.PB8.degrade(),
        io8: p.PB7.degrade(),
        io9: p.PB6.degrade(),
        io10: p.PB5.degrade(),
        io11: p.PB4.degrade(),

        io1_8_exti: p.EXTI7.degrade(),
        io2_9_exti: p.EXTI6.degrade(),
        io3_11_exti: p.EXTI4.degrade(),
        io4_exti: p.EXTI2.degrade(),
        io5_exti: p.EXTI1.degrade(),
        io6_exti: p.EXTI0.degrade(),
        io7_exti: p.EXTI8.degrade(),
        io10_exti: p.EXTI5.degrade(),

        #[cfg(feature = "host_interface")]
        host: ModuleHost { uart: host_uart },
    }
}

pub enum LedCommand {
    FlashShort,
}

static STATUS_LED: channel::Channel<ThreadModeRawMutex, LedCommand, 3> = channel::Channel::new();

pub async fn status_led(cmd: LedCommand) {
    STATUS_LED.send(cmd).await;
}

#[embassy_executor::task]
async fn status_led_task(led: AnyPin) {
    let mut led = Output::new(led, Level::Low, Speed::Low);
    /* do welcome flash */
    for _ in 0..6 {
        led.toggle();
        Timer::after_millis(50).await;
    }
    led.set_low();
    /* wait for commands */
    loop {
        match STATUS_LED.receive().await {
            LedCommand::FlashShort => {
                led.set_high();
                Timer::after_millis(100).await;
                led.set_low();
            }
        }
        Timer::after_millis(50).await;
    }
}
