use crate::iv::{Stm32wlInterfaceVariant, SubghzSpiDevice};
use defmt::info;
use embassy_futures::select::*;
use embassy_stm32::crc;
use embassy_stm32::gpio::{AnyPin, Output};
use embassy_stm32::peripherals;
use embassy_stm32::spi::Spi;
use embassy_time::{Delay, Timer};
use lora_phy::mod_params::*;
use lora_phy::sx1261_2::SX1261_2;
use lora_phy::LoRa;

pub const PACKET_LENGTH: usize = 128;
pub const CHECKSUM_LENGTH: usize = 4;
pub const PAYLOAD_LENGTH: usize = PACKET_LENGTH - CHECKSUM_LENGTH;

pub struct ModuleLoRa {
    pub lora: LoRa<
        SX1261_2<
            SubghzSpiDevice<
                Spi<'static, peripherals::SUBGHZSPI, peripherals::DMA1_CH1, peripherals::DMA1_CH2>,
            >,
            Stm32wlInterfaceVariant<Output<'static, AnyPin>>,
        >,
        Delay,
    >,
    pub lora_modulation: ModulationParams,
    pub lora_tx_params: PacketParams,
    pub lora_rx_params: PacketParams,
    pub crc: crc::Crc<'static>,
}

impl ModuleLoRa {
    pub async fn transmit(&mut self, tx_buffer: &[u8]) -> Result<(), RadioError> {
        if tx_buffer.len() >= PAYLOAD_LENGTH {
            return Err(RadioError::PayloadSizeUnexpected(tx_buffer.len()));
        }
        self.lora
            .prepare_for_tx(&self.lora_modulation, 14, false)
            .await?;
        //info!("TX len {}", tx_buffer.len());
        let mut buff = [0u8; PACKET_LENGTH];
        buff[..tx_buffer.len()].copy_from_slice(tx_buffer);
        self.crc.reset();
        buff[tx_buffer.len()..tx_buffer.len() + 4]
            .copy_from_slice(&self.crc.feed_bytes(tx_buffer).to_le_bytes());
        self.lora
            .tx(
                &self.lora_modulation,
                &mut self.lora_tx_params,
                &buff[..tx_buffer.len() + 4],
                10_000, // is the timeout broken? https://www.thethingsnetwork.org/airtime-calculator
            )
            .await
    }

    pub async fn receive_continuous(&mut self, rx_buffer: &mut [u8]) -> Result<usize, RadioError> {
        self.receive(rx_buffer, None).await
    }

    pub async fn receive_single(&mut self, rx_buffer: &mut [u8]) -> Result<usize, RadioError> {
        match select(self.receive_continuous(rx_buffer), Timer::after_secs(1)).await {
            Either::First(r) => r,
            Either::Second(_) => Err(RadioError::ReceiveTimeout),
        }
    }

    async fn receive(
        &mut self,
        rx_buffer: &mut [u8],
        window_in_secs: Option<u8>,
    ) -> Result<usize, RadioError> {
        self.lora
            .prepare_for_rx(
                &self.lora_modulation,
                &self.lora_rx_params,
                window_in_secs,
                None,
                false,
            )
            .await?;
        match self.lora.rx(&self.lora_rx_params, rx_buffer).await {
            Ok((received_len, _status)) => {
                let len = received_len as usize;
                //info!("RX rssi {} len {}", _status.rssi, received_len);
                if len > CHECKSUM_LENGTH {
                    let payload = &rx_buffer[..len - CHECKSUM_LENGTH];
                    let checksum: [u8; CHECKSUM_LENGTH] =
                        rx_buffer[len - CHECKSUM_LENGTH..len].try_into().unwrap();

                    self.crc.reset();
                    if self.crc.feed_bytes(payload) == u32::from_le_bytes(checksum) {
                        Ok(len)
                    } else {
                        Err(RadioError::CRCErrorOnReceive)
                    }
                } else {
                    Err(RadioError::HeaderError)
                }
            }
            Err(err) => Err(err),
        }
    }
}
