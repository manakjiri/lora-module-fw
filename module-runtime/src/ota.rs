use core::ops::Deref;
use heapless::Vec;
use postcard::{from_bytes, to_vec};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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
    pub data: Vec<u8, 128>,
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

/* pub struct OtaProducer {
    binary_size: u32,
    binary_sha256: [u8; 32],
    block_size: u16,
}

enum ConsumerState {}

pub struct OtaConsumer {
    binary_size: u32,
    binary_sha256: [u8; 32],
    block_size: u16,
}

impl OtaConsumer {}
 */
/* impl OtaProducer {
    pub fn new(binary_size: u32, binary_sha256: [u8; 32], block_size: u16) -> OtaProducer {
        OtaProducer {
            binary_size,
            binary_sha256,
            block_size,
        }
    }

    pub fn create_init_packet(&self) -> Vec<u8, 128> {
        to_vec(&OtaPacket::Init(OtaInitPacket {
            binary_size: self.binary_size,
            binary_sha256: self.binary_sha256,
            block_size: self.block_size,
        }))
        .unwrap()
    }
} */

/* let message = "hElLo";
let bytes = [0x01, 0x10, 0x02, 0x20];
let output: Vec<u8, 11> = to_vec(&RefStruct {
    bytes: &bytes,
    str_s: message,
}).unwrap();

assert_eq!(
    &[0x04, 0x01, 0x10, 0x02, 0x20, 0x05, b'h', b'E', b'l', b'L', b'o',],
    output.deref()
);

let out: RefStruct = from_bytes(output.deref()).unwrap();
assert_eq!(
    out,
    RefStruct {
        bytes: &bytes,
        str_s: message,
    }
); */
