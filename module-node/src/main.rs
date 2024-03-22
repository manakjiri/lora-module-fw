#![no_main]
#![no_std]
#![macro_use]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

use defmt::*;
use embassy_executor::Spawner;
use module_runtime::{embassy_time::Timer, heapless::Vec, *};

use embassy_boot_stm32::{AlignedBuffer, FirmwareUpdater, FirmwareUpdaterConfig};
use embassy_embedded_hal::adapter::BlockingAsync;
use embassy_stm32::flash::{Flash, WRITE_SIZE};
use embassy_sync::mutex::Mutex;
use heapless::FnvIndexMap;

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
    queue: FnvIndexMap<usize, Vec<u8, 96>, 8>,
    next_address: usize,
    valid_up_to_address: usize,
}

impl OtaMemoryDelegate for OtaMemory {
    async fn write(&mut self, valid_up_to: usize, offset: usize, data: &[u8]) -> bool {
        self.valid_up_to_address = valid_up_to;
        if self.page.is_none() {
            self.page = Some(OtaPage::new(self.next_address))
        }
        let page = self.page.as_mut().unwrap();

        let mut to_pop = Vec::<usize, 16>::new();
        for (a, d) in self.queue.iter() {
            if *a < page.address {
                to_pop.push(*a).unwrap();
            } else if *a < page.address + PAGE_SIZE {
                info!("writing from queue 0x{:x}: {}", *a, page.write(*a, &d[..]));
                to_pop.push(*a).unwrap();
            }
        }
        for i in to_pop.iter() {
            self.queue.remove(i);
        }

        if offset < page.address {
            warn!("offset lower than current page, ignoring");
            true
        } else {
            if page.write(offset, data) {
                true
            } else {
                match self.queue.insert(offset, data.iter().cloned().collect()) {
                    Ok(_) => true,
                    Err(_) => false,
                }
            }
        }
    }
}

impl OtaMemory {
    fn new() -> Self {
        OtaMemory {
            page: None,
            queue: FnvIndexMap::new(),
            next_address: 0,
            valid_up_to_address: 0,
        }
    }

    fn get_page(&mut self) -> Option<OtaPage> {
        match self.page.as_mut() {
            Some(p) => {
                if p.last_address == PAGE_SIZE && self.valid_up_to_address >= p.address + PAGE_SIZE
                {
                    self.next_address += PAGE_SIZE;
                    self.page.take()
                } else {
                    None
                }
            }
            None => None,
        }
    }

    fn get_last_page(&mut self) -> Option<OtaPage> {
        self.page.take()
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let mut module = init(ModuleConfig::new(ModuleVersion::Lumia), &spawner).await;
    module.set_vdd_enable(true);
    info!("hello from node");

    let flash = Mutex::new(BlockingAsync::new(Flash::new_blocking(module.flash)));
    let config = FirmwareUpdaterConfig::from_linkerfile(&flash, &flash);
    let mut magic = AlignedBuffer([0; WRITE_SIZE]);
    let mut updater = FirmwareUpdater::new(config, &mut magic.0);

    let mut memory = module.memory;
    let mut buff = [0u8; 3];
    Timer::after_millis(100).await;
    info!("res {:?}", memory.read_jedec_id(&mut buff).await);
    info!("read {=[u8]:x}", buff);

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
            if let Some(page) = ota_consumer.memory.get_last_page() {
                info!("Writing last page at 0x{:x}", page.address);
                updater
                    .write_firmware(page.address, &page.buffer)
                    .await
                    .unwrap();
            }
            updater.mark_updated().await.unwrap();
            info!("Marked as updated");
            cortex_m::peripheral::SCB::sys_reset();
        }
    }
}
