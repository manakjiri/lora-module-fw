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

#[derive(Debug, defmt::Format, PartialEq)]
enum Error {
    MessageTooShort,
    MessageUnknownType,
    Usart(usart::Error),
    LoRa(RadioError),
    SerDe(postcard::Error),
    Ota(OtaError),
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
        let mut sha = [0u8; 32];
        sha.copy_from_slice(&uart_buffer[6..38]);
        let mut ota = OtaProducer::new(OtaInitPacket {
            binary_size: u32::from_le_bytes(uart_buffer[0..4].try_into().unwrap()),
            block_size: u16::from_le_bytes(uart_buffer[4..6].try_into().unwrap()),
            binary_sha256: sha,
        });
        ota.init_download(host, lora).await.map_err(Error::Ota)?;
        self.ota = Some(ota);
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
                        index: u16::from_le_bytes(uart_buffer[0..2].try_into().unwrap()),
                        data: uart_buffer[4..].iter().cloned().collect(),
                    },
                )
                .await
                .map_err(Error::Ota)?;
            }
            None => {
                return Err(Error::Ota(OtaError::NotStarted));
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
                    lora.message(&uart_buffer[1..], &mut rx[1..])
                        .await
                        .map_err(Error::LoRa)?;
                    rx[0] = 1;
                    host.write(uart_buffer).await.map_err(Error::Usart)?;
                }
            }
            10 => {
                info!("init download");
                match self.ota.as_mut() {
                    Some(ota) => {
                        if ota.is_done() {
                            self.init_download(host, lora, &uart_buffer[1..]).await?;
                        } else {
                            return Err(Error::Ota(OtaError::AlreadyStarted));
                        }
                    }
                    None => {
                        self.init_download(host, lora, &uart_buffer[1..]).await?;
                    }
                }
            }
            11 => {
                info!("continue download");
                self.continue_download(host, lora, &uart_buffer[1..])
                    .await?;
            }
            12 => {
                if let Some(ota) = self.ota.as_mut().take() {
                    ota.abort_download(host, lora).await.map_err(Error::Ota)?;
                }
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
        host: &mut ModuleHost,
        lora: &mut ModuleLoRa,
        lora_buffer: &[u8],
    ) -> Result<(), Error> {
        match self.ota.as_mut() {
            Some(ota) => ota
                .process_response(host, lora, lora_buffer)
                .await
                .map_err(Error::Ota)?,
            None => {}
        }
        Ok(())
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let module = init(ModuleConfig::new(ModuleVersion::NucleoWL55JC)).await;
    let mut gateway = Gateway::new();
    info!("hello from gateway");

    _spawner.spawn(status_led_task(module.led)).unwrap();

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
        STATUS_LED.send(LedCommand::FlashShort).await;
    }
}
