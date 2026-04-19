//! Streaming `.dclog` reader. Auto-detects raw vs. zstd-compressed files
//! by checking the first four bytes against the zstd frame magic.

use std::fs::File;
use std::io::{BufReader, Cursor, Read};
use std::path::Path;

use crate::spec::wire::{read_file_prefix, read_framed_entry, Entry};
use crate::spec::{LogHeader, Result};

const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];

pub type LogEntry = Entry;

pub struct LogReader {
    header: LogHeader,
    inner: Box<dyn Read>,
}

impl LogReader {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut file = File::open(path)?;
        let mut peek = [0u8; 4];
        file.read_exact(&mut peek)?;

        let chained = Cursor::new(peek.to_vec()).chain(file);

        let mut inner: Box<dyn Read> = if peek == ZSTD_MAGIC {
            Box::new(zstd::stream::Decoder::new(chained)?)
        } else {
            Box::new(BufReader::new(chained))
        };

        let header = read_file_prefix(&mut inner)?;
        Ok(Self { header, inner })
    }

    pub fn header(&self) -> &LogHeader {
        &self.header
    }

    pub fn next_entry(&mut self) -> Result<Option<LogEntry>> {
        read_framed_entry(&mut self.inner)
    }

    pub fn collect_entries(mut self) -> Result<Vec<LogEntry>> {
        let mut out = Vec::new();
        while let Some(e) = self.next_entry()? {
            out.push(e);
        }
        Ok(out)
    }
}
