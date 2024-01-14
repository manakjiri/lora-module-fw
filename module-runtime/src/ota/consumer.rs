use crate::lora::*;
use crate::ota::common::*;
use defmt::*;
use heapless::Vec;

pub struct OtaConsumer {
    params: Option<OtaInitPacket>,
    recent_indexes: Vec<u16, 32>,
    temp_buffer: [u8; 1024 * 16],
}

impl OtaConsumer {
    pub fn new() -> OtaConsumer {
        OtaConsumer {
            params: None,
            recent_indexes: Vec::new(),
            temp_buffer: [0u8; 1024 * 16],
        }
    }

    async fn handle_init(
        &mut self,
        lora: &mut ModuleLoRa,
        init: OtaInitPacket,
    ) -> Result<(), OtaError> {
        info!("init download");
        self.params = Some(init);
        self.recent_indexes.clear();
        lora_transmit(lora, &OtaPacket::InitAck).await
    }

    async fn handle_data(
        &mut self,
        lora: &mut ModuleLoRa,
        data: OtaDataPacket,
    ) -> Result<(), OtaError> {
        info!("data: index {}", data.index);
        let begin = match &self.params {
            Some(p) => (p.block_size * data.index) as usize,
            None => {
                return Err(OtaError::InvalidPacketType);
            }
        };
        let end = begin + data.data.len();
        self.temp_buffer[begin..end].copy_from_slice(&data.data);

        if !self.recent_indexes.contains(&data.index) {
            if self.recent_indexes.is_full() {
                self.recent_indexes.remove(0);
            }
            let _ = self.recent_indexes.push(data.index);
        }

        lora_transmit(
            lora,
            &OtaPacket::Status(OtaStatusPacket {
                received_indexes: self.recent_indexes.iter().cloned().collect(),
            }),
        )
        .await
    }

    async fn handle_abort(&mut self, lora: &mut ModuleLoRa) -> Result<(), OtaError> {
        info!("abort download");
        lora_transmit(lora, &OtaPacket::AbortAck).await
    }

    pub async fn process_message(
        &mut self,
        lora: &mut ModuleLoRa,
        message: &[u8],
    ) -> Result<(), OtaError> {
        match postcard::from_bytes::<OtaPacket>(message).map_err(err::deserialize)? {
            OtaPacket::Init(init) => self.handle_init(lora, init).await,
            OtaPacket::Data(data) => self.handle_data(lora, data).await,
            OtaPacket::InitAck => return Err(OtaError::InvalidPacketType),
            OtaPacket::Status(_) => return Err(OtaError::InvalidPacketType),
            OtaPacket::Abort => self.handle_abort(lora).await,
            OtaPacket::AbortAck => return Err(OtaError::InvalidPacketType),
        }
    }
}
