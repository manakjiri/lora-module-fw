use crate::lora::*;
use defmt::*;
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
    InvalidPacketType,
    AlreadyStarted,
    NotStarted,
}

pub(super) mod err {
    pub fn deserialize(_: postcard::Error) -> super::OtaError {
        super::OtaError::Deserialize
    }

    pub fn serialize(_: postcard::Error) -> super::OtaError {
        super::OtaError::Serialize
    }

    pub fn transmit(_: super::RadioError) -> super::OtaError {
        super::OtaError::Transmit
    }

    pub fn receive(_: super::RadioError) -> super::OtaError {
        super::OtaError::Receive
    }
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

pub(super) async fn lora_transmit(
    lora: &mut ModuleLoRa,
    packet: &OtaPacket,
) -> Result<(), OtaError> {
    let mut tx_buffer = [0u8; PAYLOAD_LENGTH];
    let s = postcard::to_slice(packet, &mut tx_buffer).map_err(err::serialize)?;
    lora.transmit(&s).await.map_err(err::transmit)
}

pub(super) async fn lora_transmit_until_response(
    lora: &mut ModuleLoRa,
    packet: &OtaPacket,
    retries: usize,
) -> Result<OtaPacket, OtaError> {
    let mut tx_buffer = [0u8; PAYLOAD_LENGTH];
    let packet = postcard::to_slice(packet, &mut tx_buffer).map_err(err::serialize)?;
    let mut last_error: Option<OtaError> = None;
    for _ in 0..retries {
        let mut rx_buffer = [0u8; PAYLOAD_LENGTH];
        lora.transmit(&packet).await.map_err(err::transmit)?;
        match lora.receive_single(&mut rx_buffer).await {
            Ok(len) => match postcard::from_bytes::<OtaPacket>(&rx_buffer[..len])
                .map_err(err::deserialize)
            {
                Ok(ret) => return Ok(ret),
                Err(e) => {
                    warn!("{}", e);
                    last_error = Some(e);
                }
            },
            Err(e) => {
                warn!("{}", e);
                last_error = Some(err::receive(e));
            }
        }
        Timer::after_millis(100).await;
    }
    Err(last_error.unwrap())
}
