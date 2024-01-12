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

pub mod err {
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
