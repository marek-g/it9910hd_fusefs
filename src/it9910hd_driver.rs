use std::io::Write;
use std::slice;
use std::time::Duration;

#[derive(Debug)]
struct Endpoint {
    config: u8,
    iface: u8,
    setting: u8,
    address: u8,
}

pub fn run() -> Result<(), String> {
    let vid: u16 = 0x048D;
    let pid: u16 = 0x9910;

    let libusb_context = match libusb::Context::new() {
        Ok(context) => context,
        Err(e) => return Err(format!("could not initialize libusb: {}", e)),
    };

    let devices = match libusb_context.devices() {
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

    let mut handle = match device.open() {
        Ok(handle) => handle,
        Err(e) => return Err(format!("could not open device: {}", e)),
    };

    // get endpoints
    let endpoint_command_read: Endpoint;
    let endpoint_command_write: Endpoint;
    let endpoint_data_read: Endpoint;

    let config_desc = device.config_descriptor(0).unwrap();
    let interface = config_desc.interfaces().next().unwrap();
    let interface_desc = interface.descriptors().next().unwrap();
    let mut interface_descriptors = interface_desc.endpoint_descriptors();
    endpoint_command_read = {
        let endpoint_desc = interface_descriptors.next().unwrap();
        Endpoint {
            config: config_desc.number(),
            iface: interface_desc.interface_number(),
            setting: interface_desc.setting_number(),
            address: endpoint_desc.address(),
        }
    };
    endpoint_command_write = {
        let endpoint_desc = interface_descriptors.next().unwrap();
        Endpoint {
            config: config_desc.number(),
            iface: interface_desc.interface_number(),
            setting: interface_desc.setting_number(),
            address: endpoint_desc.address(),
        }
    };
    endpoint_data_read = {
        let endpoint_desc = interface_descriptors.next().unwrap();
        Endpoint {
            config: config_desc.number(),
            iface: interface_desc.interface_number(),
            setting: interface_desc.setting_number(),
            address: endpoint_desc.address(),
        }
    };

    let has_kernel_driver = match handle.kernel_driver_active(interface_desc.interface_number()) {
        Ok(true) => {
            handle
                .detach_kernel_driver(interface_desc.interface_number())
                .ok();
            true
        }
        _ => false,
    };

    handle
        .set_active_configuration(config_desc.number())
        .unwrap();
    handle
        .claim_interface(interface_desc.interface_number())
        .unwrap();
    handle
        .set_alternate_setting(
            interface_desc.interface_number(),
            interface_desc.setting_number(),
        )
        .unwrap();

    stream_video(
        &mut handle,
        &endpoint_command_read,
        &endpoint_command_write,
        &endpoint_data_read,
    )?;

    if has_kernel_driver {
        handle
            .attach_kernel_driver(interface_desc.interface_number())
            .ok();
    }

    Ok(())
}

fn stream_video(
    handle: &mut libusb::DeviceHandle,
    endpoint_command_read: &Endpoint,
    endpoint_command_write: &Endpoint,
    endpoint_data_read: &Endpoint,
) -> Result<(), String> {
    let mut transfer_handle = TransferHandle {
        device_handle: &handle,
        counter: 0,
        read_addr: endpoint_command_read.address,
        write_addr: endpoint_command_write.address,
        data_addr: endpoint_data_read.address,
    };

    //transfer_handle.debug_query_time(1)?;
    //transfer_handle.set_pc_grabber(0)?;

    //std::thread::sleep(std::time::Duration::from_millis(2000));

    //transfer_handle.debug_query_time(1)?;
    //transfer_handle.get_source()?;

    transfer_handle.set_pc_grabber(1)?;
    loop {
        let res = transfer_handle.get_pc_grabber()?;
        if res > 0 {
            break;
        }

        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    //transfer_handle.debug_query_time(1)?;

    let device_model = transfer_handle.get_hw_grabber()?;
    if device_model == 2 {
        transfer_handle.set_source(2, 4)?;
    }

    for i in 0..35 {
        transfer_handle.set_pc_grabber2(device_model, i, 1920, 1080, 10000, 25)?;
    }

    //transfer_handle.debug_query_time(1)?;

    transfer_handle.set_state(2)?;

    let mut file = std::fs::File::create("output.ts").unwrap();

    for _j in 0..10 {
        // read data always in 16 chunks before issuing other command
        for _i in 0..16 {
            let data = transfer_handle.read_data()?;
            file.write(&data).unwrap();
        }
    }

    transfer_handle.set_state(0)?;
    transfer_handle.set_pc_grabber(0)?;

    Ok(())
}

struct TransferHandle<'a> {
    device_handle: &'a libusb::DeviceHandle<'a>,
    counter: u32,
    read_addr: u8,
    write_addr: u8,
    data_addr: u8,
}

impl<'a> TransferHandle<'a> {
    pub fn debug_query_time(&mut self, time: i32) -> Result<i32, String> {
        let mut buf = [0u8; 16 + 4];

        write_le_i32(&mut buf[16..20], time);

        let received = self.send_command(&mut buf, 0x9910F001, 1)?;

        let result_time = read_le_i32(&received[16..20]);

        println!("DebugQueryTime({}) -> {}", time, result_time);

        Ok(result_time)
    }

    pub fn get_source(&mut self) -> Result<(i32, i32), String> {
        let mut buf = [0u8; 16 + 4 * 2];

        let received = self.send_command(&mut buf, 0x99100003, 1)?;

        let audio_source = read_le_i32(&received[16..20]);
        let video_source = read_le_i32(&received[20..24]);

        println!("GetSource() -> ({}, {})", audio_source, video_source);

        Ok((audio_source, video_source))
    }

    pub fn set_source(&mut self, audio_source: i32, video_source: i32) -> Result<(), String> {
        let mut buf = [0u8; 16 + 4 * 2];

        write_le_i32(&mut buf[16..20], audio_source);
        write_le_i32(&mut buf[20..24], video_source);

        let _received = self.send_command(&mut buf, 0x99100003, 2)?;
        println!("SetSource() -> ({}, {})", audio_source, video_source);

        Ok(())
    }

    pub fn set_pc_grabber(&mut self, start: i32) -> Result<(), String> {
        let mut buf = [0u8; 16 + 4 * 3];

        write_le_u32(&mut buf[16..20], 0x38384001);
        write_le_i32(&mut buf[24..28], start);

        let _received = self.send_command(&mut buf, 0x9910E001, 2)?;

        println!("SetPCGrabber({})", start);

        Ok(())
    }

    pub fn set_pc_grabber2(
        &mut self,
        device_model: i32,
        counter: u32,
        width: u32,
        height: u32,
        kbitrate: u32,
        framerate: u32,
    ) -> Result<(), String> {
        let mut buf = [0u8; 16 + 4 * 15];

        write_le_u32(&mut buf[16..20], 0x38382008);
        write_le_u32(&mut buf[24..28], if device_model == 2 { 4 } else { 5 });
        write_le_u32(&mut buf[28..32], counter);
        write_le_u32(&mut buf[32..36], 15);
        write_le_u32(&mut buf[36..40], width);
        write_le_u32(&mut buf[40..44], height);
        write_le_u32(&mut buf[44..48], kbitrate);
        write_le_u32(&mut buf[56..60], framerate);

        let _received = self.send_command(&mut buf, 0x9910E001, 2)?;

        println!("SetPCGrabber2({})", counter);

        Ok(())
    }

    pub fn get_pc_grabber(&mut self) -> Result<i32, String> {
        let mut buf = [0u8; 16 + 4 * 3];

        write_le_u32(&mut buf[16..20], 0x38384001);

        let received = self.send_command(&mut buf, 0x9910E001, 1)?;

        let result = read_le_i32(&received[24..28]);

        println!("GetPCGrabber() -> {}", result);

        Ok(result)
    }

    pub fn get_hw_grabber(&mut self) -> Result<(i32), String> {
        let mut buf = [0u8; 16 + 4 * 35 + 2];

        write_le_u32(&mut buf[16..20], 8);

        let received = self.send_command(&mut buf, 0x9910F002, 1)?;

        println!("GetHWGrabber()");

        let device_model = match received[31] {
            0x17 => 0,
            0x27 => 1,
            0x37 => 2,
            _ => -1,
        };

        Ok(device_model)
    }

    pub fn set_state(&mut self, state: u32) -> Result<(), String> {
        let mut buf = [0u8; 16 + 4];

        write_le_u32(&mut buf[16..20], state);

        let _received = self.send_command(&mut buf, 0x99100002, 2)?;

        println!("SetState({})", state);

        Ok(())
    }

    pub fn send_command(
        &mut self,
        send: &mut [u8],
        command_id: u32,
        subcommand_id: u32,
    ) -> Result<Vec<u8>, String> {
        let len = send.len() as u32;
        write_le_u32(&mut send[0..4], len);
        write_le_u32(&mut send[4..8], command_id);
        write_le_u32(&mut send[8..12], subcommand_id);
        write_le_u32(&mut send[12..16], 0x99100000 | self.counter);

        let timeout = Duration::from_secs(1);
        match self
            .device_handle
            .write_bulk(self.write_addr, &send, timeout)
        {
            Ok(_) => {
                self.counter += 1;
                println!(" - sent: {:02x?}", send);
            }
            Err(err) => {
                return Err(format!(
                    "Unable to write request to address: {}, error: {}",
                    self.write_addr, err
                ))
            }
        }

        let mut vec = Vec::<u8>::with_capacity(512);
        let buf = unsafe { slice::from_raw_parts_mut((&mut vec[..]).as_mut_ptr(), vec.capacity()) };

        let timeout = Duration::from_secs(1);
        match self.device_handle.read_bulk(self.read_addr, buf, timeout) {
            Ok(len) => {
                unsafe { vec.set_len(len) };
                println!(" - read: {:02x?}", vec);
            }
            Err(err) => {
                return Err(format!(
                    "Unable to read response from address: {}, error: {}",
                    self.read_addr, err
                ))
            }
        }

        let result_code = read_le_i32(&vec[12..16]);
        if result_code < 0 {
            return Err(format!("Negative result code: {}", result_code));
        }

        Ok(vec)
    }

    pub fn read_data(&mut self) -> Result<Vec<u8>, String> {
        let mut vec = Vec::<u8>::with_capacity(16384);
        let buf = unsafe { slice::from_raw_parts_mut((&mut vec[..]).as_mut_ptr(), vec.capacity()) };

        let timeout = Duration::from_secs(15);
        match self.device_handle.read_bulk(self.data_addr, buf, timeout) {
            Ok(len) => {
                unsafe { vec.set_len(len) };
                println!(" - read: {} bytes", vec.len());
            }
            Err(err) => {
                return Err(format!(
                    "Unable to read response from address: {}, error: {}",
                    self.data_addr, err
                ))
            }
        }

        println!("ReadData()");

        Ok(vec)
    }
}

fn read_le_i32(input: &[u8]) -> i32 {
    i32::from_le_bytes([input[0], input[1], input[2], input[3]])
}

fn write_le_i32(dest: &mut [u8], val: i32) {
    let res = val.to_le_bytes();
    dest[0] = res[0];
    dest[1] = res[1];
    dest[2] = res[2];
    dest[3] = res[3];
}

fn read_le_u32(input: &[u8]) -> u32 {
    u32::from_le_bytes([input[0], input[1], input[2], input[3]])
}

fn write_le_u32(dest: &mut [u8], val: u32) {
    let res = val.to_le_bytes();
    dest[0] = res[0];
    dest[1] = res[1];
    dest[2] = res[2];
    dest[3] = res[3];
}
