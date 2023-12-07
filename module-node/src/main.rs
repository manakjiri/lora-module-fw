#![no_main]
#![no_std]
#![macro_use]
#![feature(type_alias_impl_trait)]

use defmt::*;
use embassy_executor::Spawner;
use module_runtime::*;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut module = init(ModuleConfig::new(ModuleVersion::NucleoWL55JC)).await;

    let mut rx_buffer = [0u8; 128];
    loop {
        match module.lora_receive(rx_buffer.as_mut()).await {
            Ok(len) => {}
            Err(e) => {}
        }
    }
}
