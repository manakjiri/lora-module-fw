use defmt::info;
use embassy_lora::iv::Stm32wlInterfaceVariant;
use embassy_stm32::crc::{self, Crc};
use embassy_stm32::gpio::{AnyPin, Level, Output, Pin, Speed};
use embassy_stm32::rcc::*;
use embassy_stm32::spi::Spi;
use embassy_stm32::time::Hertz;
use embassy_stm32::usart::{self, Uart};
use embassy_stm32::{bind_interrupts, peripherals};
use embassy_time::Delay;
use lora_phy::mod_params::*;
use lora_phy::sx1261_2::SX1261_2;
use lora_phy::LoRa;

pub struct ModuleLoRa {
    pub lora: LoRa<
        SX1261_2<
            Spi<'static, peripherals::SUBGHZSPI, peripherals::DMA1_CH1, peripherals::DMA1_CH2>,
            Stm32wlInterfaceVariant<Output<'static, AnyPin>>,
        >,
        Delay,
    >,
    pub lora_modulation: ModulationParams,
    pub lora_tx_params: PacketParams,
    pub lora_rx_params: PacketParams,
}

impl ModuleLoRa {
    pub async fn transmit(&mut self, tx_buffer: &[u8]) -> Result<(), RadioError> {
        self.lora
            .prepare_for_tx(&self.lora_modulation, 14, false)
            .await?;
        self.lora
            .tx(
                &self.lora_modulation,
                &mut self.lora_tx_params,
                tx_buffer,
                10_000, // is the timeout broken? https://www.thethingsnetwork.org/airtime-calculator
            )
            .await
    }

    pub async fn receive(&mut self, rx_buffer: &mut [u8]) -> Result<usize, RadioError> {
        self.lora
            .prepare_for_rx(
                &self.lora_modulation,
                &self.lora_rx_params,
                None,
                None,
                false,
            )
            .await?;
        match self.lora.rx(&self.lora_rx_params, rx_buffer).await {
            Ok((received_len, status)) => {
                info!("RX rssi {} len {}", status.rssi, received_len);
                Ok(received_len as usize)
            }
            Err(err) => Err(err),
        }
    }

    pub async fn message(
        &mut self,
        tx_buffer: &[u8],
        rx_buffer: &mut [u8],
    ) -> Result<usize, RadioError> {
        self.transmit(tx_buffer).await?;
        self.receive(rx_buffer).await
    }
}
