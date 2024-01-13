use crate::iv::{Stm32wlInterfaceVariant, SubghzSpiDevice};
use defmt::info;
use embassy_futures::select::*;
use embassy_stm32::gpio::{AnyPin, Output};
use embassy_stm32::peripherals;
use embassy_stm32::spi::Spi;
use embassy_time::{Delay, Timer};
use lora_phy::mod_params::*;
use lora_phy::sx1261_2::SX1261_2;
use lora_phy::LoRa;

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
}

impl ModuleLoRa {
    pub async fn transmit(&mut self, tx_buffer: &[u8]) -> Result<(), RadioError> {
        self.lora
            .prepare_for_tx(&self.lora_modulation, 14, false)
            .await?;
        //info!("TX len {}", tx_buffer.len());
        self.lora
            .tx(
                &self.lora_modulation,
                &mut self.lora_tx_params,
                tx_buffer,
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
                //info!("RX rssi {} len {}", _status.rssi, received_len);
                Ok(received_len as usize)
            }
            Err(err) => Err(err),
        }
    }
}
