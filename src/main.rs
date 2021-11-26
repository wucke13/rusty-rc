#![no_main]
#![no_std]

mod hid;
mod inputs;
mod types;

#[cfg(feature = "rtt")]
mod panic_rtt {
    use core::panic::PanicInfo;
    #[inline(never)]
    #[panic_handler]
    fn panic(info: &PanicInfo) -> ! {
        rtt_target::rprintln!("{}", info);
        loop {} // You might need a compiler fence in here.
    }
}

#[cfg(not(feature = "rtt"))]
use panic_halt as _;

#[rtic::app(device = stm32f4xx_hal::stm32, dispatchers = [SDIO], peripherals = true)]
mod app {
    use dwt_systick_monotonic::{DwtSystick, ExtU32};

    use stm32f4xx_hal::{
        adc::{
            config::{AdcConfig, Dma, Resolution, SampleTime, Scan, Sequence},
            Adc,
        },
        dma::{config::DmaConfig, PeripheralToMemory, Stream0, StreamsTuple, Transfer},
        gpio::{EPin, Input, PullUp},
        otg_fs::{UsbBusType, USB},
        pac::DMA2,
        prelude::*,
        stm32::{ADC1, EXTI},
    };
    use usb_device::{bus::UsbBusAllocator, prelude::*};

    #[cfg(feature = "rtt")]
    use rtt_target::{rprint, rprintln, rtt_init_print};

    use crate::{hid::*, inputs::LinearInput, types::JoystickState};

    const MONO_HZ: u32 = 84_000_000; // 8 MHz
    const REPORT_PERIOD: u32 = 84_000;
    const ANALOG_PINS: usize = 6;
    const DIGITAL_PINS: usize = 10;
    const EP_MEMORY_WORDS: usize = 1024;

    type RcUsbDevice = UsbDevice<'static, UsbBusType>;
    type RcUsbClass = HIDClass<'static, UsbBusType>;

    #[monotonic(binds = SysTick, default = true)]
    type MyMono = DwtSystick<MONO_HZ>;

    type DMATransfer =
        Transfer<Stream0<DMA2>, Adc<ADC1>, PeripheralToMemory, &'static mut [u16; ANALOG_PINS], 0>;

    #[shared]
    struct Shared {
        transfer: DMATransfer,
        usb_device: RcUsbDevice,
        usb_class: RcUsbClass,
        exti: EXTI,
        user_button: EPin<Input<PullUp>>,
        analog_inputs: [u16; ANALOG_PINS],
        digital_inputs: [EPin<Input<PullUp>>; DIGITAL_PINS],
    }

    #[local]
    struct Local {
        buffer: Option<&'static mut [u16; ANALOG_PINS]>,
        linear_inputs: [LinearInput; ANALOG_PINS],
        //ep_memory: &'static [u32; 1024],
        //usb_bus: &'static UsbBusAllocator<UsbBusType>
    }

