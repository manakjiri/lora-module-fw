#![no_main]
#![no_std]
#![macro_use]
#![feature(type_alias_impl_trait)]

use defmt::*;
use embassy_executor::Spawner;
use module_runtime::*;

use embassy_boot_stm32::{AlignedBuffer, FirmwareUpdater, FirmwareUpdaterConfig};
use embassy_embedded_hal::adapter::BlockingAsync;
use embassy_stm32::flash::{Flash, WRITE_SIZE};
use embassy_sync::mutex::Mutex;

const PAGE_SIZE: usize = 2048;

struct OtaMemory {
    page_buffer: [u8; PAGE_SIZE],
    max_offset: usize,
}

impl OtaMemoryDelegate for OtaMemory {
    async fn write(&mut self, offset: usize, data: &[u8]) -> bool {
        true
    }
}

impl OtaMemory {
    fn new() -> Self {
        OtaMemory {
            page_buffer: [0u8; PAGE_SIZE],
            max_offset: 0,
        }
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let module = init(ModuleConfig::new(ModuleVersion::Lumia), &spawner).await;

    let flash = Mutex::new(BlockingAsync::new(Flash::new_blocking(module.flash)));
    let config = FirmwareUpdaterConfig::from_linkerfile(&flash);
    let mut magic = AlignedBuffer([0; WRITE_SIZE]);
    let mut updater = FirmwareUpdater::new(config, &mut magic.0);

    //updater.write_firmware(offset, data)

    info!("hello from node");
    let mut ota_consumer = OtaConsumer::<OtaMemory>::new(OtaMemory::new());
    let mut lora = module.lora;
    let mut rx_buffer = [0u8; 128];
    loop {
        match lora.receive_continuous(rx_buffer.as_mut()).await {
            Ok(len) => match ota_consumer
                .process_message(&mut lora, &rx_buffer[..len])
                .await
            {
                Ok(()) => {}
                Err(e) => {
                    error!("ota error: {}", e)
                }
            },
            Err(e) => {
                error!("lora error: {}", e)
            }
        }
        status_led(LedCommand::FlashShort).await;
    }
}
