#![no_main]
#![no_std]
#![macro_use]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

mod ota_memory;
use defmt::*;
use embassy_executor::Spawner;
use module_runtime::*;
use embassy_boot_stm32::{AlignedBuffer, FirmwareUpdater, FirmwareUpdaterConfig};
use embassy_embedded_hal::adapter::BlockingAsync;
use embassy_stm32::flash::{Flash, WRITE_SIZE};
use embassy_sync::mutex::Mutex;
use ota_memory::*;


#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let mut module = init(ModuleConfig::new(ModuleVersion::Lumia), &spawner).await;
    //module.set_vdd_enable(true);
    info!("hello from node {}", module.lora.address);

    let flash = Mutex::new(BlockingAsync::new(Flash::new_blocking(module.flash)));
    let config = FirmwareUpdaterConfig::from_linkerfile(&flash, &flash);
    let mut magic = AlignedBuffer([0; WRITE_SIZE]);
    let mut updater = FirmwareUpdater::new(config, &mut magic.0);

    //let mut memory = module.memory;
    //let mut buff = [0u8; 3];
    //Timer::after_millis(100).await;
    //info!("res {:?}", memory.read_jedec_id(&mut buff).await);
    //info!("read {=[u8]:x}", buff);

    let mut ota_consumer = OtaConsumer::<OtaMemory>::new(OtaMemory::new());
    let mut lora = module.lora;
    loop {
        match lora.receive_continuous().await {
            Ok(p) => match p.packet_type {
                LoRaPacketType::OTA => match ota_consumer.process_message(&mut lora, p).await {
                    Ok(()) => {}
                    Err(e) => {
                        error!("ota error: {}", e)
                    }
                },
                LoRaPacketType::SoilSensor => {
                    info!("soil sensor packet");
                },
                _ => {}
            },
            Err(e) => {
                error!("lora error: {}", e)
            }
        }
        status_led(LedCommand::FlashShort).await;

        // disabled for testing the range
        /* if let Some(page) = ota_consumer.memory.get_page() {
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
        } */
    }
}
