use core::convert::TryFrom;

pub struct FlySkyFsi6 {
    analog_channels: [u16; 6],
    sa: TwoWay,
    sb: TwoWay,
    sc: ThreeWay,
    sd: TwoWay,
}

pub enum TwoWay {
    Low,
    High,
}

pub enum ThreeWay {
    Low,
    Mid,
    High,
}

impl TryFrom<&[bool; 2]> for ThreeWay {
    type Error = &'static str;

    fn try_from(value: &[bool; 2]) -> Result<Self, Self::Error> {
        match value {
            [false, false] => Ok(Self::Mid),
            [true, false] => Ok(Self::Low),
            [false, true] => Ok(Self::High),
            _ => Err("Both bools are true, invalid state"),
        }
    }
}

#[derive(Debug)]
pub enum LinearInput {
    NoCalibration,
    OngoingCalibration { start: u16, end: u16 },
    Calibrated { start: u16, mid: u16, end: u16 },
}

impl Default for LinearInput {
    fn default() -> Self {
        Self::NoCalibration
    }
}

impl LinearInput {
    const RESOLUTION: u16 = 1000;
    pub fn reset_calibration(&mut self) {
        *self = Self::NoCalibration;
    }

    /// Commits the calibration with the current value as mid point
    pub fn set_center(&mut self, v: u16) {
        match *self {
            Self::OngoingCalibration { start, end } => {
                *self = Self::Calibrated { start, end, mid: v }
            }
            Self::Calibrated { ref mut mid, .. } => *mid = v,
            _ => {}
        }
    }

    /// Get a scaled value
    ///
    /// Also processes value for calibration if one is ongoing
    pub fn get(&mut self, v: u16) -> u16 {
        // consider current value for calibration if one is ongoing
        match self {
            Self::NoCalibration => {
                *self = Self::OngoingCalibration { start: v, end: v };
            }
            Self::OngoingCalibration { start, .. } if v < *start => {
                *start = v;
            }
            Self::OngoingCalibration { end, .. } if v > *end => {
                *end = v;
            }
            _ => {}
        }

        let half_resolution = Self::RESOLUTION / 2;

        // determine start, mid and end
        //let mid = Self::RESOLUTION / 2;
        let (start, mid, end) = match self {
            Self::NoCalibration => {
                return half_resolution;
            }
            Self::OngoingCalibration { start, end } => (*start, (*start + *end) / 2, *end),
            Self::Calibrated { start, mid, end } => (*start, *mid, *end),
        };

        // limit v to the allowed range
        let mut v = num::clamp(v, start, end);

        // check in which half of the resolution we are
        let (start, end, offset) = if v < mid {
            (start, mid, 0)
        } else if v > mid {
            (mid, end, half_resolution)
        } else {
            return half_resolution;
        };

        let span = end - start;
        debug_assert!(span != 0);

        // correct integer division bias towards caused by cutting the decimals
        v += span / (Self::RESOLUTION);

        ((v - start) as u32 * half_resolution as u32 / span as u32) as u16 + offset
    }
}
