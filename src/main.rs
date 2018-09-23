#![feature(uniform_paths, try_from)]

use clap::{app_from_crate, crate_authors, crate_description, crate_name, crate_version, Arg};
use config::{Config, File as ConfigFile, FileFormat, Value as ConfigValue};
use journald::reader::{JournalReader, JournalReaderConfig, JournalSeek};
use serde_json::{to_string_pretty as pretty, Map as JsonMap, Value as JsonValue};
use serde_yaml::{to_string as to_yaml_string, Value as YamlValue};
use std::{collections::BTreeMap, iter::FromIterator, path::Path};

fn get_configs(command_line_args: Config) -> Config {
    // Load the default config file
    let default_yaml_config = include_str!("defaults.yaml");
    // Set to collect active config files
    let active_paths: BTreeMap<String, isize>;
    // Get the config paths that work
    {
        // Position in list
        let mut pos: isize = 0;
        // Create an empty config
        let mut config = Config::default();
        // Merge the default config with the command line args
        config
            .merge(ConfigFile::from_str(default_yaml_config, FileFormat::Yaml))
            .unwrap()
            .merge(command_line_args.clone())
            .unwrap();
        active_paths = config
            .get_array("configs")
            .unwrap()
            .into_iter()
            .map(|config_file| {
                (config_file.try_into::<String>().unwrap(), {
                    pos += 1;
                    pos
                })
            }).collect::<BTreeMap<String, isize>>()
            .into_iter()
            .filter(|(path, _)| Path::new(&path).exists())
            .collect::<BTreeMap<String, isize>>();
    }
    // Compose final merged config
    let mut config = Config::default();
    config
        .merge(ConfigFile::from_str(default_yaml_config, FileFormat::Yaml))
        .unwrap();
    let mut ordered_path_list = Vec::from_iter(active_paths.into_iter());
    ordered_path_list.sort_by(|&(_, a), &(_, b)| a.cmp(&b));
    let mut used_path: Vec<String> = vec![];
    for (path, _) in ordered_path_list.into_iter() {
        config.merge(ConfigFile::with_name(&path)).unwrap();
        used_path.push(path);
    }
    config.merge(command_line_args).unwrap();
    config
        .set(
            "configs",
            ConfigValue::from(
                used_path
                    .into_iter()
                    .map(|config_path| ConfigValue::from(config_path))
                    .collect::<Vec<ConfigValue>>(),
            ),
        ).unwrap();
    config
}

// Get Command Line Arguments
fn get_command_line_args() -> Config {
    // Create an empty config set
    let mut config = Config::default();
    // Get the Arguments from the command line
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
                .required_unless_one(&["foreground", "print-config", "list-config-files"])
                .conflicts_with_all(&["foreground", "print-config", "list-config-files"])
                .help("Run the application in the background."),
            Arg::with_name("foreground")
                .long("foreground")
                .short("f")
                .required_unless_one(&["daemon", "print-config", "list-config-files"])
                .conflicts_with_all(&["daemon", "print-config", "list-config-files"])
                .help("Run the application in the foreground."),
            Arg::with_name("verbose")
                .long("verbose")
                .short("v")
                .help("Provide more detailed information")
                .takes_value(true)
                .possible_values(&["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"])
                .default_value("1"),
            Arg::with_name("user")
                .long("user")
                .short("u")
                .takes_value(true)
                .help("The user to run as."),
            Arg::with_name("group")
                .long("group")
                .short("g")
                .takes_value(true)
                .help("The group to run as."),
            Arg::with_name("print-config")
                .long("print-config")
                .alias("pc")
                .alias("print")
                .required_unless_one(&["daemon", "foreground", "list-config-files"])
                .conflicts_with_all(&["daemon", "foreground", "list-config-files"])
                .help("Print the merged config used by this application."),
            Arg::with_name("list-config-files")
                .long("list-config-files")
                .alias("lcf")
                .alias("list")
                .required_unless_one(&["daemon", "foreground", "print-config"])
                .conflicts_with_all(&["daemon", "foreground", "print-config"])
                .help("List the config files used by this application."),
        ]).get_matches();
    // Process all the arguments presented
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
            "user" | "group" => {
                config
                    .set(
                        arg_name.into(),
                        ConfigValue::from(vals.get(0).unwrap().to_str().unwrap().to_string()),
                    ).unwrap();
            }
            "list-config-files" | "print-config" => {
                config
                    .set(arg_name.into(), ConfigValue::from(true))
                    .unwrap();
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
    // Return the resultant config
    config
}

fn main() {
    let command_line_args = get_command_line_args();
    let config = get_configs(command_line_args);
    if config.get_int("verbose").unwrap_or(0) >= 5 {
        println!("{:#?}", config);
    }
    if config.get_bool("list-config-files").unwrap_or(false) {
        for filename in config
            .get_array("configs")
            .unwrap_or(Vec::new())
            .into_iter()
        {
            println!(
                "{}",
                filename
                    .try_into::<String>()
                    .unwrap_or("-! Problem with Filename !-".to_string())
            );
        }
        return;
    }

    if config.get_bool("print-config").unwrap_or(false) {
        println!(
            "{}",
            to_yaml_string(&config.try_into::<YamlValue>().unwrap()).unwrap()
        );
        return;
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
