#![feature(uniform_paths)]

use config::{Config, File as ConfigFile, FileFormat, Value as ConfigValue};
use journald::reader::{JournalReader, JournalReaderConfig, JournalSeek};
use serde_json::{to_string_pretty as pretty, Map as JsonMap, Value as JsonValue};

fn get_configs(default_config_paths: Vec<String>) -> Config {
    let default_yaml_config = include_str!("defaults.yaml");
    let mut config = Config::default();
    config
        .merge(ConfigFile::from_str(default_yaml_config, FileFormat::Yaml))
        .unwrap();
    if default_config_paths.len() > 0 {
        config
            .set("config_paths", ConfigValue::from(default_config_paths))
            .unwrap();
    }
    config
}

fn get_args() {}

fn main() {
    //let config_paths = vec![];
    let config_paths = vec!["/some/path".to_string()];
    let config = get_configs(config_paths);
    println!("{:#?}", config);
    let journal_reader_config = JournalReaderConfig::default();
    let mut journal_reader =
        JournalReader::open(&journal_reader_config).expect("Failed to open journal");
    journal_reader
        .seek(JournalSeek::Tail)
        .expect("Failed to seek to end of journal");
    let current_entry = journal_reader
        .previous_entry()
        .expect("Failed to get previous record")
        .unwrap();
    let fields = current_entry.fields;
    let mut json_map = JsonMap::new();
    let fields_iter = fields.into_iter();
    for (fields_key, fields_value) in fields_iter {
        json_map.insert(fields_key.into(), fields_value.to_string().into());
    }
    let json_value: JsonValue = json_map.into();
    let json_string = pretty(&json_value).unwrap();
    println!("{}", json_string);
}
