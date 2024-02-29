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

struct OtaPage {
    buffer: [u8; PAGE_SIZE],
    last_address: usize,
    address: usize,
}

impl OtaPage {
    fn new(address: usize) -> Self {
        OtaPage {
            buffer: [0u8; PAGE_SIZE],
            last_address: 0,
            address,
        }
    }

    fn write(&mut self, offset: usize, data: &[u8]) -> bool {
        if self.last_address != PAGE_SIZE {
            let start = offset - self.address;
            let end = start + data.len();
            if end > self.buffer.len() {
                // something weird happened, lets hope for the best next time
                return false;
            }
            self.buffer[start..end].copy_from_slice(data);
            self.last_address = end;
            true
        } else {
            // trying to write too far when the last block is lost and next comes
            // we should get the missing one in the next call
            false
        }
    }
}

struct OtaMemory {
    page: Option<OtaPage>,
}

impl OtaMemoryDelegate for OtaMemory {
    async fn write(&mut self, offset: usize, data: &[u8]) -> bool {
        if self.page.is_none() {
            self.page = Some(OtaPage::new(offset % PAGE_SIZE))
        }
        self.page.as_mut().unwrap().write(offset, data)
    }
}

impl OtaMemory {
    fn new() -> Self {
        OtaMemory { page: None }
    }

    fn get_page(&mut self) -> Option<OtaPage> {
        self.page.take()
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let module = init(ModuleConfig::new(ModuleVersion::Lumia), &spawner).await;
    info!("hello from node");

    let flash = Mutex::new(BlockingAsync::new(Flash::new_blocking(module.flash)));
    let config = FirmwareUpdaterConfig::from_linkerfile(&flash);
    let mut magic = AlignedBuffer([0; WRITE_SIZE]);
    let mut updater = FirmwareUpdater::new(config, &mut magic.0);

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

        if let Some(page) = ota_consumer.memory.get_page() {
            info!("Writing page at 0x{:x}", page.address);
            updater
                .write_firmware(page.address, &page.buffer)
                .await
                .unwrap();
        }

        if ota_consumer.is_done() {
            updater.mark_updated().await.unwrap();
            info!("Marked as updated");
            cortex_m::peripheral::SCB::sys_reset();
        }
    }
}
