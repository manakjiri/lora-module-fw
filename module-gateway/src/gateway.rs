use defmt::*;
use gateway_host_schema::{self, HostPacket};
use module_runtime::{gateway_host_schema::GatewayPacket, heapless::Vec, *};

#[derive(Debug, defmt::Format, PartialEq)]
pub enum Error {
    Ota(OtaError),
}

pub struct Gateway {
    ota: Option<OtaProducer>,
}

impl Gateway {
    pub fn new() -> Gateway {
        Gateway { ota: None }
    }

    async fn init_download(
        &mut self,
        lora: &mut ModuleLoRa,
        init: gateway_host_schema::OtaInitRequest,
    ) -> Result<GatewayPacket, Error> {
        let mut ota = OtaProducer::new(OtaInitPacket {
            binary_size: init.binary_size,
            block_size: init.block_size,
            block_count: init.block_count,
            binary_sha256: init.binary_sha256,
        });
        let ret = ota.init_download(lora).await.map_err(Error::Ota)?;
        self.ota = Some(ota);
        Ok(ret)
    }

    async fn continue_download(
        &mut self,
        lora: &mut ModuleLoRa,
        data: gateway_host_schema::OtaData,
    ) -> Result<(), Error> {
        match self.ota.as_mut() {
            Some(ota) => {
                ota.continue_download(
                    lora,
                    OtaDataPacket {
                        index: data.index,
                        data: data.data,
                    },
                )
                .await
                .map_err(Error::Ota)?;
            }
            None => {
                return Err(Error::Ota(OtaError::NotStarted));
            }
        }
        Ok(())
    }

    pub async fn process_host_message(
        &mut self,
        lora: &mut ModuleLoRa,
        packet: HostPacket,
    ) -> Result<Option<GatewayPacket>, Error> {
        let ret = match packet {
            HostPacket::PingRequest => Some(GatewayPacket::PingResponse),
            HostPacket::OtaInit(init) => {
                info!("init download");
                match self.ota.as_mut() {
                    Some(ota) => {
                        if ota.is_done() {
                            Some(self.init_download(lora, init).await?)
                        } else {
                            return Err(Error::Ota(OtaError::AlreadyStarted));
                        }
                    }
                    None => Some(self.init_download(lora, init).await?),
                }
            }
            HostPacket::OtaData(data) => {
                //info!("continue download");
                self.continue_download(lora, data).await?;
                None
            }
            HostPacket::OtaDoneRequest => {
                if let Some(ota) = self.ota.as_mut().take() {
                    Some(ota.done_download(lora).await.map_err(Error::Ota)?)
                } else {
                    Some(GatewayPacket::OtaDoneAck) //FIXME this should return invalid command or something
                }
            }
            HostPacket::OtaAbortRequest => {
                if let Some(ota) = self.ota.as_mut().take() {
                    Some(ota.abort_download(lora).await.map_err(Error::Ota)?)
                } else {
                    Some(GatewayPacket::OtaAbortAck) //FIXME this should return invalid command or something
                }
            }
            HostPacket::OtaGetStatus => {
                let packet = GatewayPacket::OtaStatus({
                    if let Some(ota) = self.ota.as_ref() {
                        ota.get_status()
                    } else {
                        gateway_host_schema::OtaStatus {
                            in_progress: false,
                            not_acked: Vec::new(),
                            last_acked: 0,
                        }
                    }
                });
                Some(packet)
            }
        };
        Ok(ret)
    }

    pub async fn process_peer_message(
        &mut self,
        lora: &mut ModuleLoRa,
        lora_buffer: &[u8],
    ) -> Result<Option<GatewayPacket>, Error> {
        match self.ota.as_mut() {
            Some(ota) => Ok(Some(
                ota.process_response_raw(lora, lora_buffer)
                    .await
                    .map_err(Error::Ota)?,
            )),
            None => Ok(None),
        }
    }
}
