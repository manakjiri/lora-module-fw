#![no_main]
#![no_std]
#![macro_use]
#![feature(type_alias_impl_trait)]

use defmt::*;
use embassy_executor::Spawner;
use module_runtime::*;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let mut module = init(ModuleConfig::new(ModuleVersion::Lumia), &spawner).await;
    info!("hello from node");

    if let Err(e) = module.display.init().await {
        error!("init display: {}", e);
    }
    let _ = module.display.write_frame(&[0u8; 128 * 8]).await;

    let mut ota_consumer = OtaConsumer::new();
    let mut lora = module.lora;
    let mut rx_buffer = [0u8; 128];
    loop {
        match lora.receive_continuous(rx_buffer.as_mut()).await {
            Ok(len) => match ota_consumer
                .process_message(&mut lora, &rx_buffer[..len])
                .await
            {
                Ok(()) => {
                    if let Err(e) = module.display.write_frame(ota_consumer.get_buffer()).await {
                        error!("write display: {}", e);
                    }
                }
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