    #[init(local= [ep_memory:[u32; EP_MEMORY_WORDS]= [0; EP_MEMORY_WORDS], usb_bus:Option<UsbBusAllocator<UsbBusType>> = None ])]
    fn init(cx: init::Context) -> (Shared, Local, init::Monotonics) {
        #[cfg(feature = "rtt")]
        rtt_init_print!();

        let mut dcb = cx.core.DCB;
        let dwt = cx.core.DWT;
        let systick = cx.core.SYST;

        cx.device.RCC.apb2enr.write(|w| w.syscfgen().enabled());
        let rcc = cx.device.RCC.constrain();
        let gpioa = cx.device.GPIOA.split();
        let gpiob = cx.device.GPIOB.split();
        let _gpioc = cx.device.GPIOC.split();

        // digital inputs
        let user_button = gpioa.pa0.into_pull_up_input().erase();
        let digital_inputs = [
            gpiob.pb0.into_pull_up_input().erase(),
            gpiob.pb1.into_pull_up_input().erase(),
            gpiob.pb2.into_pull_up_input().erase(),
            gpiob.pb3.into_pull_up_input().erase(),
            gpiob.pb4.into_pull_up_input().erase(),
            gpiob.pb5.into_pull_up_input().erase(),
            gpiob.pb6.into_pull_up_input().erase(),
            gpiob.pb7.into_pull_up_input().erase(),
            gpiob.pb8.into_pull_up_input().erase(),
            gpiob.pb9.into_pull_up_input().erase(),
        ];

        // analog inputs & dma
        let dma = StreamsTuple::new(cx.device.DMA2);

        let config = DmaConfig::default()
            .transfer_complete_interrupt(true)
            .memory_increment(true)
            .double_buffer(false);

        let pa1 = gpioa.pa1.into_analog();
        let pa2 = gpioa.pa2.into_analog();
        let pa3 = gpioa.pa3.into_analog();
        let pa4 = gpioa.pa4.into_analog();
        let pa5 = gpioa.pa5.into_analog();
        let pa6 = gpioa.pa6.into_analog();

        let adc_config = AdcConfig::default()
            .dma(Dma::Continuous)
            .scan(Scan::Enabled)
            .resolution(Resolution::Twelve)
            //.continuous(Continuous::Continuous)
            .default_sample_time(SampleTime::Cycles_480);

        let mut adc = Adc::adc1(cx.device.ADC1, true, adc_config);
        adc.configure_channel(&pa1, Sequence::One, SampleTime::Cycles_480);
        adc.configure_channel(&pa2, Sequence::Two, SampleTime::Cycles_480);
        adc.configure_channel(&pa3, Sequence::Three, SampleTime::Cycles_480);
        adc.configure_channel(&pa4, Sequence::Four, SampleTime::Cycles_480);
        adc.configure_channel(&pa5, Sequence::Five, SampleTime::Cycles_480);
        adc.configure_channel(&pa6, Sequence::Six, SampleTime::Cycles_480);

        let first_buffer = cortex_m::singleton!(: [u16;ANALOG_PINS] = [0;ANALOG_PINS]).unwrap();
        let second_buffer =
            Some(cortex_m::singleton!(: [u16;ANALOG_PINS] = [0;ANALOG_PINS]).unwrap());
        let transfer = Transfer::init_peripheral_to_memory(dma.0, adc, first_buffer, None, config);

        let _clocks = rcc
            .cfgr
            .use_hse(25.mhz())
            .sysclk(MONO_HZ.hz())
            .require_pll48clk()
            .freeze();

        //// USB initialization
        let usb = USB {
            hclk: 1000.hz(),
            usb_global: cx.device.OTG_FS_GLOBAL,
            usb_device: cx.device.OTG_FS_DEVICE,
            usb_pwrclk: cx.device.OTG_FS_PWRCLK,
            pin_dm: gpioa.pa11.into_alternate(),
            pin_dp: gpioa.pa12.into_alternate(),
        };

        *cx.local.usb_bus = Some(UsbBusType::new(usb, cx.local.ep_memory));

        // let usb_bus = USB_BUS.as_ref().unwrap();

        let usb_class = HIDClass::new(&cx.local.usb_bus.as_ref().unwrap());
        // https://github.com/obdev/v-usb/blob/master/usbdrv/USB-IDs-for-free.txt
        // For USB Joystick as there is no USB Game Pad on this free ID list
        let usb_device = UsbDeviceBuilder::new(
            &cx.local.usb_bus.as_ref().unwrap(),
            UsbVidPid(0x16c0, 0x27dc),
        )
        .manufacturer("autumnal.de")
        .product("RC USB Controller")
        .serial_number(env!("CARGO_PKG_VERSION"))
        .build();

        //Initialize Interrupt Input
        let syscfg = cx.device.SYSCFG;
        let exti = cx.device.EXTI;

        // enqueu
        read_analog::spawn().unwrap();
        //report::spawn().unwrap();
        polling::spawn().unwrap();

        let mono = DwtSystick::new(&mut dcb, dwt, systick, MONO_HZ);

        #[cfg(feature = "rtt")]
        rprintln!("init done");

        (
            Shared {
                transfer,
                usb_device,
                usb_class,
                analog_inputs: [1500; ANALOG_PINS],
                exti,
                user_button,
                digital_inputs,
            },
            Local {
                linear_inputs: Default::default(),
                buffer: second_buffer,
            },
            init::Monotonics(mono),
        )
    }

