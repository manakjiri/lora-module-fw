use crate::lora::*;
use crate::ota::common::*;
use defmt::*;
use gateway_host_schema::{self, GatewayPacket, OtaStatus};
use heapless::Vec;

#[derive(Debug, defmt::Format, PartialEq)]
pub enum OtaProducerState {
    Init,
    Download,
    Done,
}

pub struct OtaProducer {
    params: OtaInitPacket,
    destination_address: usize,
    state: OtaProducerState,
    data_cache: Vec<OtaDataPacket, 8>,
    not_acked_indexes: Vec<u16, 128>,
    highest_sent_index: u16,
    last_acked_index: u16,
}

impl OtaProducer {
    pub fn new(params: OtaInitPacket, destination_address: usize) -> OtaProducer {
        OtaProducer {
            params,
            destination_address,
            state: OtaProducerState::Init,
            data_cache: Vec::new(),
            not_acked_indexes: Vec::new(),
            highest_sent_index: 0,
            last_acked_index: 0,
        }
    }

    pub fn is_done(&self) -> bool {
        self.state == OtaProducerState::Done
    }

    pub fn get_status(&self) -> OtaStatus {
        OtaStatus {
            not_acked: self.not_acked_indexes.iter().cloned().collect(),
            in_progress: self.state == OtaProducerState::Download,
            last_acked: self.last_acked_index,
        }
    }

    async fn process_status(
        &mut self,
        _lora: &mut ModuleLoRa,
        status: OtaStatusPacket,
    ) -> Result<GatewayPacket, OtaError> {
        // remove all acknowledged indexes from the internal registry
        for received in status.received_indexes.as_slice() {
            if let Some(i) = self.not_acked_indexes.iter().position(|i| *i == *received) {
                self.not_acked_indexes.swap_remove(i);
            }
        }
        self.last_acked_index = status.valid_up_to_index;
        info!(
            "status: pend {}, ack {}, upto {}",
            self.not_acked_indexes.as_slice(),
            status.received_indexes.as_slice(),
            status.valid_up_to_index
        );
        // all blocks are acked and the last block has been already sent (thus also acked)
        if self.not_acked_indexes.is_empty()
            && self.highest_sent_index as u32
                == (self.params.binary_size / (self.params.block_size as u32))
        {
            self.state = OtaProducerState::Done;
            info!("ota producer done");
            Ok(GatewayPacket::OtaDoneAck)
            //TODO transmit done to the node
        } else {
            Ok(GatewayPacket::OtaStatus(self.get_status()))
        }
    }

    pub async fn process_response(
        &mut self,
        lora: &mut ModuleLoRa,
        packet: OtaPacket,
    ) -> Result<GatewayPacket, OtaError> {
        match packet {
            OtaPacket::Init(_) => return Err(OtaError::InvalidPacketType),
            OtaPacket::Data(_) => return Err(OtaError::InvalidPacketType),
            OtaPacket::Done => return Err(OtaError::InvalidPacketType),
            OtaPacket::Abort => return Err(OtaError::InvalidPacketType),
            OtaPacket::InitAck => {
                if self.state == OtaProducerState::Init {
                    self.state = OtaProducerState::Download;
                    Ok(GatewayPacket::OtaInitAck)
                } else {
                    Err(OtaError::InvalidPacketType)
                }
            }
            OtaPacket::Status(status) => {
                if self.state != OtaProducerState::Init {
                    self.process_status(lora, status).await
                } else {
                    Err(OtaError::InvalidPacketType)
                }
            }
            OtaPacket::DoneAck => {
                self.state = OtaProducerState::Done;
                Ok(GatewayPacket::OtaDoneAck)
            }
            OtaPacket::AbortAck => {
                self.state = OtaProducerState::Done;
                Ok(GatewayPacket::OtaAbortAck)
            }
        }
    }

    pub async fn process_response_raw(
        &mut self,
        lora: &mut ModuleLoRa,
        packet: LoRaPacket,
    ) -> Result<GatewayPacket, OtaError> {
        self.process_response(
            lora,
            postcard::from_bytes::<OtaPacket>(&packet.payload).map_err(err::deserialize)?,
        )
        .await
    }

    pub async fn init_download(
        &mut self,
        lora: &mut ModuleLoRa,
    ) -> Result<GatewayPacket, OtaError> {
        let packet = OtaPacket::Init(self.params.clone());
        let resp =
            lora_transmit_until_response(lora, self.destination_address, &packet, 10).await?;
        self.process_response(lora, resp).await
    }

    pub async fn continue_download(
        &mut self,
        lora: &mut ModuleLoRa,
        data: OtaDataPacket,
    ) -> Result<(), OtaError> {
        let current_index = data.index;
        info!("data: index {}", data.index);
        lora_transmit(lora, self.destination_address, &OtaPacket::Data(data)).await?;

        if !self.not_acked_indexes.contains(&current_index) {
            //TODO handle the case where this would overflow - we have too many unACKED, need to throttle transmit
            self.not_acked_indexes.push(current_index).unwrap();
        }
        if current_index > self.highest_sent_index {
            self.highest_sent_index = current_index;
        }
        Ok(())
    }

    pub async fn done_download(
        &mut self,
        lora: &mut ModuleLoRa,
    ) -> Result<GatewayPacket, OtaError> {
        let resp =
            lora_transmit_until_response(lora, self.destination_address, &OtaPacket::Done, 10)
                .await?;
        self.process_response(lora, resp).await
    }

    pub async fn abort_download(
        &mut self,
        lora: &mut ModuleLoRa,
    ) -> Result<GatewayPacket, OtaError> {
        let resp =
            lora_transmit_until_response(lora, self.destination_address, &OtaPacket::Abort, 10)
                .await?;
        self.process_response(lora, resp).await
    }
}
