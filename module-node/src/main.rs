#![no_main]
#![no_std]
#![macro_use]
#![feature(type_alias_impl_trait)]

use defmt::*;
use embassy_executor::Spawner;
use module_runtime::*;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut module = init().await;

    info!("Hello World!");

    unwrap!(module.uart.write(b"test\r\n").await);

    let mut uart_buffer = [0u8; 128];
    loop {
        let result = module.uart.read_until_idle(&mut uart_buffer).await;
        match result {
            Ok(size) => {
                info!("size {}", size);
                if size > 0 {
                    match module.uart.write(&uart_buffer[0..size]).await {
                        Ok(()) => {}
                        Err(e) => {
                            error!("tx error: {}", e);
                        }
                    }
                }
            }
            Err(e) => {
                error!("rx error: {}", e);
            }
        }
    }
}
