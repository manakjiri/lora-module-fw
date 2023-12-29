#![no_main]
#![no_std]
#![macro_use]
#![feature(type_alias_impl_trait)]

use defmt::*;
use embassy_executor::Spawner;
use embassy_futures::select::*;
use embassy_stm32::usart;
use gateway_host_schema::{self, HostPacket};
//use lora_phy::mod_params::RadioError;
use module_runtime::{gateway_host_schema::GatewayPacket, heapless::Vec, *};

#[derive(Debug, defmt::Format, PartialEq)]
enum Error {
    Usart(usart::Error),
    //LoRa(RadioError),
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

    async fn host_write(
        &mut self,
        host: &mut ModuleHost,
        packet: GatewayPacket,
    ) -> Result<(), Error> {
        let mut tx_buffer = [0u8; 256];
        host.write(&postcard::to_slice(&packet, &mut tx_buffer).map_err(Error::SerDe)?)
            .await
            .map_err(Error::Usart)
    }

    async fn init_download(
        &mut self,
        host: &mut ModuleHost,
        lora: &mut ModuleLoRa,
        init: gateway_host_schema::OtaInitRequest,
    ) -> Result<(), Error> {
        let mut ota = OtaProducer::new(OtaInitPacket {
            binary_size: init.binary_size,
            block_size: init.block_size,
            binary_sha256: init.binary_sha256,
        });
        self.host_write(host, ota.init_download(lora).await.map_err(Error::Ota)?)
            .await?;
        self.ota = Some(ota);
        Ok(())
    }

    async fn continue_download(
        &mut self,
        _host: &mut ModuleHost,
        lora: &mut ModuleLoRa,
        data: gateway_host_schema::OtaData,
    ) -> Result<(), Error> {
        match self.ota.as_mut() {
            Some(ota) => {
                ota.continue_download(
                    lora,
                    OtaDataPacket {
                        index: data.index,
                        data: data.data,
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
        match postcard::from_bytes::<HostPacket>(uart_buffer).map_err(Error::SerDe)? {
            HostPacket::PingRequest => {
                self.host_write(host, GatewayPacket::PingResponse).await?;
            }
            HostPacket::OtaInit(init) => {
                info!("init download");
                match self.ota.as_mut() {
                    Some(ota) => {
                        if ota.is_done() {
                            self.init_download(host, lora, init).await?;
                        } else {
                            return Err(Error::Ota(OtaError::AlreadyStarted));
                        }
                    }
                    None => {
                        self.init_download(host, lora, init).await?;
                    }
                }
            }
            HostPacket::OtaData(data) => {
                info!("continue download");
                self.continue_download(host, lora, data).await?;
            }
            HostPacket::OtaAbort => {
                if let Some(ota) = self.ota.as_mut().take() {
                    ota.abort_download(lora).await.map_err(Error::Ota)?;
                }
            }
            HostPacket::OtaGetStatus => {
                let packet = GatewayPacket::OtaStatus({
                    if let Some(ota) = self.ota.as_ref() {
                        ota.get_status()
                    } else {
                        gateway_host_schema::OtaStatus {
                            in_progress: false,
                            not_acked: Vec::new(),
                        }
                    }
                });
                self.host_write(host, packet).await?;
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
            Some(ota) => {
                let packet = ota
                    .process_response(lora, lora_buffer)
                    .await
                    .map_err(Error::Ota)?;

                self.host_write(host, packet).await?;
            }
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
        status_led(LedCommand::FlashShort).await;
    }
}
