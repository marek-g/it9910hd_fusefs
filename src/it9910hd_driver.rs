use crate::usb_wrapper::UsbWrapper;
use std::slice;
use std::time::Duration;

pub struct IT9910Driver {
    usb_device: UsbWrapper,
    counter: u32,
}

impl IT9910Driver {
    pub fn open() -> Result<Self, String> {
        Ok(IT9910Driver {
            usb_device: UsbWrapper::new()?,
            counter: 0,
        })
    }

    pub fn start(
        &mut self,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
        audio_src: u32,
        video_src: u32,
        brightness: i32,
        contrast: i32,
        hue: i32,
        saturation: i32,
    ) -> Result<(), String> {
        //self.debug_query_time(1)?;
        //self.set_pc_grabber(0)?;

        //std::thread::sleep(std::time::Duration::from_millis(2000));

        //self.debug_query_time(1)?;
        //self.get_source()?;

        self.set_pc_grabber(1)?;
        loop {
            let res = self.get_pc_grabber()?;
            if res > 0 {
                break;
            }

            std::thread::sleep(std::time::Duration::from_millis(500));
        }

        //self.debug_query_time(1)?;

        let device_model = self.get_hw_grabber()?;
        if device_model == 2 {
            self.set_source(audio_src, video_src)?;
        }

        for i in 0..35 {
            self.set_pc_grabber2(device_model, i, width, height, bitrate, fps)?;
        }

        self.set_brightness(brightness)?;
        self.set_contrast(contrast)?;
        self.set_hue(hue)?;
        self.set_saturation(saturation)?;

        self.set_state(2)?;

        Ok(())
    }

    pub fn read_data(&mut self, buf: &mut [u8]) -> Result<usize, String> {
        //println!("ReadData()");

        let mut len = 0usize;
        //for _ in 0..16 {
        len += self.read_data_one_chunk(&mut buf[len..])?;
        //}

        Ok(len)
    }

    pub fn stop(&mut self) -> Result<(), String> {
        self.set_state(0)?;
        self.set_pc_grabber(0)?;
        loop {
            let res = self.get_pc_grabber()?;
            if res == 0 {
                break;
            }

            std::thread::sleep(std::time::Duration::from_millis(500));
        }

        Ok(())
    }

    #[allow(dead_code)]
    fn debug_query_time(&mut self, time: i32) -> Result<i32, String> {
        let mut buf = [0u8; 16 + 4];

        write_le_i32(&mut buf[16..20], time);

        let received = self.send_command(&mut buf, 0x9910F001, 1)?;

        let result_time = read_le_i32(&received[16..20]);

        //println!("DebugQueryTime({}) -> {}", time, result_time);

        Ok(result_time)
    }

    #[allow(dead_code)]
    fn get_source(&mut self) -> Result<(i32, i32), String> {
        let mut buf = [0u8; 16 + 4 * 2];

        let received = self.send_command(&mut buf, 0x99100003, 1)?;

        let audio_source = read_le_i32(&received[16..20]);
        let video_source = read_le_i32(&received[20..24]);

        //println!("GetSource() -> ({}, {})", audio_source, video_source);

        Ok((audio_source, video_source))
    }

    fn set_source(&mut self, audio_source: u32, video_source: u32) -> Result<(), String> {
        let mut buf = [0u8; 16 + 4 * 2];

        write_le_u32(&mut buf[16..20], audio_source);
        write_le_u32(&mut buf[20..24], video_source);

        let _received = self.send_command(&mut buf, 0x99100003, 2)?;
        //println!("SetSource() -> ({}, {})", audio_source, video_source);

        Ok(())
    }

    fn set_brightness(&mut self, brightness: i32) -> Result<(), String> {
        let mut buf = [0u8; 16 + 4 * 2];

        write_le_i32(&mut buf[16..20], 0);
        write_le_i32(&mut buf[20..24], brightness);

        let _received = self.send_command(&mut buf, 0x99100101, 2)?;

        Ok(())
    }

    fn set_contrast(&mut self, contrast: i32) -> Result<(), String> {
        let mut buf = [0u8; 16 + 4 * 2];

        write_le_i32(&mut buf[16..20], 0);
        write_le_i32(&mut buf[20..24], contrast);

        let _received = self.send_command(&mut buf, 0x99100102, 2)?;

        Ok(())
    }

