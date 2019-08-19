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
    it_driver: Option<IT9910Driver>,

    next_fh: u64,
    file_data: HashMap<u64, OpenedFileData>,

    buffer: Vec<u8>,
    //buffer_start_offset: usize,
}

impl IT9910FS {
    pub fn new() -> Result<Self, String> {
        Ok(IT9910FS {
            it_driver: None,
            next_fh: 0,
            file_data: HashMap::new(),
            buffer: Vec::new(),
            //buffer_start_offset: 0,
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
            println!("Open");

            if self.file_data.len() == 0 {
                println!("Open device");

                /*let mut libusb_context = match libusb::Context::new() {
                    Ok(context) => context,
                    Err(e) => {
                        eprintln!("could not initialize libusb: {}", e);
                        reply.error(EIO);
                        return;
                    }
                };*/

                let mut it_driver = match IT9910Driver::open() {
                    Ok(it_driver) => it_driver,
                    Err(err) => {
                        eprintln!("Unable to find or open IT9910 device: {}", err);
                        reply.error(EIO);
                        return;
                    }
                };

                if let Err(err) = it_driver.start() {
                    eprintln!("Unable to start IT9910 device: {}", err);
                    reply.error(EIO);
                    return;
                }

                self.it_driver = Some(it_driver);
            }

            self.file_data.insert(
                self.next_fh,
                OpenedFileData {
                    current_position: 0,
                },
            );

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
                println!("Read: {}, {}, {}", fh, offset, size);

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

                if let Some(ref mut it_driver) = &mut self.it_driver {
                    while needed_size > self.buffer.len() {
                        let mut vec = vec![0u8; 16 * 16384];
                        match it_driver.read_data(&mut vec[..]) {
                            Ok(n) => {
                                self.buffer.extend_from_slice(&vec[0..n]);
                            }
                            Err(err) => {
                                eprintln!("Error reading data from IT9910 device: {}", err);
                                reply.error(EIO);
                                return;
                            }
                        }
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

        if self.file_data.len() == 0 {
            println!("Close device");

            if let Some(ref mut it_driver) = &mut self.it_driver {
                if let Err(err) = it_driver.stop() {
                    eprintln!("Problem when stopping IT9910 device: {}", err);
                }
            }

            self.it_driver = None;
        }

        reply.ok();
    }
}

fn main() -> Result<(), String> {
    //env_logger::init();
    let mountpoint = env::args_os().nth(1).unwrap();
    let options = ["-o", "ro", "-o", "fsname=it9910fs"]
        .iter()
        .map(|o| o.as_ref())
        .collect::<Vec<&OsStr>>();

    let it9910fs = IT9910FS::new()?;

    fuse::mount(it9910fs, mountpoint, &options).unwrap();

    Ok(())
}
