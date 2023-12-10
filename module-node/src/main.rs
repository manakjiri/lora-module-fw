#![no_main]
#![no_std]
#![macro_use]
#![feature(type_alias_impl_trait)]

use defmt::*;
use embassy_executor::Spawner;
use heapless::Vec;
use lora_phy::mod_params::RadioError;
use module_runtime::{futures::TryFutureExt, *};
use ota::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, defmt::Format, PartialEq)]
enum Error {
    OtaInvalidPacketType,
    LoRa(RadioError),
    SerDe(postcard::Error),
}

struct OtaConsumer {
    params: Option<OtaInitPacket>,
    recent_indexes: Vec<u16, 32>,
    temp_buffer: [u8; 1024 * 16],
}

impl OtaConsumer {
    fn new() -> OtaConsumer {
        OtaConsumer {
            params: None,
            recent_indexes: Vec::new(),
            temp_buffer: [0u8; 1024 * 16],
        }
    }

    async fn handle_init(
        &mut self,
        module: &mut ModuleInterface,
        init: OtaInitPacket,
    ) -> Result<(), Error> {
        self.params = Some(init);

        let mut tx_buffer = [0u8; 128];
        module
            .lora_transmit(
                &postcard::to_slice(&OtaPacket::InitAck, &mut tx_buffer).map_err(Error::SerDe)?,
            )
            .map_err(Error::LoRa)
            .await
    }

    async fn handle_data(
        &mut self,
        module: &mut ModuleInterface,
        data: OtaDataPacket,
    ) -> Result<(), Error> {
        let begin = match &self.params {
            Some(p) => (p.block_size * data.index) as usize,
            None => {
                return Err(Error::OtaInvalidPacketType);
            }
        };
        let end = begin + data.data.len();
        self.temp_buffer[begin..end].copy_from_slice(&data.data);

        if self.recent_indexes.is_full() {
            self.recent_indexes.pop();
        }
        let _ = self.recent_indexes.push(data.index);

        let mut tx_buffer = [0u8; 128];
        let packet = OtaStatusPacket {
            received_indexes: self.recent_indexes.iter().cloned().collect(),
        };
        module
            .lora_transmit(
                &postcard::to_slice(&OtaPacket::Status(packet), &mut tx_buffer)
                    .map_err(Error::SerDe)?,
            )
            .map_err(Error::LoRa)
            .await
    }

    async fn process_message(
        &mut self,
        module: &mut ModuleInterface,
        message: &[u8],
    ) -> Result<(), Error> {
        match postcard::from_bytes::<OtaPacket>(message).map_err(Error::SerDe)? {
            OtaPacket::Init(init) => self.handle_init(module, init).await,
            OtaPacket::Data(data) => self.handle_data(module, data).await,
            OtaPacket::InitAck => return Err(Error::OtaInvalidPacketType),
            OtaPacket::Status(_) => return Err(Error::OtaInvalidPacketType),
        }
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut module = init(ModuleConfig::new(ModuleVersion::NucleoWL55JC)).await;
    let mut ota_consumer = OtaConsumer::new();

    let mut rx_buffer = [0u8; 128];
    loop {
        match module.lora_receive(rx_buffer.as_mut()).await {
            Ok(len) => match ota_consumer
                .process_message(&mut module, &rx_buffer[..len])
                .await
            {
                Ok(()) => {}
                Err(e) => {
                    error!("ota error: {}", e)
                }
            },
            Err(e) => {
                error!("lora error: {}", e)
            }
        }
    }
}
