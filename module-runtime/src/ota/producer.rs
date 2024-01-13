use crate::lora::*;
use crate::ota::common::*;
use defmt::*;
use embassy_time::Timer;
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
    state: OtaProducerState,
    data_cache: Vec<OtaDataPacket, 8>,
    not_acked_indexes: Vec<u16, 128>,
    highest_sent_index: u16,
}

impl OtaProducer {
    pub fn new(params: OtaInitPacket) -> OtaProducer {
        OtaProducer {
            params,
            state: OtaProducerState::Init,
            data_cache: Vec::new(),
            not_acked_indexes: Vec::new(),
            highest_sent_index: 0,
        }
    }

    pub fn is_done(&self) -> bool {
        self.state == OtaProducerState::Done
    }

    pub fn get_status(&self) -> OtaStatus {
        OtaStatus {
            not_acked: self.not_acked_indexes.iter().cloned().collect(),
            in_progress: self.state == OtaProducerState::Download,
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
        info!(
            "status: pend {}, ack {}",
            self.not_acked_indexes.as_slice(),
            status.received_indexes.as_slice()
        );
        //TODO proper handling, now just transmit not acked to host to deal with it
        let mut tx_buffer = [0u8; 128];
        tx_buffer[0] = 21;
        for i in 0..self.not_acked_indexes.len() {
            tx_buffer[i + 1] = self.not_acked_indexes[i] as u8;
            warn!("not ACKed {}", self.not_acked_indexes[i]);
        }

        // all blocks are acked and the last block has been already sent (thus also acked)
        if self.not_acked_indexes.is_empty()
            && self.highest_sent_index as u32
                == (self.params.binary_size / (self.params.block_size as u32))
        {
            self.state = OtaProducerState::Done;
            info!("ota producer done");
            Ok(GatewayPacket::OtaDone)
            //TODO transmit done to the node
        } else {
            Ok(GatewayPacket::OtaStatus(self.get_status()))
        }
    }

    pub async fn process_response(
        &mut self,
        lora: &mut ModuleLoRa,
        buffer: &[u8],
    ) -> Result<GatewayPacket, OtaError> {
        match postcard::from_bytes::<OtaPacket>(buffer).map_err(err::deserialize)? {
            OtaPacket::Init(_) => return Err(OtaError::InvalidPacketType),
            OtaPacket::Data(_) => return Err(OtaError::InvalidPacketType),
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
            OtaPacket::AbortAck => {
                self.state = OtaProducerState::Done;
                Ok(GatewayPacket::OtaAbortAck)
            }
        }
    }

    pub async fn init_download(
        &mut self,
        lora: &mut ModuleLoRa,
    ) -> Result<GatewayPacket, OtaError> {
        let mut tx_buffer = [0u8; 128];
        let packet = postcard::to_slice(&OtaPacket::Init(self.params.clone()), &mut tx_buffer)
            .map_err(err::serialize)?;

        //TODO move to a function
        let mut last_error: Option<OtaError> = None;
        for _ in 0..5 {
            let mut rx_buffer = [0u8; 128];
            lora.transmit(&packet).await.map_err(err::transmit)?;
            match lora.receive_single(&mut rx_buffer).await {
                Ok(len) => match self.process_response(lora, &rx_buffer[..len]).await {
                    Ok(ret) => return Ok(ret),
                    Err(e) => {
                        warn!("init download error: {}", e);
                        last_error = Some(e);
                    }
                },
                Err(_e) => {
                    warn!("init download error: {}", _e);
                    last_error = Some(OtaError::Receive);
                }
            }
            Timer::after_millis(100).await;
        }
        Err(last_error.unwrap())
    }

    pub async fn continue_download(
        &mut self,
        lora: &mut ModuleLoRa,
        data: OtaDataPacket,
    ) -> Result<(), OtaError> {
        let mut tx_buffer = [0u8; 128];
        let current_index = data.index;

        info!("data: index {}", data.index);
        lora.transmit(
            postcard::to_slice(&OtaPacket::Data(data), &mut tx_buffer).map_err(err::serialize)?,
        )
        .await
        .map_err(err::transmit)?;

        self.not_acked_indexes.push(current_index).unwrap();
        if current_index > self.highest_sent_index {
            self.highest_sent_index = current_index;
        }
        Ok(())
    }

    pub async fn abort_download(
        &mut self,
        lora: &mut ModuleLoRa,
    ) -> Result<GatewayPacket, OtaError> {
        let mut tx_buffer = [0u8; 128];
        let packet =
            postcard::to_slice(&OtaPacket::Abort, &mut tx_buffer).map_err(err::serialize)?;

        //TODO move to a function
        let mut last_error: Option<OtaError> = None;
        for _ in 0..10 {
            let mut rx_buffer = [0u8; 128];
            lora.transmit(&packet).await.map_err(err::transmit)?;
            match lora.receive_single(&mut rx_buffer).await {
                Ok(len) => match self.process_response(lora, &rx_buffer[..len]).await {
                    Ok(ret) => return Ok(ret),
                    Err(e) => {
                        warn!("abort download error: {}", e);
                        last_error = Some(e);
                    }
                },
                Err(_e) => {
                    warn!("abort download error: {}", _e);
                    last_error = Some(OtaError::Receive);
                }
            }
            Timer::after_millis(100).await;
        }
        Err(last_error.unwrap())
    }
}
