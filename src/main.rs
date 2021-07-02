use clap::{value_t, App, Arg};
use fuse::ReplyEmpty;
use fuse::ReplyOpen;
use fuse::{
    FileAttr, FileType, Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, Request,
};
use libc::EIO;
use libc::ENOENT;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::slice;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::{Duration, UNIX_EPOCH};
use thread_priority::*;

mod it9910hd_driver;
use it9910hd_driver::*;

mod usb_wrapper;

const TTL: Duration = Duration::from_secs(1); // 1 second

const DIR_ATTR: FileAttr = FileAttr {
    ino: 1,
    size: 0,
    blocks: 0,
    atime: UNIX_EPOCH, // 1970-01-01 00:00:00
    mtime: UNIX_EPOCH,
    ctime: UNIX_EPOCH,
    crtime: UNIX_EPOCH,
    kind: FileType::Directory,
    perm: 0o755,
    nlink: 2,
    uid: 501,
    gid: 20,
    rdev: 0,
    flags: 0,
};

const HDMI_STREAM_TS_ATTR: FileAttr = FileAttr {
    ino: 2,
    size: 512 * 1024 * 1024 * 1024,
    blocks: 0,
    atime: UNIX_EPOCH, // 1970-01-01 00:00:00
    mtime: UNIX_EPOCH,
    ctime: UNIX_EPOCH,
    crtime: UNIX_EPOCH,
    kind: FileType::RegularFile,
    perm: 0o644,
    nlink: 1,
    uid: 501,
    gid: 20,
    rdev: 0,
    flags: 0,
};

struct OpenedFileData {
    pub current_position: usize,
}

struct IT9910FS {
    width: u32,
    height: u32,
    fps: u32,
    bitrate: u32,
    audio_src: u32,
    video_src: u32,
    brightness: i32,
    constrast: i32,
    hue: i32,
    saturation: i32,

    data_receiver: Option<Receiver<Vec<u8>>>,
    terminate_sender: Option<Sender<()>>,
    thread_ended_receiver: Option<Receiver<()>>,

    read_buffer: Vec<u8>,
    last_packet: Vec<u8>,
    last_packet_pos: usize,

    next_fh: u64,
    file_data: HashMap<u64, OpenedFileData>,
}

impl IT9910FS {
    pub fn new(
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
        audio_src: u32,
        video_src: u32,
        brightness: i32,
        constrast: i32,
        hue: i32,
        saturation: i32,
    ) -> Result<Self, String> {
        Ok(IT9910FS {
            width: width,
            height: height,
            fps: fps,
            bitrate: bitrate,
            audio_src: audio_src,
            video_src: video_src,
            brightness: brightness,
            constrast: constrast,
            hue: hue,
            saturation: saturation,

            data_receiver: None,
            terminate_sender: None,
            thread_ended_receiver: None,

            read_buffer: vec![0u8; 1024 * 1024],
            last_packet: Vec::new(),
            last_packet_pos: 0,

            next_fh: 0,
            file_data: HashMap::new(),
        })
    }
}