    fn set_hue(&mut self, hue: i32) -> Result<(), String> {
        let mut buf = [0u8; 16 + 4 * 2];

        write_le_i32(&mut buf[16..20], 0);
        write_le_i32(&mut buf[20..24], hue);

        let _received = self.send_command(&mut buf, 0x99100103, 2)?;

        Ok(())
    }

    fn set_saturation(&mut self, set_saturation: i32) -> Result<(), String> {
        let mut buf = [0u8; 16 + 4 * 2];

        write_le_i32(&mut buf[16..20], 0);
        write_le_i32(&mut buf[20..24], set_saturation);

        let _received = self.send_command(&mut buf, 0x99100104, 2)?;

        Ok(())
    }

    fn set_pc_grabber(&mut self, start: i32) -> Result<(), String> {
        let mut buf = [0u8; 16 + 4 * 3];

        write_le_u32(&mut buf[16..20], 0x38384001);
        write_le_i32(&mut buf[24..28], start);

        let _received = self.send_command(&mut buf, 0x9910E001, 2)?;

        //println!("SetPCGrabber({})", start);

        Ok(())
    }

    fn set_pc_grabber2(
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

        //println!("SetPCGrabber2({})", counter);

        Ok(())
    }

    fn get_pc_grabber(&mut self) -> Result<i32, String> {
        let mut buf = [0u8; 16 + 4 * 3];

        write_le_u32(&mut buf[16..20], 0x38384001);

        let received = self.send_command(&mut buf, 0x9910E001, 1)?;

        let result = read_le_i32(&received[24..28]);

        //println!("GetPCGrabber() -> {}", result);

        Ok(result)
    }

    fn get_hw_grabber(&mut self) -> Result<i32, String> {
        let mut buf = [0u8; 16 + 4 * 35 + 2];

        write_le_u32(&mut buf[16..20], 8);

        let received = self.send_command(&mut buf, 0x9910F002, 1)?;

        //println!("GetHWGrabber()");

        let device_model = match received[31] {
            0x17 => 0,
            0x27 => 1,
            0x37 => 2,

            // MyGica HD Cap X-II returs 0 here and has multiple inputs (HDMI / YpbPr / CVBS)
            0x00 => 2,

            // unknown device - let's assume it can switch inputs
            _ => 2,
        };

        Ok(device_model)
    }

    fn set_state(&mut self, state: u32) -> Result<(), String> {
        let mut buf = [0u8; 16 + 4];

        write_le_u32(&mut buf[16..20], state);

        let _received = self.send_command(&mut buf, 0x99100002, 2)?;

        println!("SetState({})", state);

        Ok(())
    }

    fn send_command(
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

        let timeout = Duration::from_secs(5);
        match self
            .usb_device
            .handle
            .write_bulk(self.usb_device.write_addr, &send, timeout)
        {
            Ok(_) => {
                self.counter += 1;
                //println!(" - sent: {:02x?}", send);
            }
            Err(err) => {
                return Err(format!(
                    "Unable to write request to address: {}, error: {}",
                    self.usb_device.write_addr, err
                ))
            }
        }

        let mut vec = Vec::<u8>::with_capacity(512);
        let buf = unsafe { slice::from_raw_parts_mut((&mut vec[..]).as_mut_ptr(), vec.capacity()) };

        let timeout = Duration::from_secs(5);
        match self
            .usb_device
            .handle
            .read_bulk(self.usb_device.read_addr, buf, timeout)
        {
            Ok(len) => {
                unsafe { vec.set_len(len) };
                //println!(" - read: {:02x?}", vec);
            }
            Err(err) => {
                return Err(format!(
                    "Unable to read response from address: {}, error: {}",
                    self.usb_device.read_addr, err
                ))
            }
        }

        let result_code = read_le_i32(&vec[12..16]);
        if result_code < 0 {
            return Err(format!("Negative result code: {}", result_code));
        }

        Ok(vec)
    }

    fn read_data_one_chunk(&mut self, buf: &mut [u8]) -> Result<usize, String> {
        let timeout = Duration::from_secs(10);
        //println!("Read one chunk, buf size: {}", buf.len());
        match self
            .usb_device
            .handle
            .read_bulk(self.usb_device.data_addr, buf, timeout)
        {
            Ok(len) => {
                //println!("Read: {} bytes", len);
                Ok(len)
            }
            Err(err) => {
                return Err(format!(
                    "Unable to read response from address: {}, error: {}",
                    self.usb_device.data_addr, err
                ))
            }
        }
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

#[allow(dead_code)]
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
