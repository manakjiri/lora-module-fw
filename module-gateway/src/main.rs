#![no_main]
#![no_std]
#![macro_use]
#![feature(type_alias_impl_trait)]

use defmt::*;
use embassy_executor::Spawner;
use embassy_futures::select::*;
use embassy_stm32::usart;
use lora_phy::mod_params::RadioError;
use module_runtime::*;
use ota::*;

type UartBuffer = [u8; 128];

#[derive(Debug, defmt::Format, PartialEq)]
enum Error {
    MessageTooShort,
    MessageUnknownType,
    Usart(usart::Error),
    LoRa(RadioError),
    SerDe(postcard::Error),
    Ota(OtaError),
}

async fn lora_message(
    module: &mut ModuleInterface,
    tx_buffer: &[u8],
    rx_buffer: &mut [u8],
) -> Result<usize, Error> {
    module.lora_transmit(tx_buffer).await.map_err(Error::LoRa)?;
    module.lora_receive(rx_buffer).await.map_err(Error::LoRa)
}

#[derive(Debug, defmt::Format, PartialEq)]
enum OtaProducerState {
    Init,
    Download,
}

struct OtaProducer {
    state: OtaProducerState,
}

impl OtaProducer {
    fn new() -> OtaProducer {
        OtaProducer {
            state: OtaProducerState::Init,
        }
    }

    fn process_status(
        &mut self,
        module: &mut ModuleInterface,
        status: OtaStatusPacket,
    ) -> Result<(), Error> {
        Ok(())
    }

    fn process_response(
        &mut self,
        module: &mut ModuleInterface,
        buffer: &[u8],
    ) -> Result<(), Error> {
        match postcard::from_bytes::<OtaPacket>(buffer).map_err(Error::SerDe)? {
            OtaPacket::Init(_) => return Err(Error::Ota(OtaError::OtaInvalidPacketType)),
            OtaPacket::Data(_) => return Err(Error::Ota(OtaError::OtaInvalidPacketType)),
            OtaPacket::InitAck => {
                if self.state == OtaProducerState::Init {
                    self.state = OtaProducerState::Download;
                    Ok(())
                } else {
                    return Err(Error::Ota(OtaError::OtaInvalidPacketType));
                }
            }
            OtaPacket::Status(status) => {
                if self.state == OtaProducerState::Download {
                    self.process_status(module, status)
                } else {
                    return Err(Error::Ota(OtaError::OtaInvalidPacketType));
                }
            }
        }
    }

    async fn init_download(
        &mut self,
        module: &mut ModuleInterface,
        init: OtaInitPacket,
    ) -> Result<(), Error> {
        let mut tx_buffer = [0u8; 128];
        let packet =
            postcard::to_slice(&OtaPacket::Init(init), &mut tx_buffer).map_err(Error::SerDe)?;

        let mut last_error: Option<Error> = None;
        for _ in 0..5 {
            let mut rx_buffer = [0u8; 128];
            match lora_message(module, &packet, &mut rx_buffer).await {
                Ok(len) => match self.process_response(module, &rx_buffer[..len]) {
                    Ok(()) => return Ok(()),
                    Err(e) => {
                        last_error = Some(e);
                    }
                },
                Err(e) => {
                    last_error = Some(e);
                }
            }
        }
        Err(last_error.unwrap())
    }
}

struct Gateway {
    ota: Option<OtaProducer>,
}

impl Gateway {
    fn new() -> Gateway {
        Gateway { ota: None }
    }

    async fn process_host_message(
        &mut self,
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
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut module = init(ModuleConfig::new(ModuleVersion::NucleoWL55JC)).await;
    let mut gateway = Gateway::new();

    let mut uart_buffer: UartBuffer = [0u8; 128];
    let mut lora_buffer = [0u8; 128];
    loop {
        match select(
            module.host_uart_read_until_idle(&mut uart_buffer),
            module.lora_receive(&mut lora_buffer),
        )
        .await
        {
            Either::First(uart_result) => match uart_result {
                Ok(size) => {
                    //info!("size {}", size);
                    match gateway
                        .process_host_message(&mut module, &uart_buffer[..size])
                        .await
                    {
                        Ok(()) => {}
                        Err(e) => {
                            error!("{}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("uart: {}", e);
                }
            },
            Either::Second(lora_result) => match lora_result {
                Ok(len) => {
                    match 
                }
            }
        }
    }
}
