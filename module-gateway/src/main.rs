#![no_main]
#![no_std]
#![macro_use]
#![feature(type_alias_impl_trait)]

use defmt::*;
use embassy_executor::Spawner;
use embassy_futures::select::*;
use embassy_stm32::usart;
use lora_phy::mod_params::RadioError;
use module_runtime::{embassy_time::Timer, *};
use ota::*;

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
    lora: &mut ModuleLoRa,
    tx_buffer: &[u8],
    rx_buffer: &mut [u8],
) -> Result<usize, Error> {
    lora.transmit(tx_buffer).await.map_err(Error::LoRa)?;
    lora.receive(rx_buffer).await.map_err(Error::LoRa)
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
        _host: &mut ModuleHost,
        _lora: &mut ModuleLoRa,
        _status: OtaStatusPacket,
    ) -> Result<(), Error> {
        Ok(())
    }

    fn process_response(
        &mut self,
        host: &mut ModuleHost,
        lora: &mut ModuleLoRa,
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
                    self.process_status(host, lora, status)
                } else {
                    return Err(Error::Ota(OtaError::OtaInvalidPacketType));
                }
            }
        }
    }

    async fn init_download(
        &mut self,
        host: &mut ModuleHost,
        lora: &mut ModuleLoRa,
        init: OtaInitPacket,
    ) -> Result<(), Error> {
        let mut tx_buffer = [0u8; 128];
        let packet =
            postcard::to_slice(&OtaPacket::Init(init), &mut tx_buffer).map_err(Error::SerDe)?;

        let mut last_error: Option<Error> = None;
        for _ in 0..5 {
            let mut rx_buffer = [0u8; 128];
            match lora_message(lora, &packet, &mut rx_buffer).await {
                Ok(len) => match self.process_response(host, lora, &rx_buffer[..len]) {
                    Ok(()) => return Ok(()),
                    Err(e) => {
                        last_error = Some(e);
                    }
                },
                Err(e) => {
                    last_error = Some(e);
                }
            }
            Timer::after_millis(100).await;
        }
        Err(last_error.unwrap())
    }

    async fn continue_download(
        &mut self,
        _host: &mut ModuleHost,
        lora: &mut ModuleLoRa,
        data: OtaDataPacket,
    ) -> Result<(), Error> {
        let mut tx_buffer = [0u8; 128];
        lora.transmit(
            postcard::to_slice(&OtaPacket::Data(data), &mut tx_buffer).map_err(Error::SerDe)?,
        )
        .await
        .map_err(Error::LoRa)
    }
}

struct Gateway {
    ota: Option<OtaProducer>,
}

impl Gateway {
    fn new() -> Gateway {
        Gateway { ota: None }
    }

    async fn init_download(
        &mut self,
        host: &mut ModuleHost,
        lora: &mut ModuleLoRa,
        uart_buffer: &[u8],
    ) -> Result<(), Error> {
        match self.ota {
            Some(_) => {
                return Err(Error::Ota(OtaError::OtaAlreadyStarted));
            }
            None => {
                let mut ota = OtaProducer::new();
                let mut sha = [0u8; 32];
                sha.copy_from_slice(&uart_buffer[6..38]);
                ota.init_download(
                    host,
                    lora,
                    OtaInitPacket {
                        binary_size: u32::from_be_bytes(uart_buffer[0..4].try_into().unwrap()),
                        block_size: u16::from_be_bytes(uart_buffer[4..6].try_into().unwrap()),
                        binary_sha256: sha,
                    },
                )
                .await?;
                self.ota = Some(ota);
            }
        }
        Ok(())
    }

    async fn continue_download(
        &mut self,
        host: &mut ModuleHost,
        lora: &mut ModuleLoRa,
        uart_buffer: &[u8],
    ) -> Result<(), Error> {
        match self.ota.as_mut() {
            Some(ota) => {
                ota.continue_download(
                    host,
                    lora,
                    OtaDataPacket {
                        index: u16::from_be_bytes(uart_buffer[0..4].try_into().unwrap()),
                        data: uart_buffer[4..].iter().cloned().collect(),
                    },
                )
                .await?;
            }
            None => {
                return Err(Error::Ota(OtaError::OtaNotStarted));
            }
        }
        Ok(())
    }

    async fn process_host_message(
        &mut self,
        host: &mut ModuleHost,
        lora: &mut ModuleLoRa,
        uart_buffer: &[u8],
    ) -> Result<(), Error> {
        if uart_buffer.len() == 0 {
            return Err(Error::MessageTooShort);
        }
        match uart_buffer[0] {
            // ping
            0 => {
                host.write(uart_buffer).await.map_err(Error::Usart)?;
            }
            // transmit lora
            1 => {
                if uart_buffer.len() > 1 {
                    let mut rx = [0u8; 128];
                    lora_message(lora, &uart_buffer[1..], &mut rx[1..]).await?;
                    rx[0] = 1;
                    host.write(uart_buffer).await.map_err(Error::Usart)?;
                }
            }
            10 => {
                self.init_download(host, lora, &uart_buffer[1..]).await?;
            }
            11 => {
                self.continue_download(host, lora, &uart_buffer[1..])
                    .await?;
            }
            // unhandled
            _ => {
                return Err(Error::MessageUnknownType);
            }
        }
        Ok(())
    }

    async fn process_peer_message(
        &mut self,
        _host: &mut ModuleHost,
        _lora: &mut ModuleLoRa,
        _lora_buffer: &[u8],
    ) -> Result<(), Error> {
        match self.ota.as_mut() {
            Some(_ota) => {}
            None => {}
        }
        Ok(())
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let module = init(ModuleConfig::new(ModuleVersion::NucleoWL55JC)).await;
    let mut gateway = Gateway::new();

    let mut host = module.host;
    let mut lora = module.lora;

    let mut uart_buffer = [0u8; 128];
    let mut lora_buffer = [0u8; 128];
    loop {
        let interfaces = select(host.read(&mut uart_buffer), lora.receive(&mut lora_buffer));
        match interfaces.await {
            Either::First(uart_result) => match uart_result {
                Ok(size) => {
                    //info!("size {}", size);
                    match gateway
                        .process_host_message(&mut host, &mut lora, &uart_buffer[..size])
                        .await
                    {
                        Ok(()) => {}
                        Err(e) => {
                            error!("uart: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("uart: {}", e);
                }
            },
            Either::Second(lora_result) => match lora_result {
                Ok(len) => {
                    match gateway
                        .process_peer_message(&mut host, &mut lora, &lora_buffer[..len])
                        .await
                    {
                        Ok(()) => {}
                        Err(e) => {
                            error!("lora: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("lora: {}", e);
                }
            },
        }
    }
}
