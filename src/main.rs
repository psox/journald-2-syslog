#![feature(uniform_paths)]

use journald::reader::*;
use journald::JournalEntry;

fn main() {
    let journal_reader_config = JournalReaderConfig::default();
    let mut journal_reader =
        JournalReader::open(&journal_reader_config).expect("Failed to open journal");
    journal_reader
        .seek(JournalSeek::Tail)
        .expect("Failed to seek to end of journal");
    let current_entry =  journal_reader
        .previous_entry() 
        .expect("Failed to get previous record").unwrap();

    println!("{:#?}", current_entry);
}