    #[task(shared = [transfer])]
    fn polling(mut cx: polling::Context) {
        cx.shared.transfer.lock(|transfer| {
            transfer.start(|adc| {
                adc.start_conversion();
            });
        });

        // reschedule self
        polling::spawn_after(1.millis()).ok();
    }

    // read analog
    #[task(shared = [ user_button, digital_inputs, analog_inputs], local = [ linear_inputs])]
    fn read_analog(cx: read_analog::Context) {
        let read_analog::Context { mut shared, local } = cx;

        let (axes, buttons) = (
            shared.user_button,
            shared.analog_inputs,
            shared.digital_inputs,
        )
            .lock(|user_button, analog_inputs, digital_inputs| {
                // analog
                let mut axes = [0u16; ANALOG_PINS];
                let commit_calibration = user_button.is_low();
                for (analog_reading, (linear_input, axis)) in analog_inputs
                    .iter()
                    .zip(local.linear_inputs.iter_mut().zip(axes.iter_mut()))
                    .take(2)
                {
                    if commit_calibration {
                        linear_input.set_center(*analog_reading);
                    }
                    *axis = linear_input.get(*analog_reading);
                }

                // digital
                let mut buttons = [false; DIGITAL_PINS];
                digital_inputs
                    .iter()
                    .zip(buttons.iter_mut())
                    .for_each(|(pin, out)| *out = pin.is_low());
                (axes, buttons)
            });

        // print the readings
        #[cfg(feature = "rtt")]
        {
            rprint!("axes: ");
            for axis in axes.iter().take(2) {
                rprint!("[{:4}] ", axis);
            }
            rprint!(", buttons: ");
            for button in buttons.iter() {
                rprint!("[{}] ", if *button { 'X' } else { ' ' });
            }
            rprintln!("");
        }

        // reschedule self
        read_analog::spawn_after(1.secs()).unwrap();
    }

    #[task(binds = DMA2_STREAM0, shared = [transfer, analog_inputs], local = [buffer])]
    fn dma(cx: dma::Context) {
        let dma::Context { mut shared, local } = cx;
        let (buffer, _) = shared.transfer.lock(|transfer| {
            transfer
                .next_transfer(local.buffer.take().unwrap())
                .unwrap()
        });

        shared.analog_inputs.lock(|a| *a = *buffer);

        *local.buffer = Some(buffer);
    }

    // Periodic status update to Computer (every millisecond)
    #[task(shared = [usb_class])]
    fn usb_report(mut cx: usb_report::Context) {
        // schedule itself to keep the loop running
        usb_report::spawn_after(1.millis()).unwrap();
        // TODO make schedule usb_report from DMA

        /*
        cx.resources.ppm_parser.lock(|parser: &mut PpmParser| {
            if let Some(frame) = parser.next_frame() {
                *last_frame = frame;
                #[cfg(feature = "rtt")]
                rprintln!("{:?}", frame);
            }
        });
        */

        let report = JoystickState::from_ppm_time();

        cx.shared
            .usb_class
            .lock(|class| class.write(unsafe { report.as_u8_slice() }));
    }

    // Global USB Interrupt (does not include Wakeup)
    #[task(binds = OTG_FS, shared = [usb_device, usb_class], priority = 2)]
    fn usb_tx(cx: usb_tx::Context) {
        (cx.shared.usb_device, cx.shared.usb_class)
            .lock(|usb_device, usb_class| usb_device.poll(&mut [usb_class]));
    }

    // Interrupt for USB Wakeup
    #[task(binds = OTG_FS_WKUP, shared = [usb_device, usb_class], priority = 2)]
    fn usb_rx(cx: usb_rx::Context) {
        (cx.shared.usb_device, cx.shared.usb_class)
            .lock(|usb_device, usb_class| usb_device.poll(&mut [usb_class]));
    }
}
