#[allow(dead_code)]
enum ButtonState {
    Up,
    Neutral,
    Down,
}

/// Channel count starts at 1
///
/// Standard AETR mapping
#[repr(packed)]
#[derive(Copy, Clone, Debug)]
pub struct JoystickState {
    /// Left Stick (Axis 0 and 1)
    pub left_x: i16,
    pub left_y: i16,

    /// Right Stick (Axis 2 and 3)
    pub right_x: i16,
    pub right_y: i16,

    /// Dials
    pub dial_1: i16,
    pub dial_2: i16,

    /// Buttons
    pub buttons: u8,
}

impl JoystickState {
    pub fn from_ppm_time() -> Self {
        let buttons: u8 = 0;

        JoystickState {
            left_x: 0,
            left_y: 0,
            right_x: 0,
            right_y: 0,
            dial_1: 0,
            dial_2: 0,
            buttons,
        }
    }

    // this is actually safe, as long as `JoystickState` is packed. More information:
    // https://stackoverflow.com/questions/28127165/how-to-convert-struct-to-u8
    /// Return a byte slice to this struct
    pub unsafe fn as_u8_slice(&self) -> &[u8] {
        ::core::slice::from_raw_parts(
            (self as *const Self) as *const u8,
            ::core::mem::size_of::<Self>(),
        )
    }
}
