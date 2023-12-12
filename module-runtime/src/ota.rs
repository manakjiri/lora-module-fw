use crate::{host::*, lora::*};
use defmt::*;
use embassy_stm32::usart;
use embassy_time::Timer;
use heapless::Vec;
use lora_phy::mod_params::RadioError;
use serde::{Deserialize, Serialize};

#[derive(Debug, defmt::Format, PartialEq)]
pub enum OtaError {
    Deserialize,
    Serialize,
    Transmit,
    Receive,
    HostWrite,
    HostRead,
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

fn err_host_write(_: usart::Error) -> OtaError {
    OtaError::HostWrite
}

fn err_host_read(_: usart::Error) -> OtaError {
    OtaError::HostRead
}

fn err_transmit(_: RadioError) -> OtaError {
    OtaError::Transmit
}

fn err_receive(_: RadioError) -> OtaError {
    OtaError::Receive
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
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
}

#[derive(Debug, defmt::Format, PartialEq)]
pub enum OtaProducerState {
    Init,
    Download,
}

pub struct OtaProducer {
    pub state: OtaProducerState,
}

impl OtaProducer {
    pub fn new() -> OtaProducer {
        OtaProducer {
            state: OtaProducerState::Init,
        }
    }

    async fn process_status(
        &mut self,
        _host: &mut ModuleHost,
        _lora: &mut ModuleLoRa,
        _status: OtaStatusPacket,
    ) -> Result<(), OtaError> {
        Ok(())
    }

    pub async fn process_response(
        &mut self,
        host: &mut ModuleHost,
        lora: &mut ModuleLoRa,
        buffer: &[u8],
    ) -> Result<(), OtaError> {
        match postcard::from_bytes::<OtaPacket>(buffer).map_err(err_deserialize)? {
            OtaPacket::Init(_) => return Err(OtaError::InvalidPacketType),
            OtaPacket::Data(_) => return Err(OtaError::InvalidPacketType),
            OtaPacket::InitAck => {
                if self.state == OtaProducerState::Init {
                    self.state = OtaProducerState::Download;
                    host.write(&[20u8]).await.map_err(err_host_write)?;
                    Ok(())
                } else {
                    return Err(OtaError::InvalidPacketType);
                }
            }
            OtaPacket::Status(status) => {
                if self.state == OtaProducerState::Download {
                    self.process_status(host, lora, status).await
                } else {
                    return Err(OtaError::InvalidPacketType);
                }
            }
        }
    }

    pub async fn init_download(
        &mut self,
        host: &mut ModuleHost,
        lora: &mut ModuleLoRa,
        init: OtaInitPacket,
    ) -> Result<(), OtaError> {
        let mut tx_buffer = [0u8; 128];
        let packet =
            postcard::to_slice(&OtaPacket::Init(init), &mut tx_buffer).map_err(err_serialize)?;

        let mut last_error: Option<OtaError> = None;
        for _ in 0..3 {
            let mut rx_buffer = [0u8; 128];
            lora.transmit(&packet).await.map_err(err_transmit)?;
            match lora.receive(&mut rx_buffer).await {
                Ok(len) => match self.process_response(host, lora, &rx_buffer[..len]).await {
                    Ok(()) => return Ok(()),
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
        _host: &mut ModuleHost,
        lora: &mut ModuleLoRa,
        data: OtaDataPacket,
    ) -> Result<(), OtaError> {
        let mut tx_buffer = [0u8; 128];
        lora.transmit(
            postcard::to_slice(&OtaPacket::Data(data), &mut tx_buffer).map_err(err_serialize)?,
        )
        .await
        .map_err(err_transmit)
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
        info!("continue download");
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
        }
    }
}
