#![no_main]
#![no_std]
#![macro_use]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

mod soil_sensor;
mod ota_memory;

use defmt::*;
use embassy_executor::Spawner;
use module_runtime::*;
use embassy_boot_stm32::{AlignedBuffer, FirmwareUpdater, FirmwareUpdaterConfig};
use embassy_embedded_hal::adapter::BlockingAsync;
use embassy_stm32::flash::{Flash, WRITE_SIZE};
use embassy_sync::mutex::Mutex;
use soil_sensor::{SoilSensor, SoilSensorResult};
use ota_memory::OtaMemory;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let mut module = init(ModuleConfig::new(ModuleVersion::Lumia), &spawner).await;
    module.set_vdd_enable(true);
    info!("hello from node {}", module.lora.address);

    let flash = Mutex::new(BlockingAsync::new(Flash::new_blocking(module.flash)));
    let config = FirmwareUpdaterConfig::from_linkerfile(&flash, &flash);
    let mut magic = AlignedBuffer([0; WRITE_SIZE]);
    let mut _updater = FirmwareUpdater::new(config, &mut magic.0);

    let mut soil_sensor = SoilSensor::new(
            module.io8,
            module.io9,
            module.io7,
            module.io4,
            module.io5,
            module.io3,
            module.io2,
            module.io2_9_exti,
        );

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
                    Ok(_) => {}
                    Err(e) => {
                        error!("ota error: {}", e)
                    }
                },
                LoRaPacketType::SoilSensor => {
                    let samples = soil_sensor.sample_all().await;
                    let mut resp = LoRaPacket::new(p.source, LoRaPacketType::SoilSensor);
                    for sample in samples {
                        let bytes = match sample {
                            SoilSensorResult::Timeout => [0, 0],
                            SoilSensorResult::Ok(d) => {
                                (d.as_micros() as u16).to_le_bytes().try_into().unwrap()
                            }
                        };
                        resp.payload.push(bytes[0]).unwrap();
                        resp.payload.push(bytes[1]).unwrap();
                    }
                    match lora.transmit(&mut resp).await {
                        Ok(_) => {}
                        Err(e) => {
                            error!("lora tx error: {}", e)
                        }
                    }
                },
                _ => {}
            },
            Err(e) => {
                error!("lora rx error: {}", e)
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
