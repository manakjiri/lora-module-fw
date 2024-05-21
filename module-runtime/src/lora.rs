use crate::iv::{Stm32wlInterfaceVariant, SubghzSpiDevice};
use defmt::info;
use embassy_futures::select::*;
use embassy_stm32::crc;
use embassy_stm32::gpio::Output;
use embassy_stm32::peripherals;
use embassy_stm32::spi::Spi;
use embassy_time::{Delay, Timer};
use heapless::Vec;
use lora_phy::mod_params::*;
use lora_phy::sx126x::Sx126x;
use lora_phy::LoRa;

pub const PACKET_LENGTH: usize = 128;
pub const HEADER_LENGTH: usize = 5;
pub const CHECKSUM_LENGTH: usize = 4;
pub const PAYLOAD_LENGTH: usize = PACKET_LENGTH - HEADER_LENGTH - CHECKSUM_LENGTH;

#[derive(defmt::Format, Debug, Clone, Copy)]
pub enum LoRaPacketType {
    Ping,
    OTA,
    SoilSensor,
}

pub struct LoRaPacket {
    pub source: usize,
    pub destination: usize,
    pub packet_type: LoRaPacketType,
    pub payload: Vec<u8, PAYLOAD_LENGTH>,
}

pub struct ModuleLoRa {
    pub lora: LoRa<
        Sx126x<
            SubghzSpiDevice<
                Spi<'static, peripherals::SUBGHZSPI, peripherals::DMA1_CH1, peripherals::DMA1_CH2>,
            >,
            Stm32wlInterfaceVariant<Output<'static>>,
        >,
        Delay,
    >,
    pub lora_modulation: ModulationParams,
    pub crc: crc::Crc<'static>,
    pub address: usize,
}

impl LoRaPacket {
    pub fn new(destination: usize, packet_type: LoRaPacketType) -> Self {
        LoRaPacket {
            destination,
            source: 0,
            packet_type,
            payload: Vec::new(),
        }
    }

    pub fn new_with_payload(destination: usize, packet_type: LoRaPacketType, payload: Vec<u8, PAYLOAD_LENGTH>) -> Self {
        let mut ret = Self::new(destination, packet_type);
        ret.payload = payload;
        ret
    }

    pub fn parse(buff: &[u8]) -> Option<Self> {
        if buff.len() <= HEADER_LENGTH || buff.len() > PACKET_LENGTH {
            return None;
        }
        Some(LoRaPacket {
            destination: u16::from_le_bytes(buff[0..2].try_into().ok()?) as usize,
            source: u16::from_le_bytes(buff[2..4].try_into().ok()?) as usize,
            packet_type: match buff[4] {
                0 => LoRaPacketType::Ping,
                1 => LoRaPacketType::OTA,
                2 => LoRaPacketType::SoilSensor,
                _ => return None,
            },
            payload: Vec::from_slice(&buff[HEADER_LENGTH..]).ok()?,
        })
    }

    pub fn serialize(&self, buff: &mut [u8]) -> Option<usize> {
        let len = HEADER_LENGTH + self.payload.len();
        if len > buff.len() {
            return None;
        }
        buff[0..2].copy_from_slice(&(self.destination as u16).to_le_bytes());
        buff[2..4].copy_from_slice(&(self.source as u16).to_le_bytes());
        buff[4] = match self.packet_type {
            LoRaPacketType::Ping => 0,
            LoRaPacketType::OTA => 1,
            LoRaPacketType::SoilSensor => 2,
        };
        buff[HEADER_LENGTH..HEADER_LENGTH + self.payload.len()].copy_from_slice(&self.payload);
        Some(len)
    }
}

impl ModuleLoRa {
    /* sets the source address automatically */
    pub async fn transmit(&mut self, packet: &mut LoRaPacket) -> Result<(), RadioError> {
        packet.source = self.address;
        let mut buff = [0u8; PACKET_LENGTH];
        /* serialize the packet */
        let len = packet
            .serialize(buff[..PACKET_LENGTH - CHECKSUM_LENGTH].as_mut())
            .ok_or(RadioError::PayloadSizeUnexpected(packet.payload.len()))?;

        /* calculate and add the CRC at the end of the packet */
        self.crc.reset();
        let checksum = self.crc.feed_bytes(&buff[..len]).to_le_bytes();
        buff[len..len + CHECKSUM_LENGTH].copy_from_slice(&checksum);

        /* prepare for transmit */
        let mut lora_tx_params = self
            .lora
            .create_tx_packet_params(4, false, false, false, &self.lora_modulation)
            .unwrap();
        self.lora
            .prepare_for_tx(&self.lora_modulation, 15, false)
            .await?;
        info!("TX len {}", len + CHECKSUM_LENGTH);
        /* transmit the packet */
        self.lora
            .tx(
                &self.lora_modulation,
                &mut lora_tx_params,
                &buff[..len + CHECKSUM_LENGTH],
                100_000, // is the timeout broken? https://www.thethingsnetwork.org/airtime-calculator
            )
            .await
    }

    pub async fn receive_continuous(&mut self) -> Result<LoRaPacket, RadioError> {
        self.receive_addressed().await
    }

    pub async fn receive_single(&mut self) -> Result<LoRaPacket, RadioError> {
        match select(self.receive_continuous(), Timer::after_secs(5)).await {
            Either::First(r) => r,
            Either::Second(_) => Err(RadioError::ReceiveTimeout),
        }
    }

    async fn receive_addressed(&mut self) -> Result<LoRaPacket, RadioError> {
        loop {
            match self.receive().await {
                Ok(packet) => {
                    if packet.destination == self.address {
                        return Ok(packet);
                    }
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }
    }

    async fn receive(&mut self) -> Result<LoRaPacket, RadioError> {
        let lora_rx_params = self
            .lora
            .create_rx_packet_params(
                4,
                false,
                PACKET_LENGTH as u8,
                false,
                false,
                &self.lora_modulation,
            )
            .unwrap();
        self.lora
            .prepare_for_rx(
                RxMode::Continuous,
                &self.lora_modulation,
                &lora_rx_params,
                false,
            )
            .await?;
        let mut buff = [0u8; PACKET_LENGTH];
        match self.lora.rx(&lora_rx_params, &mut buff).await {
            Ok((received_len, _status)) => {
                let len = received_len as usize;
                info!("RX rssi {} len {}", _status.rssi, received_len);
                if len > CHECKSUM_LENGTH + HEADER_LENGTH {
                    let payload = &buff[..len - CHECKSUM_LENGTH];
                    let checksum = &buff[len - CHECKSUM_LENGTH..len];

                    self.crc.reset();
                    if self.crc.feed_bytes(payload)
                        == u32::from_le_bytes(checksum.try_into().unwrap())
                    {
                        Ok(LoRaPacket::parse(payload).ok_or(RadioError::Busy)?) //FIXME
                    } else {
                        Err(RadioError::Busy) //FIXME
                    }
                } else {
                    Err(RadioError::Busy) //FIXME
                }
            }
            Err(err) => Err(err),
        }
    }

    pub async fn sleep(&mut self) -> Result<(), RadioError> {
        self.lora.enter_standby().await
    }
}
