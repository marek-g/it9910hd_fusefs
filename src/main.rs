use clap::{value_t, App, Arg};
use fuse::ReplyEmpty;
use fuse::ReplyOpen;
use fuse::{
    FileAttr, FileType, Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, Request,
};
use libc::EIO;
use libc::ENOENT;
use std::collections::HashMap;
use std::env;
use std::ffi::OsStr;
use std::slice;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::{Duration, UNIX_EPOCH};

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
    size: 128 * 1024 * 1024 * 1024,
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
    buffer_max_len: u32,

    data_receiver: Option<Receiver<Vec<u8>>>,
    terminate_sender: Option<Sender<()>>,
    thread_ended_receiver: Option<Receiver<()>>,

    buffer: Vec<u8>,

    next_fh: u64,
    file_data: HashMap<u64, OpenedFileData>,
}

impl IT9910FS {
    pub fn new(
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
        buffer_max_len: u32,
    ) -> Result<Self, String> {
        Ok(IT9910FS {
            width: width,
            height: height,
            fps: fps,
            bitrate: bitrate,
            buffer_max_len: buffer_max_len,

            data_receiver: None,
            terminate_sender: None,
            thread_ended_receiver: None,

            buffer: Vec::new(),

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

            if self.file_data.len() == 0 {
                println!("Open device");

                let (data_sender, data_receiver) = channel();
                let (terminate_sender, terminate_receiver) = channel();
                let (thread_ended_sender, thread_ended_receiver) = channel();

                let width = self.width;
                let height = self.height;
                let fps = self.fps;
                let bitrate = self.bitrate;

                thread::spawn(move || {
                    // TODO: handle Err result!
                    if let Err(err) = run(
                        data_sender,
                        terminate_receiver,
                        thread_ended_sender,
                        width,
                        height,
                        fps,
                        bitrate) {
                        eprintln!("IT9910 thread error: {}", err);
                    }
                });

                self.data_receiver = Some(data_receiver);
                self.terminate_sender = Some(terminate_sender);
                self.thread_ended_receiver = Some(thread_ended_receiver);
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
            if let Some(ref mut file_data) = &mut self.file_data.get_mut(&fh) {
                //println!("Read: {}, {}, {}", fh, offset, size);

                if offset as usize != file_data.current_position {
                    // do not support seek operation
                    reply.error(ENOENT);
                    return;
                }

                let needed_size = offset as usize + size as usize;
                if needed_size > self.buffer.len() + 4 * 1024 * 1024 {
                    // do not read so far ahead
                    reply.error(ENOENT);
                    return;
                }

                while needed_size > self.buffer.len() {
                    if let Some(data_receiver) = &self.data_receiver {
                        let packet = data_receiver.recv().unwrap();
                        self.buffer.extend_from_slice(&packet);
                    }
                }

                reply.data(&self.buffer[offset as usize..needed_size]);

                file_data.current_position += size as usize;

            /*let mut vec = Vec::with_capacity(_size as usize);
            while vec.len() < _size as usize {
                let need_new_buffer = if let Some(last_buffer) = &self.last_buffer {
                    self.last_buffer_pos >= last_buffer.len()
                } else {
                    true
                };

                if need_new_buffer {
                    self.last_buffer_pos = 0;
                    if let Some(data_receiver) = &self.data_receiver {
                        self.last_buffer = Some(data_receiver.recv().unwrap());
                    }
                }

                if let Some(last_buffer) = &self.last_buffer {
                    let size = (last_buffer.len() - self.last_buffer_pos).min(_size as usize);
                    vec.extend_from_slice(
                        &last_buffer[self.last_buffer_pos..self.last_buffer_pos + size],
                    );
                    self.last_buffer_pos += size;
                }
            }*/

            //reply.data(&vec);
            } else {
                reply.error(ENOENT);
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
                terminate_sender.send(());
            }

            if let Some(thread_ended_receiver) = &self.thread_ended_receiver {
                thread_ended_receiver.recv();
            }

            self.buffer.clear();
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
) -> Result<(), String> {
    let mut it_driver = match IT9910Driver::open() {
        Ok(it_driver) => it_driver,
        Err(err) => {
            return Err(format!("Unable to find or open IT9910 device: {}", err));
        }
    };

    if let Err(err) = it_driver.start(width, height, fps, bitrate) {
        return Err(format!("Unable to start IT9910 device: {}", err));
    }

    loop {
        for _ in 0..16 {
            let mut vec = Vec::<u8>::with_capacity(16384);
            let mut buf =
                unsafe { slice::from_raw_parts_mut((&mut vec[..]).as_mut_ptr(), vec.capacity()) };

            let len = it_driver.read_data(&mut buf)?;
            unsafe { vec.set_len(len) };

            sender.send(vec);
        }

        match terminate_receiver.try_recv() {
            Ok(_) => break,
            Err(_) => (),
        }
    }

    if let Err(err) = it_driver.stop() {
        return Err(format!("Problem when stopping IT9910 device: {}", err));
    }

    thread_ended_sender.send(());

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
                .help("video bitrate, can be between 2000..20000")
                .short("b")
                .long("bitrate")
                .default_value("10000"),
        )
        .arg(
            Arg::with_name("buffer_len")
                .help("buffer size in MB")
                .short("l")
                .long("buffer_len")
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
    let bitrate = value_t!(matches, "bitrate", u32).unwrap_or(10000);
    let buffer_len = value_t!(matches, "buffer_len", u32).unwrap_or(100);
    let mountpoint = matches.value_of("dir").unwrap();

    let options = ["-o", "ro", "-o", "fsname=it9910fs"]
        .iter()
        .map(|o| o.as_ref())
        .collect::<Vec<&OsStr>>();

    let it9910fs = IT9910FS::new(width, height, fps, bitrate, buffer_len)?;

    fuse::mount(it9910fs, mountpoint, &options).unwrap();

    Ok(())
}
