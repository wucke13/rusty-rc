trait Radio: Into<CrsfPacket>{ 
    
    // Get the USB descriptor of this radio
    ///
    fn usb_descriptor(&self);


    /// generate usb package
    fn to_usb_package(&self, buf:&mut [u8]);

    const fn usb_package_length()->usize;

    /// Take raw input values
    fn raw_inputs(&mut self, analog_inputs:&[u16], digital_inputs: [bool] );
}

enum CrsfPacket {

}
