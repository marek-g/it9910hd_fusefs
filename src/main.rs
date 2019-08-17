use fuse::ReplyEmpty;
use fuse::ReplyOpen;
use fuse::{
    FileAttr, FileType, Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, Request,
};
use libc::ENOENT;
use std::env;
use std::ffi::OsStr;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::{Duration, UNIX_EPOCH};

mod it9910hd_driver;
use it9910hd_driver::*;

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

struct IT9910HD_FS {
    next_fh: u64,
    data_receiver: Option<Receiver<Vec<u8>>>,
    terminate_sender: Option<Sender<()>>,

    buffer: Vec<u8>,
    current_position: usize,
    //buffer_start_offset: usize,
}

impl IT9910HD_FS {
    pub fn new() -> Self {
        IT9910HD_FS {
            next_fh: 0,
            data_receiver: None,
            terminate_sender: None,
            buffer: Vec::new(),
            current_position: 0,
            //buffer_start_offset: 0,
        }
    }
}

impl Filesystem for IT9910HD_FS {
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

    fn open(&mut self, _req: &Request, _ino: u64, _flags: u32, reply: ReplyOpen) {
        println!("Open");

        let (data_sender, data_receiver) = channel();
        let (terminate_sender, terminate_receiver) = channel();

        thread::spawn(move || {
            run(data_sender, terminate_receiver);
        });

        self.data_receiver = Some(data_receiver);
        self.terminate_sender = Some(terminate_sender);
        self.current_position = 0;

        reply.opened(0, 0);
    }

    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        reply: ReplyData,
    ) {
        if ino == 2 {
            println!("Read: {}, {}, {}", _fh, offset, size);

            if offset as usize != self.current_position {
                // do not support seek operation
                reply.error(ENOENT);
                return;
            }

            let needed_size = offset as usize + size as usize;
            if needed_size - self.buffer.len() > 4 * 1024 * 1024 {
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

            self.current_position += size as usize;

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
    }

    fn release(
        &mut self,
        _req: &Request,
        _ino: u64,
        _fh: u64,
        _flags: u32,
        _lock_owner: u64,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        println!("Close: {}", _fh);

        if let Some(terminate_sender) = &self.terminate_sender {
            terminate_sender.send(());
        }

        reply.ok();
    }
}

fn main() {
    //env_logger::init();
    let mountpoint = env::args_os().nth(1).unwrap();
    let options = ["-o", "ro", "-o", "fsname=it9910hd_fs"]
        .iter()
        .map(|o| o.as_ref())
        .collect::<Vec<&OsStr>>();

    let it9910hd_fs = IT9910HD_FS::new();

    fuse::mount(it9910hd_fs, mountpoint, &options).unwrap();

    /*match run() {
        Err(err) => panic!("Cannot open Encoder: {}", err),
        _ => (),
    };*/
}
