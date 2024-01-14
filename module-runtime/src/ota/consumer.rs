use crate::lora::*;
use crate::ota::common::*;
use defmt::*;
use heapless::Vec;

pub struct OtaConsumer {
    params: Option<OtaInitPacket>,
    recent_indexes: Vec<u16, 32>,
    valid_up_to_index: u16,
    temp_buffer: [u8; 1024 * 16],
}

impl OtaConsumer {
    pub fn new() -> OtaConsumer {
        OtaConsumer {
            params: None,
            recent_indexes: Vec::new(),
            valid_up_to_index: 0,
            temp_buffer: [0u8; 1024 * 16],
        }
    }

    async fn handle_init(
        &mut self,
        lora: &mut ModuleLoRa,
        init: OtaInitPacket,
    ) -> Result<(), OtaError> {
        info!("init download");
        *self = OtaConsumer::new();
        self.params = Some(init);
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

        // update recent_indexes with the new index
        if !self.recent_indexes.contains(&data.index) {
            if self.recent_indexes.is_full() {
                self.recent_indexes.remove(0);
            }
            let _ = self.recent_indexes.push(data.index);
        }
        // update valid_up_to_index
        for i in self.valid_up_to_index..u16::MAX {
            if !self.recent_indexes.contains(&i) {
                break;
            }
            self.valid_up_to_index = i;
        }
        // send the data ACK
        lora_transmit(lora, &OtaPacket::Status(self.get_status())).await
    }

    async fn handle_done(&mut self, lora: &mut ModuleLoRa) -> Result<(), OtaError> {
        let block_count = match &self.params {
            Some(p) => p.block_count,
            None => {
                return Err(OtaError::InvalidPacketType);
            }
        };
        info!("done download");
        if self.valid_up_to_index == block_count {
            lora_transmit(lora, &OtaPacket::DoneAck).await
        } else {
            lora_transmit(lora, &OtaPacket::Status(self.get_status())).await
        }
    }

    async fn handle_abort(&mut self, lora: &mut ModuleLoRa) -> Result<(), OtaError> {
        info!("abort download");
        self.params = None;
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
            OtaPacket::Done => self.handle_done(lora).await,
            OtaPacket::DoneAck => return Err(OtaError::InvalidPacketType),
            OtaPacket::Abort => self.handle_abort(lora).await,
            OtaPacket::AbortAck => return Err(OtaError::InvalidPacketType),
        }
    }

    fn get_status(&self) -> OtaStatusPacket {
        OtaStatusPacket {
            received_indexes: self.recent_indexes.iter().cloned().collect(),
            valid_up_to_index: self.valid_up_to_index,
        }
    }
}
