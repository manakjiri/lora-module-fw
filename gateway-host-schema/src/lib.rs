#![no_std]

use heapless::Vec;
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub struct OtaInitRequest {
    pub binary_size: u32,
    pub binary_sha256: [u8; 32],
    pub block_size: u16,
}

#[derive(Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct OtaData {
    pub index: u16, // index of this block
    pub data: Vec<u8, 96>,
}

#[derive(Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct OtaStatus {
    pub not_acked: Vec<u16, 128>,
}

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub enum HostPacket {
    PingRequest,
    OtaInit(OtaInitRequest),
    OtaData(OtaData),
    OtaAbort,
}

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub enum GatewayPacket {
    PingResponse,
    OtaInitAck,
    OtaStatus(OtaStatus),
    OtaDone,
    OtaAbortAck,
}
