#![no_main]
#![no_std]
#![macro_use]
#![feature(type_alias_impl_trait)]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::usart;
use lora_phy::mod_params::RadioError;
use module_runtime::*;

type UartBuffer = [u8; 128];

#[derive(Debug, defmt::Format, PartialEq)]
enum Error {
    MessageTooShort,
    MessageUnknownType,
    Usart(usart::Error),
    LoRa(RadioError),
}

async fn lora_message(
    module: &mut ModuleInterface,
    tx_buffer: &[u8],
    rx_buffer: &mut [u8],
) -> Result<u8, Error> {
    module.lora_transmit(tx_buffer).await.map_err(Error::LoRa)?;
    module.lora_receive(rx_buffer).await.map_err(Error::LoRa)
}

async fn process_host_message(
    module: &mut ModuleInterface,
    uart_buffer: &[u8],
) -> Result<(), Error> {
    if uart_buffer.len() == 0 {
        return Err(Error::MessageTooShort);
    }
    match uart_buffer[0] {
        // ping
        0 => {
            module
                .host_uart_write(uart_buffer)
                .await
                .map_err(Error::Usart)?;
        }
        // transmit lora
        1 => {
            if uart_buffer.len() > 1 {
                let mut rx = [0u8; 128];
                lora_message(module, &uart_buffer[1..], &mut rx[1..]).await?;
                rx[0] = 1;
                module
                    .host_uart_write(uart_buffer)
                    .await
                    .map_err(Error::Usart)?;
            }
        }
        // unhandled
        _ => {
            return Err(Error::MessageUnknownType);
        }
    }
    Ok(())
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut module = init(ModuleConfig::new(ModuleVersion::NucleoWL55JC)).await;

    let mut uart_buffer: UartBuffer = [0u8; 128];
    loop {
        let result = module.host_uart_read_until_idle(&mut uart_buffer).await;
        match result {
            Ok(size) => {
                //info!("size {}", size);
                match process_host_message(&mut module, &uart_buffer[..size]).await {
                    Ok(()) => {}
                    Err(e) => {
                        error!("{}", e);
                    }
                }
            }
            Err(e) => {
                error!("uart: {}", e);
            }
        }
    }
}
