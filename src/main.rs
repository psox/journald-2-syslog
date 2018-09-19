#![feature(uniform_paths)]

use clap::{app_from_crate, crate_authors, crate_description, crate_name, crate_version, Arg};
use config::{Config, File as ConfigFile, FileFormat, Value as ConfigValue};
use journald::reader::{JournalReader, JournalReaderConfig, JournalSeek};
use serde_json::{to_string_pretty as pretty, Map as JsonMap, Value as JsonValue};

fn get_configs(command_line_args: Config) -> Config {
    let default_yaml_config = include_str!("defaults.yaml");
    let mut config = Config::default();
    config
        .merge(ConfigFile::from_str(default_yaml_config, FileFormat::Yaml))
        .unwrap()
        .merge(command_line_args)
        .unwrap();
    config
}

fn get_command_line_args() -> Config {
    let mut config = Config::default();
    let app_matches = app_from_crate!()
        .args(&[
            Arg::with_name("configs")
                .short("c")
                .long("configs")
                .alias("config")
                .multiple(true)
                .takes_value(true)
                .help("Takes one or more configs files.")
                .long_help(include_str!("config_help.txt")),
            Arg::with_name("daemon")
                .long("daemon")
                .short("d")
                .required_unless("foreground")
                .help("Run the application in the background."),
            Arg::with_name("foreground")
                .long("foreground")
                .short("f")
                .required_unless("daemon")
                .conflicts_with("daemon")
                .help("Run the application in the foreground."),
            Arg::with_name("verbose")
                .long("verbose")
                .short("v")
                .help("Provide more detailed information")
                .takes_value(true)
                .possible_values(&["0", "1", "2", "3", "4", "5", "6", "8", "9"])
                .default_value("1"),
        ]).get_matches();
    for (arg_name, arg_value) in app_matches.args.into_iter() {
        let vals = &arg_value.vals;
        match arg_name {
            "verbose" => {
                config
                    .set(
                        arg_name.into(),
                        ConfigValue::from(
                            vals.get(0)
                                .unwrap()
                                .to_str()
                                .unwrap()
                                .to_string()
                                .parse::<i64>()
                                .unwrap(),
                        ),
                    ).unwrap();
            }
            "daemon" | "foreground" => {
                config
                    .set("run_mode".into(), ConfigValue::from(arg_name.to_string()))
                    .unwrap();
            }
            "configs" => {
                config
                    .set(
                        arg_name.into(),
                        ConfigValue::from(
                            vals.into_iter()
                                .map(|config_path| ConfigValue::from(config_path.to_str().unwrap()))
                                .collect::<Vec<ConfigValue>>(),
                        ),
                    ).unwrap();
            }
            arg_name => panic!("{} not processed having value {:?}", arg_name, arg_value),
        }
    }
    config
}

fn main() {
    let command_line_args = get_command_line_args();
    let final_config = get_configs(command_line_args);
    // let config_paths = vec!["/some/path".to_string()];
    // let config = get_configs(config_paths);
    if final_config.get_int("verbose").unwrap() >= 5 {
        println!("{:#?}", final_config);
    }
    if false {
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
}
