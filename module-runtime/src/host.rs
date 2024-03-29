use defmt::error;
use embassy_stm32::peripherals;
use embassy_stm32::usart::{self, Uart};

const HOST_UART_BUFFER_SIZE: usize = 512;

#[derive(Debug, defmt::Format)]
pub enum HostError {
    NoData,
    DataTooLong,
}

pub struct ModuleHost {
    pub uart: Uart<'static, peripherals::LPUART1, peripherals::DMA1_CH3, peripherals::DMA1_CH4>,
}

impl ModuleHost {
    pub async fn read(&mut self, buffer: &mut [u8]) -> Result<usize, usart::Error> {
        let mut buff = [0u8; HOST_UART_BUFFER_SIZE];
        let len = self.uart.read_until_idle(&mut buff).await?;
        //info!("RX {}: {:?}", len, &buff[..len]);
        match maxval_decode(&buff[..len], buffer, 254) {
            Ok(len) => Ok(len),
            Err(e) => {
                error!("uart decode: {:?}", e);
                Err(usart::Error::BufferTooLong)
            }
        }
    }

    pub async fn write(&mut self, buffer: &[u8]) -> Result<(), usart::Error> {
        let mut buff = [0u8; HOST_UART_BUFFER_SIZE];
        let len = match maxval_encode(buffer, &mut buff, 254) {
            Ok(len) => len,
            Err(e) => {
                error!("uart decode: {:?}", e);
                return Err(usart::Error::BufferTooLong);
            }
        };
        //info!("TX {}: {:?}", len, &buff[..len]);
        self.uart.write(&buff[..len]).await
    }
}

fn maxval_encode(data_in: &[u8], data_out: &mut [u8], max_val: u8) -> Result<usize, HostError> {
    if data_in.len() == 0 {
        return Err(HostError::NoData);
    }
    let mut i = 0;
    let mut j = 0;
    while i < data_in.len() {
        if j >= data_out.len() - 1 {
            return Err(HostError::DataTooLong);
        }
        if data_in[i] >= max_val {
            data_out[j] = max_val;
            data_out[j + 1] = data_in[i] - max_val;
            j += 2;
        } else {
            data_out[j] = data_in[i];
            j += 1;
        }
        i += 1;
    }
    if j >= data_out.len() {
        return Err(HostError::DataTooLong);
    }
    data_out[j] = 0xff; // terminator
    j += 1;
    Ok(j as usize)
}

fn maxval_decode(data_in: &[u8], data_out: &mut [u8], max_val: u8) -> Result<usize, HostError> {
    if data_in.len() == 0 {
        return Err(HostError::NoData);
    }
    let mut i = 0;
    let mut j = 0;
    let mut next_add = false;
    while i < data_in.len() {
        if j >= data_out.len() {
            return Err(HostError::DataTooLong);
        }
        if data_in[i] == max_val {
            next_add = true;
            i += 1;
            continue;
        }
        data_out[j] = if next_add {
            data_in[i] + max_val
        } else {
            data_in[i]
        };
        j += 1;
        next_add = false;
        i += 1;
    }
    // to account for the terminator
    Ok((j - 1) as usize)
}