impl Filesystem for IT9910FS {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        if parent == 1 && name.to_str() == Some("hdmi_stream.ts") {
            reply.entry(&TTL, &HDMI_STREAM_TS_ATTR, 0);
        } else {
            reply.error(ENOENT);
        }
    }

    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        match ino {
            1 => reply.attr(&TTL, &DIR_ATTR),
            2 => reply.attr(&TTL, &HDMI_STREAM_TS_ATTR),
            _ => reply.error(ENOENT),
        }
    }

    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        if ino != 1 {
            reply.error(ENOENT);
            return;
        }

        let entries = vec![
            (1, FileType::Directory, "."),
            (1, FileType::Directory, ".."),
            (2, FileType::RegularFile, "hdmi_stream.ts"),
        ];

        for (i, entry) in entries.into_iter().enumerate().skip(offset as usize) {
            // i + 1 means the index of the next entry
            reply.add(entry.0, (i + 1) as i64, entry.1, entry.2);
        }
        reply.ok();
    }

    fn open(&mut self, _req: &Request, ino: u64, _flags: u32, reply: ReplyOpen) {
        if ino == 2 {
            println!("Open {}", self.next_fh);

            if self.file_data.len() == 1 {
                // currently do not support more than one client
                eprintln!("Sorry! More than one client is currently not supported!");
                reply.error(EIO);
                return;
            }

            if self.file_data.len() == 0 {
                println!("Open device");

                let (data_sender, data_receiver) = channel();
                let (terminate_sender, terminate_receiver) = channel();
                let (thread_ended_sender, thread_ended_receiver) = channel();

                let width = self.width;
                let height = self.height;
                let fps = self.fps;
                let bitrate = self.bitrate;
                let audio_src = self.audio_src;
                let video_src = self.video_src;
                let brightness = self.brightness;
                let contrast = self.constrast;
                let hue = self.hue;
                let saturation = self.saturation;

                thread::spawn(move || {
                    let thread_id = thread_native_id();
                    if let Err(err) = set_thread_priority(
                        thread_id,
                        ThreadPriority::Max,
                        ThreadSchedulePolicy::Normal(NormalThreadSchedulePolicy::Normal),
                    ) {
                        eprintln!("Warning! Cannot set thread priority: {:?}", err);
                    }

                    // TODO: handle Err result!
                    if let Err(err) = run(
                        data_sender,
                        terminate_receiver,
                        thread_ended_sender,
                        width,
                        height,
                        fps,
                        bitrate,
                        audio_src,
                        video_src,
                        brightness,
                        contrast,
                        hue,
                        saturation,
                    ) {
                        eprintln!("IT9910 thread error: {}", err);
                    }
                });

                self.data_receiver = Some(data_receiver);
                self.terminate_sender = Some(terminate_sender);
                self.thread_ended_receiver = Some(thread_ended_receiver);
                self.last_packet = Vec::new();
                self.last_packet_pos = 0;
            }

            self.file_data.insert(
                self.next_fh,
                OpenedFileData {
                    current_position: 0,
                },
            );

            println!("No opened files: {}", self.file_data.len());

            reply.opened(self.next_fh, 0);
            self.next_fh += 1;
        }
    }

    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        fh: u64,
        offset: i64,
        size: u32,
        reply: ReplyData,
    ) {
        if ino == 2 {
            let mut failed = false;

            if let Some(ref mut file_data) = &mut self.file_data.get_mut(&fh) {
                //println!("Read: {}, {}, {}", fh, offset, size);

                if offset as usize != file_data.current_position {
                    // do not support seek operation
                    reply.error(ENOENT);
                    return;
                }

                if size as usize > self.read_buffer.len() {
                    // do not read more than read buffer size
                    reply.error(ENOENT);
                    return;
                }

                let mut to_read = size as usize;
                let mut position = 0usize;
                while to_read > 0 {
                    let last_packet_size = self.last_packet.len() - self.last_packet_pos;
                    if last_packet_size > 0 {
                        let to_copy = to_read.min(last_packet_size);
                        (&mut self.read_buffer[position..position + to_copy]).copy_from_slice(
                            &self.last_packet[self.last_packet_pos..self.last_packet_pos + to_copy],
                        );
                        to_read -= to_copy;
                        position += to_copy;
                        self.last_packet_pos += to_copy;
                    } else {
                        if let Some(data_receiver) = &mut self.data_receiver {
                            self.last_packet = match data_receiver.recv() {
                                Ok(packet) => packet,
                                Err(err) => {
                                    eprintln!("Error during data receiving: {}", err);
                                    failed = true;
                                    break;
                                }
                            };
                            self.last_packet_pos = 0;
                        }
                    }
                }

                if failed {
                    reply.error(EIO);
                } else {
                    reply.data(&self.read_buffer[0..size as usize]);
                }

                file_data.current_position += size as usize;
            } else {
                reply.error(ENOENT);
            }

            if failed {
                self.file_data.remove(&fh);
            }
        } else {
            reply.error(ENOENT);
        }
    }

    fn release(
        &mut self,
        _req: &Request,
        _ino: u64,
        fh: u64,
        _flags: u32,
        _lock_owner: u64,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        println!("Close: {}", fh);

        self.file_data.remove(&fh);

        println!("No opened files: {}", self.file_data.len());

        if self.file_data.len() == 0 {
            println!("Close device");

            if let Some(terminate_sender) = &self.terminate_sender {
                terminate_sender.send(()).unwrap();
            }

            if let Some(thread_ended_receiver) = &self.thread_ended_receiver {
                thread_ended_receiver.recv().unwrap();
            }
        }

        reply.ok();
    }
}

