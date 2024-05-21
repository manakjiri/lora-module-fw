use embassy_futures::select::*;
use embassy_stm32::{
    exti::{AnyChannel, ExtiInput},
    gpio::{AnyPin, Level, Output, OutputOpenDrain, Pull, Speed, Flex},
};
use embassy_time::{Duration, Instant, Timer};
use module_runtime::*;

pub struct SoilSensor<'a> {
    mux_in1: Output<'a>,
    mux_in2: Output<'a>,
    mux_nen: Output<'a>,
    chg_100k: Flex<'a>,
    chg_1m: Flex<'a>,
    dischg: OutputOpenDrain<'a>,
    comp: ExtiInput<'a>,
}

#[derive(defmt::Format, Debug, Clone, Copy)]
pub enum SoilSensorResult {
    Timeout,
    Ok(u16),
}

pub enum SoilSensorRange {
    Low,
    High,
}

impl<'a> SoilSensor<'a> {
    pub fn new(
        mux_in1: AnyPin,
        mux_in2: AnyPin,
        mux_nen: AnyPin,
        chg_100k: AnyPin,
        chg_1m: AnyPin,
        dischg: AnyPin,
        comp: AnyPin,
        comp_exti: AnyChannel,
    ) -> Self {
        SoilSensor {
            mux_in1: Output::new(mux_in1, Level::High, Speed::Low),
            mux_in2: Output::new(mux_in2, Level::High, Speed::Low),
            mux_nen: Output::new(mux_nen, Level::High, Speed::Low),
            chg_100k: Flex::new(chg_100k),
            chg_1m: Flex::new(chg_1m),
            dischg: OutputOpenDrain::new(dischg, Level::High, Speed::Low, Pull::None),
            comp: ExtiInput::new(comp, comp_exti, Pull::None),
        }
    }

    pub async fn sample_current_channel(&mut self, range: SoilSensorRange) -> SoilSensorResult {
        match range {
            SoilSensorRange::Low => {
                self.chg_1m.set_as_input(Pull::None);
                self.chg_100k.set_as_output(Speed::Low);
                self.chg_100k.set_low();
            },
            SoilSensorRange::High => {
                self.chg_100k.set_as_input(Pull::None);
                self.chg_1m.set_as_output(Speed::Low);
                self.chg_1m.set_low();
            },
        }
        /* start discharging */
        self.mux_nen.set_low();
        self.dischg.set_low();
        Timer::after_micros(100).await;
        /* discharge done */
        self.dischg.set_high();
        //info!("comp: {}", comp.is_high());

        /* measure */
        //info!("measuring");
        match range {
            SoilSensorRange::Low => {
                self.chg_100k.set_high();
            },
            SoilSensorRange::High => {
                self.chg_1m.set_high();
            },
        }
        let start = Instant::now();
        let ret = match select(self.comp.wait_for_high(), Timer::after_millis(2)).await {
            Either::First(_) => {
                let mut elapsed = start.elapsed();
                match range {
                    SoilSensorRange::Low => {
                        elapsed *= 10;
                    },
                    SoilSensorRange::High => {
                        elapsed *= 1;
                    },
                }
                SoilSensorResult::Ok(elapsed.as_micros() as u16)
            },
            Either::Second(_) => SoilSensorResult::Timeout,
        };
        //info!("comp: {}", comp.is_high());
        /* stop measuring */
        self.chg_100k.set_as_input(Pull::None);
        self.chg_1m.set_as_input(Pull::None);
        self.dischg.set_low();
        self.mux_nen.set_high();
        ret
    }

    pub async fn sample_current_channel_autorange(&mut self) -> SoilSensorResult {
        let mut ret = self.sample_current_channel(SoilSensorRange::High).await;
        if let SoilSensorResult::Timeout = ret {
            ret = self.sample_current_channel(SoilSensorRange::Low).await;
        }
        ret
    }

    pub fn set_channel(&mut self, channel: u8) {
        self.mux_in1.set_level((channel & 1 == 1).then_some(Level::High).unwrap_or(Level::Low));
        self.mux_in2.set_level((channel & 2 == 2).then_some(Level::High).unwrap_or(Level::Low));
    }

    pub async fn sample_all_average(&mut self) -> [SoilSensorResult; 4] {
        const SAMPLES: usize = 10;
        let mut results = [SoilSensorResult::Timeout; 4];
        for i in 0..4 {
            self.set_channel(i);
            let mut sum = 0;
            let mut timeout = false;
            for _ in 0..SAMPLES {
                sum += match self.sample_current_channel_autorange().await {
                    SoilSensorResult::Timeout => {
                        timeout = true;
                        break;
                    },
                    SoilSensorResult::Ok(d) => d,
                };
            }
            results[i as usize] = if timeout {
                SoilSensorResult::Timeout
            } else {
                SoilSensorResult::Ok(sum / SAMPLES as u16)
            };
        }
        results
    }
}
