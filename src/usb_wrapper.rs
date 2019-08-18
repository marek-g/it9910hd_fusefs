// This file is a modified version of the
// https://github.com/oberien/logitech-g910-rs/blob/master/src/utils.rs
// which is licensed under either of
//
//     Apache License, Version 2.0, (LICENSE-APACHE or http://www.apache.org/licenses/LICENSE-2.0)
//     MIT license (LICENSE-MIT or http://opensource.org/licenses/MIT)
//
// at your option.
//
// I needed this techinque because the design of libusb (at the time of writing this project)
// didn't allow me to return and use device handle outside one method without this wrapper.
// This is because of very strange usage of references and liftimes by libusb!
//
// I removed AsyncGroup support from original file. If you are interested in it you can look there.

use libusb::{
    Context,
    DeviceHandle,
};

pub struct UsbWrapper {
    pub handle: &'static DeviceHandle<'static>,
    pub read_addr: u8,
    pub write_addr: u8,
    pub data_addr: u8,

    context: &'static Context,
    has_kernel_driver: bool,
    interface_number: u8,
}

impl UsbWrapper {
    pub fn new() -> Result<UsbWrapper, String> {
        // We must leak both context and handle, as rust does not allow sibling structs.
        // Leaking them gives us a &'static reference, which we can then use without
        // lifetime bounds, as it outlives everything.
        // We must make sure though, that the leaked memory is freed afterwards,
        // which is done in Drop.
        let context = Box::new(get_context()?);
        let context_ptr = Box::into_raw(context);
        let context_ref = unsafe { &*context_ptr as &'static Context };
        let (handle, driver, interface_number, read_addr, write_addr, data_addr) = get_handle(context_ref)?;
        let handle_ptr = Box::into_raw(Box::new(handle));
        unsafe {
            Ok(UsbWrapper {
                handle: &mut *handle_ptr as &'static mut DeviceHandle<'static>,
                read_addr: read_addr,
                write_addr: write_addr,
                data_addr: data_addr,

                context: context_ref,
                has_kernel_driver: driver,
                interface_number: interface_number,
            })
        }
    }
}

macro_rules! unwrap_safe {
    ($e:expr) => {
        match $e {
            Ok(_) => {},
            Err(e) => println!("Error while dropping UsbWrapper during another panic: {:?}", e),
        }
    }
}

impl Drop for UsbWrapper {
    fn drop(&mut self) {
        // make sure handle_mut is dropped before dropping it's refering content
        // this assures that there will be no dangling pointers
        {
            let handle_mut = unsafe { &mut *(self.handle as *const _ as *mut DeviceHandle<'static>) };
            unwrap_safe!(handle_mut.release_interface(self.interface_number));
            if self.has_kernel_driver {
                unwrap_safe!(handle_mut.attach_kernel_driver(self.interface_number));
            }
        }
        // drop the DeviceHandle to release Context
        let handle_ptr = &*self.handle as *const _ as *mut DeviceHandle<'static>;
        drop(unsafe { Box::from_raw(handle_ptr) });
        let context_ptr = self.context as *const _ as *mut Context;
        drop(unsafe { Box::from_raw(context_ptr) });
    }
}

fn get_context() -> Result<Context, String> {
    match libusb::Context::new() {
        Ok(context) => Ok(context),
        Err(e) => return Err(format!("could not initialize libusb: {}", e)),
    }
}

fn get_handle<'a>(context: &'a Context) -> Result<(DeviceHandle<'a>, bool, u8, u8, u8, u8), String> {
    let vid: u16 = 0x048D;
    let pid: u16 = 0x9910;

    let devices = match context.devices() {
        Ok(d) => d,
        Err(e) => return Err(format!("could not get list of devices: {}", e)),
    };

    let (device, _device_desc) = match devices
        .iter()
        .map(|d| {
            let dd = d.device_descriptor();
            (d, dd)
        })
        .find(|(_, dd)| {
            if let Ok(dd) = dd {
                dd.vendor_id() == vid && dd.product_id() == pid
            } else {
                false
            }
        }) {
        Some((device, Ok(device_desc))) => (device, device_desc),
        _ => {
            return Err(format!(
                "could not open device: VID: {:04x} PID: {:04x}",
                vid, pid
            ))
        }
    };

    // get endpoints
    let config_desc = device.config_descriptor(0).unwrap();
    let config_desc_number = config_desc.number();
    let interface = config_desc.interfaces().next().unwrap();
    let interface_desc = interface.descriptors().next().unwrap();
    let interface_number = interface_desc.interface_number();
    let interface_desc_setting_number = interface_desc.setting_number();

    let mut interface_descriptors = interface_desc.endpoint_descriptors();
    let read_addr = interface_descriptors.next().unwrap().address();
    let write_addr = interface_descriptors.next().unwrap().address();
    let data_addr = interface_descriptors.next().unwrap().address();

    let mut handle = match device.open() {
        Ok(handle) => handle,
        Err(e) => return Err(format!("could not open device: {}", e)),
    };

    let has_kernel_driver = match handle.kernel_driver_active(interface_number) {
        Ok(true) => {
            handle.detach_kernel_driver(interface_number).ok();
            true
        }
        _ => false,
    };

    handle.set_active_configuration(config_desc_number).unwrap();
    handle.claim_interface(interface_number).unwrap();
    handle
        .set_alternate_setting(interface_number, interface_desc_setting_number)
        .unwrap();

    Ok((handle, has_kernel_driver, interface_number, read_addr, write_addr, data_addr))
}
