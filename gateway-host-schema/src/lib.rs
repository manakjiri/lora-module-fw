#![no_std]

use heapless::Vec;
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub struct OtaInitRequest {
    pub destination_address: usize,
    pub binary_size: u32,
    pub binary_sha256: [u8; 32],
    pub block_size: u16,
    pub block_count: u16,
}

#[derive(Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct OtaData {
    pub index: u16, // index of this block
    pub data: Vec<u8, 96>,
}

#[derive(Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct OtaStatus {
    pub in_progress: bool,
    pub not_acked: Vec<u16, 64>,
    pub last_acked: u16,
}

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub enum HostPacket {
    PingRequest,
    OtaGetStatus,
    OtaInit(OtaInitRequest),
    OtaData(OtaData),
    OtaDoneRequest,
    OtaAbortRequest,
}

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub enum GatewayPacket {
    PingResponse,
    OtaInitAck,
    OtaStatus(OtaStatus),
    OtaDoneAck,
    OtaAbortAck,
}