pub fn run(
    sender: Sender<Vec<u8>>,
    terminate_receiver: Receiver<()>,
    thread_ended_sender: Sender<()>,
    width: u32,
    height: u32,
    fps: u32,
    bitrate: u32,
    audio_src: u32,
    video_src: u32,
    brightness: i32,
    constrast: i32,
    hue: i32,
    saturation: i32,
) -> Result<(), String> {
    let mut it_driver = match IT9910Driver::open() {
        Ok(it_driver) => it_driver,
        Err(err) => {
            return Err(format!("Unable to find or open IT9910 device: {}", err));
        }
    };

    if let Err(err) = it_driver.start(
        width, height, fps, bitrate, audio_src, video_src, brightness, constrast, hue, saturation,
    ) {
        return Err(format!("Unable to start IT9910 device: {}", err));
    }

    loop {
        loop {
            let mut vec = Vec::<u8>::with_capacity(16384);
            let mut buf =
                unsafe { slice::from_raw_parts_mut((&mut vec[..]).as_mut_ptr(), vec.capacity()) };

            let len = it_driver.read_data(&mut buf)?;
            unsafe { vec.set_len(len) };

            sender.send(vec).unwrap();

            if len < 16384 {
                break;
            }
        }

        match terminate_receiver.try_recv() {
            Ok(_) => break,
            Err(_) => (),
        }
    }

    if let Err(err) = it_driver.stop() {
        return Err(format!("Problem when stopping IT9910 device: {}", err));
    }

    thread_ended_sender.send(()).unwrap();

    Ok(())
}

fn main() -> Result<(), String> {
    let matches = App::new("IT9910HD FuseFS")
        .version("1.0")
        .author("Marek Gibek")
        .arg(
            Arg::with_name("width")
                .help("video width, can be: 1920, 1280, 720")
                .short("w")
                .long("width")
                .takes_value(true)
                .default_value("1920"),
        )
        .arg(
            Arg::with_name("height")
                .help("video height, can be: 1080, 720, 576, 480")
                .short("h")
                .long("height")
                .default_value("1080"),
        )
        .arg(
            Arg::with_name("fps")
                .help("video framerate, for example 25, 30 etc.")
                .short("f")
                .long("fps")
                .default_value("25"),
        )
        .arg(
            Arg::with_name("bitrate")
                .help("video bitrate, can be between 2000..52000")
                .short("b")
                .long("bitrate")
                .default_value("20000"),
        )
        .arg(
            Arg::with_name("audio_src")
                .help("audio source")
                .short("a")
                .long("audio_src")
                .default_value("2"),
        )
        .arg(
            Arg::with_name("video_src")
                .help("video source")
                .short("v")
                .long("video_src")
                .default_value("4"),
        )
        .arg(
            Arg::with_name("brightness")
                .help("brightness, range: -100..100")
                .long("brightness")
                .default_value("0"),
        )
        .arg(
            Arg::with_name("contrast")
                .help("contrast, range: 0..1000")
                .long("contrast")
                .default_value("100"),
        )
        .arg(
            Arg::with_name("hue")
                .help("hue, range: 0..360")
                .long("hue")
                .default_value("0"),
        )
        .arg(
            Arg::with_name("saturation")
                .help("saturation, range: 0..1000")
                .long("saturation")
                .default_value("100"),
        )
        .arg(
            Arg::with_name("dir")
                .help("mountpoint for video filesystem")
                .index(1)
                .required(true),
        )
        .get_matches();

    let width = value_t!(matches, "width", u32).unwrap_or(1920);
    let height = value_t!(matches, "height", u32).unwrap_or(1080);
    let fps = value_t!(matches, "fps", u32).unwrap_or(25);
    let bitrate = value_t!(matches, "bitrate", u32).unwrap_or(20000);
    let audio_src = value_t!(matches, "audio_src", u32).unwrap_or(2);
    let video_src = value_t!(matches, "video_src", u32).unwrap_or(4);
    let brightness = value_t!(matches, "brightness", i32).unwrap_or(0);
    let contrast = value_t!(matches, "contrast", i32).unwrap_or(0);
    let hue = value_t!(matches, "hue", i32).unwrap_or(0);
    let saturation = value_t!(matches, "saturation", i32).unwrap_or(0);
    let mountpoint = matches.value_of("dir").unwrap();

    println!("IT9910HD FuseFS.");
    println!(
        "Resolution: {}x{}, {} fps, {} kbps.",
        width, height, fps, bitrate
    );
    println!("Audio Src: {}, video src: {}", audio_src, video_src);
    println!("--------");

    let options = ["-o", "ro", "-o", "fsname=it9910fs"]
        .iter()
        .map(|o| o.as_ref())
        .collect::<Vec<&OsStr>>();

    let it9910fs = IT9910FS::new(
        width, height, fps, bitrate, audio_src, video_src, brightness, contrast, hue, saturation,
    )?;

    fuse::mount(it9910fs, mountpoint, &options).unwrap();

    Ok(())
}
