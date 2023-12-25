use crate::{host::*, lora::*};
use defmt::*;
use embassy_time::Timer;
use gateway_host_schema::{self, GatewayPacket, HostPacket, OtaStatus};
use heapless::Vec;
use lora_phy::mod_params::RadioError;
use serde::{Deserialize, Serialize};

#[derive(Debug, defmt::Format, PartialEq)]
pub enum OtaError {
    Deserialize,
    Serialize,
    Transmit,
    Receive,
    InvalidPacketType,
    AlreadyStarted,
    NotStarted,
}

fn err_deserialize(_: postcard::Error) -> OtaError {
    OtaError::Deserialize
}

fn err_serialize(_: postcard::Error) -> OtaError {
    OtaError::Serialize
}

fn err_transmit(_: RadioError) -> OtaError {
    OtaError::Transmit
}

fn err_receive(_: RadioError) -> OtaError {
    OtaError::Receive
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Clone)]
/* sent by the gateway to node */
pub struct OtaInitPacket {
    pub binary_size: u32,
    pub binary_sha256: [u8; 32],
    pub block_size: u16,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
/* sent by the gateway to node */
pub struct OtaDataPacket {
    pub index: u16, // index of this block
    pub data: Vec<u8, 96>,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
/* sent by the node to gateway */
pub struct OtaStatusPacket {
    /* array of the most recently received indexes
    node purposefully includes index numbers that it already sent previously
    because these ACKs may get lost, by doing this we try to minimize the number
    of redundantly retransmitted data packets */
    pub received_indexes: Vec<u16, 32>,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
pub enum OtaPacket {
    Init(OtaInitPacket),
    InitAck,
    Data(OtaDataPacket),
    Status(OtaStatusPacket),
    Abort,
    AbortAck,
}

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

    async fn process_status(
        &mut self,
        _lora: &mut ModuleLoRa,
        status: OtaStatusPacket,
    ) -> Result<GatewayPacket, OtaError> {
        // remove all acknowledged indexes from the internal registry
        //info! {"ACK {}", status.received_indexes};
        for received in status.received_indexes {
            self.not_acked_indexes
                .iter()
                .position(|i| *i == received)
                .map(|i| self.not_acked_indexes.swap_remove(i));
        }
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
            Ok(GatewayPacket::OtaStatus(OtaStatus {
                not_acked: self.not_acked_indexes.iter().cloned().collect(),
            }))
        }
    }

    pub async fn process_response(
        &mut self,
        lora: &mut ModuleLoRa,
        buffer: &[u8],
    ) -> Result<GatewayPacket, OtaError> {
        match postcard::from_bytes::<OtaPacket>(buffer).map_err(err_deserialize)? {
            OtaPacket::Init(_) => return Err(OtaError::InvalidPacketType),
            OtaPacket::Data(_) => return Err(OtaError::InvalidPacketType),
            OtaPacket::Abort => return Err(OtaError::InvalidPacketType),
            OtaPacket::InitAck => {
                if self.state == OtaProducerState::Init {
                    self.state = OtaProducerState::Download;
                    Ok(GatewayPacket::OtaInitAck)
                } else {
                    return Err(OtaError::InvalidPacketType);
                }
            }
            OtaPacket::Status(status) => {
                if self.state != OtaProducerState::Init {
                    self.process_status(lora, status).await
                } else {
                    return Err(OtaError::InvalidPacketType);
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
            .map_err(err_serialize)?;

        let mut last_error: Option<OtaError> = None;
        for _ in 0..5 {
            let mut rx_buffer = [0u8; 128];
            lora.transmit(&packet).await.map_err(err_transmit)?;
            match lora.receive(&mut rx_buffer).await {
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

        lora.transmit(
            postcard::to_slice(&OtaPacket::Data(data), &mut tx_buffer).map_err(err_serialize)?,
        )
        .await
        .map_err(err_transmit)?;

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
            postcard::to_slice(&OtaPacket::Abort, &mut tx_buffer).map_err(err_serialize)?;

        //TODO move to a function
        let mut last_error: Option<OtaError> = None;
        for _ in 0..10 {
            let mut rx_buffer = [0u8; 128];
            lora.transmit(&packet).await.map_err(err_transmit)?;
            match lora.receive(&mut rx_buffer).await {
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

        let mut tx_buffer = [0u8; 128];
        lora.transmit(
            &postcard::to_slice(&OtaPacket::InitAck, &mut tx_buffer).map_err(err_serialize)?,
        )
        .await
        .map_err(err_transmit)
    }

    async fn handle_data(
        &mut self,
        lora: &mut ModuleLoRa,
        data: OtaDataPacket,
    ) -> Result<(), OtaError> {
        //info!("continue download");
        let begin = match &self.params {
            Some(p) => (p.block_size * data.index) as usize,
            None => {
                return Err(OtaError::InvalidPacketType);
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
        lora.transmit(
            &postcard::to_slice(&OtaPacket::Status(packet), &mut tx_buffer)
                .map_err(err_serialize)?,
        )
        .await
        .map_err(err_transmit)
    }

    async fn handle_abort(&mut self, lora: &mut ModuleLoRa) -> Result<(), OtaError> {
        info!("abort download");

        let mut tx_buffer = [0u8; 128];
        lora.transmit(
            &postcard::to_slice(&OtaPacket::AbortAck, &mut tx_buffer).map_err(err_serialize)?,
        )
        .await
        .map_err(err_transmit)
    }

    pub async fn process_message(
        &mut self,
        lora: &mut ModuleLoRa,
        message: &[u8],
    ) -> Result<(), OtaError> {
        match postcard::from_bytes::<OtaPacket>(message).map_err(err_deserialize)? {
            OtaPacket::Init(init) => self.handle_init(lora, init).await,
            OtaPacket::Data(data) => self.handle_data(lora, data).await,
            OtaPacket::InitAck => return Err(OtaError::InvalidPacketType),
            OtaPacket::Status(_) => return Err(OtaError::InvalidPacketType),
            OtaPacket::Abort => self.handle_abort(lora).await,
            OtaPacket::AbortAck => return Err(OtaError::InvalidPacketType),
        }
    }
}
