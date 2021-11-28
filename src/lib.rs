use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::io::BufReader;
use std::io::ErrorKind;
use std::io::SeekFrom;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::str::FromStr;
use std::thread::sleep;
use std::time::Duration;

/// Where shall it starts to print from
#[derive(Clone)]
pub enum StartFrom {
    /// The beginning of the file
    Beginning,
    /// Specify the cursor position offset
    Offset(u64),
    /// The end of the file, which is the last known position
    End,
}

impl FromStr for StartFrom {
    type Err = ();

    fn from_str(input: &str) -> Result<StartFrom, Self::Err> {
        match input {
            "start" => Ok(StartFrom::Beginning),
            "end" => Ok(StartFrom::End),
            _      => Ok(StartFrom::End),
        }
    }
}

pub enum LogWatcherAction {
    None,
    SeekToEnd,
}

pub struct LogWatcher {
    filename: String,
    refresh_preiod: u32,
    inode: u64,
    pos: u64,
    reader: BufReader<File>,
    finish: bool,
}

impl LogWatcher {
    pub fn register<P: AsRef<Path>>(
        filename: P,
        starts_from: StartFrom,
        refresh_period: u32
    ) -> Result<LogWatcher, io::Error> {
        let f = match File::open(&filename) {
            Ok(x) => x,
            Err(err) => return Err(err),
        };

        let metadata = match f.metadata() {
            Ok(x) => x,
            Err(err) => return Err(err),
        };

        let mut reader = BufReader::new(f);

        let starts_from = match starts_from {
            StartFrom::Beginning => 0u64,
            StartFrom::Offset(pos) => pos,
            StartFrom::End => metadata.len(),
        };

        reader.seek(SeekFrom::Start(starts_from)).unwrap();

        Ok(LogWatcher {
            filename: filename.as_ref().to_string_lossy().to_string(),
            refresh_preiod: refresh_period,
            inode: metadata.ino(),
            pos: starts_from,
            reader,
            finish: false,
        })
    }

    fn reopen_if_log_rotated<F: ?Sized>(&mut self, callback: &mut F)
    where
        F: FnMut(u64, usize, String) -> LogWatcherAction,
    {
        loop {
            match File::open(&self.filename) {
                Ok(x) => {
                    let f = x;
                    let metadata = match f.metadata() {
                        Ok(m) => m,
                        Err(_) => {
                            sleep(Duration::new(0, self.refresh_preiod * 1000));
                            continue;
                        }
                    };
                    if metadata.ino() != self.inode {
                        self.finish = true;
                        self.watch(callback);
                        self.finish = false;
                        println!("reloading log file");
                        self.reader = BufReader::new(f);
                        self.pos = 0;
                        self.inode = metadata.ino();
                    } else {
                        sleep(Duration::new(0, self.refresh_preiod * 1000));
                    }
                    break;
                }
                Err(err) => {
                    if err.kind() == ErrorKind::NotFound {
                        sleep(Duration::new(0, self.refresh_preiod * 1000));
                        continue;
                    }
                }
            };
        }
    }

    pub fn watch<F: ?Sized>(&mut self, callback: &mut F)
    where
        F: FnMut(u64, usize, String) -> LogWatcherAction,
    {
        loop {
            let mut line = String::new();
            let resp = self.reader.read_line(&mut line);
            match resp {
                Ok(len) => {
                    if len > 0 {
                        let old_pos = self.pos;
                        self.pos += len as u64;
                        self.reader.seek(SeekFrom::Start(self.pos)).unwrap();
                        match callback(old_pos, len, line.replace("\n", "")) {
                            LogWatcherAction::SeekToEnd => {
                                println!("SeekToEnd");
                                self.reader.seek(SeekFrom::End(0)).unwrap();
                            }
                            LogWatcherAction::None => {}
                        }
                        line.clear();
                    } else {
                        if self.finish {
                            break;
                        } else {
                            self.reopen_if_log_rotated(callback);
                            self.reader.seek(SeekFrom::Start(self.pos)).unwrap();
                        }
                    }
                }
                Err(err) => {
                    println!("{}", err);
                }
            }
        }
    }
}
