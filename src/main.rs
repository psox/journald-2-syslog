#![feature(uniform_paths, try_from)]
#![allow(unknown_lints)]
#![warn(clippy::all)]

use chrono::{
   DateTime,
   Duration,
   Utc,
};
use clap::{
   app_from_crate,
   crate_authors,
   crate_description,
   crate_name,
   crate_version,
   Arg,
};

use config::{
   Config,
   File as ConfigFile,
   FileFormat,
   Value as ConfigValue,
};
use failure::Error as FailError;
use parse_duration::parse as parse_duration;

// use serde_json::{to_string_pretty as pretty, Map as JsonMap, Value as
// JsonValue};
use serde_yaml::{
   to_string as to_yaml_string,
   Value as YamlValue,
};
use std::{
   collections::BTreeMap,
   iter::FromIterator,
   path::Path,
   result::Result as StdResult,
};
use systemd::journal::{
   Journal,
   JournalFiles,
   JournalSeek,
};

type Result<T,> = StdResult<T, FailError,>;

fn get_configs(command_line_args : Config) -> Result<Config,>
{
   // Load the default config file
   let default_yaml_config = include_str!("defaults.yaml");

   // Set to collect active config files
   let active_paths : BTreeMap<String, isize,>;

   // Get the config paths that work
   {
      // Position in list
      let mut pos : isize = 0;

      // Create an empty config
      let mut config = Config::default();

      // Merge the default config with the command line args
      config
         .merge(ConfigFile::from_str(default_yaml_config, FileFormat::Yaml,),)?
         .merge(command_line_args.clone(),)?;

      active_paths = config
         .get_array("configs",)?
         .into_iter()
         .map(|config_file| {
            (config_file.try_into::<String>().unwrap(), {
               pos += 1;

               pos
            },)
         },)
         .collect::<BTreeMap<String, isize,>>()
         .into_iter()
         .filter(|(path, _,)| Path::new(&path,).exists(),)
         .collect::<BTreeMap<String, isize,>>();
   }

   // Compose final merged config
   let mut config = Config::default();

   config.merge(ConfigFile::from_str(default_yaml_config, FileFormat::Yaml,),)?;

   let mut ordered_path_list = Vec::from_iter(active_paths.into_iter(),);

   ordered_path_list.sort_by(|&(_, a,), &(_, b,)| a.cmp(&b,),);

   let mut used_path : Vec<String,> = vec![];

   for (path, _,) in ordered_path_list.into_iter()
   {
      config.merge(ConfigFile::with_name(&path,),)?;

      used_path.push(path,);
   }

   config.merge(command_line_args,)?.set(
      "configs",
      ConfigValue::from(
         used_path
            .into_iter()
            .map(ConfigValue::from,)
            .collect::<Vec<ConfigValue,>>(),
      ),
   )?;

   Ok(config,)
}

// Get Command Line Arguments
fn get_command_line_args() -> Result<Config,>
{
   // Create an empty config set
   let mut config = Config::default();

   // Get the Arguments from the command line
   let app_matches =
      app_from_crate!()
         .args(
            &[
               Arg::with_name("configs",)
                  .short("c",)
                  .long("configs",)
                  .alias("config",)
                  .multiple(true,)
                  .takes_value(true,)
                  .help("Takes one or more configs files.",)
                  .long_help(include_str!("config_help.txt"),),
               Arg::with_name("daemon",)
                  .long("daemon",)
                  .short("d",)
                  .required_unless_one(&["foreground", "print-config", "list-config-files",],)
                  .conflicts_with_all(&["foreground", "print-config", "list-config-files",],)
                  .help("Run the application in the background.",),
               Arg::with_name("foreground",)
                  .long("foreground",)
                  .short("f",)
                  .required_unless_one(&["daemon", "print-config", "list-config-files",],)
                  .conflicts_with_all(&["daemon", "print-config", "list-config-files",],)
                  .help("Run the application in the foreground.",),
               Arg::with_name("verbose",)
                  .long("verbose",)
                  .short("v",)
                  .help("Provide more detailed information",)
                  .takes_value(true,)
                  .possible_values(&["0", "1", "2", "3", "4", "5", "6", "7", "8", "9",],)
                  .default_value("1",),
               Arg::with_name("history-duration",)
                  .long("history-duration",)
                  .visible_alias("time",)
                  .alias("hd",)
                  .takes_value(true,)
                  .conflicts_with_all(&["history-absolute", "history-count",],)
                  .help("How much history should be pre-loaded counting back from now.",),
               Arg::with_name("history-absolute",)
                  .long("history-absolute",)
                  .visible_alias("absolute",)
                  .alias("ha",)
                  .takes_value(true,)
                  .conflicts_with_all(&["history-duration", "history-count",],)
                  .help(
                     "How much history should be pre-loaded starting at some absolute point in \
                      time.",
                  ),
               Arg::with_name("history-count",)
                  .long("history-count",)
                  .visible_alias("count",)
                  .alias("hc",)
                  .takes_value(true,)
                  .conflicts_with_all(&["history-duration", "history-absolute",],)
                  .help(
                     "How much history should be pre-loaded with this number of previous records.",
                  ),
               Arg::with_name("print-config",)
                  .long("print-config",)
                  .alias("pc",)
                  .visible_alias("print",)
                  .required_unless_one(&["daemon", "foreground", "list-config-files",],)
                  .conflicts_with_all(&["daemon", "foreground", "list-config-files",],)
                  .help("Print the merged config used by this application.",),
               Arg::with_name("list-config-files",)
                  .long("list-config-files",)
                  .alias("lcf",)
                  .visible_alias("list",)
                  .required_unless_one(&["daemon", "foreground", "print-config",],)
                  .conflicts_with_all(&["daemon", "foreground", "print-config",],)
                  .help("List the config files used by this application.",),
               Arg::with_name("host-name",)
                  .long("host-name",)
                  .visible_alias("hn",)
                  .short("H",)
                  .help("The host name or IP where data should be sent.",),
               Arg::with_name("host-port",)
                  .long("host-port",)
                  .visible_alias("hp",)
                  .short("P",)
                  .help("The host port number where data should be sent.",),
            ],
         )
         .get_matches();

   // Process all the arguments presented
   for (arg_name, arg_value,) in app_matches.args.into_iter()
   {
      let vals = &arg_value.vals;

      match arg_name
      {
         "verbose" =>
         {
            config.set(
               arg_name,
               ConfigValue::from(
                  vals
                     .get(0,)
                     .unwrap()
                     .to_str()
                     .unwrap()
                     .to_string()
                     .parse::<i64>()?,
               ),
            )?;
         },
         "list-config-files" | "print-config" =>
         {
            config.set(arg_name, ConfigValue::from(true,),)?;
         },
         "daemon" | "foreground" =>
         {
            config.set("run_mode", ConfigValue::from(arg_name.to_string(),),)?;
         },
         "configs" =>
         {
            config.set(
               arg_name,
               ConfigValue::from(
                  vals
                     .into_iter()
                     .map(|config_path| ConfigValue::from(config_path.to_str().unwrap(),),)
                     .collect::<Vec<ConfigValue,>>(),
               ),
            )?;
         },
         "history-duration" =>
         {
            config
               .set(
                  arg_name,
                  ConfigValue::from(vals.get(0,).unwrap().to_str().unwrap(),),
               )?
               .set("history-type", ConfigValue::from("duration",),)?;
         },
         "history-absolute" =>
         {
            config
               .set(
                  arg_name,
                  ConfigValue::from(vals.get(0,).unwrap().to_str().unwrap(),),
               )?
               .set("history-type", ConfigValue::from("absolute",),)?;
         },
         "history-count" =>
         {
            config
               .set(
                  arg_name,
                  ConfigValue::from(
                     vals
                        .get(0,)
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .parse::<i64>()
                        .unwrap(),
                  ),
               )?
               .set("history-type", ConfigValue::from("count",),)?;
         },
         arg_name =>
         {
            panic!(
               "{} not processed having value:- \n{:#?}",
               arg_name, arg_value
            )
         },
      }
   }

   // Return the resultant config
   Ok(config,)
}

fn main_wrapper() -> Result<(),>
{
   let command_line_args = get_command_line_args()?;

   let config = get_configs(command_line_args,)?;

   if config.get_int("verbose",).unwrap_or(0,) >= 5
   {
      println!("{:#?}", config);
   }

   if config.get_bool("list-config-files",).unwrap_or(false,)
   {
      for filename in config.get_array("configs",).unwrap_or_default().into_iter()
      {
         println!(
            "{}",
            filename
               .try_into::<String>()
               .unwrap_or_else(|_| "-! Problem with Filename !-".to_string())
         );
      }

      return Ok((),);
   }

   if config.get_bool("print-config",).unwrap_or(false,)
   {
      println!("{}", to_yaml_string(&config.try_into::<YamlValue>()?)?);

      return Ok((),);
   }

   let _cursor_location_file = config.get_str("last-cursor-location",)?;

   let mut journal = Journal::open(JournalFiles::All, false, false,)?;

   match config.get_str("history-type",)?.as_str()
   {
      "duration" =>
      {
         let duration = Duration::from_std(parse_duration(
            config.get_str("history-duration",)?.as_str(),
         )?,)?;

         if duration != Duration::seconds(0,)
         {
            let now : DateTime<Utc,> = Utc::now();

            let start_time : u64 = now
               .checked_sub_signed(duration,)
               .unwrap()
               .timestamp()
               .to_string()
               .parse::<u64>()?
               * 1_000_000;

            journal.seek(JournalSeek::ClockRealtime {
               usec : start_time,
            },)?;
         }
         else
         {
            journal.seek(JournalSeek::Tail,)?;
         }
      },
      "absolute" =>
      {
         let absolute = config
            .get_str("history-absolute",)?
            .as_str()
            .parse::<DateTime<Utc,>>()?
            .timestamp()
            .to_string()
            .parse::<u64>()?
            * 1_000_000;

         journal.seek(JournalSeek::ClockRealtime {
            usec : absolute,
         },)?;
      },
      "count" =>
      {
         let count : i64 = config.get_int("history-count",)?;

         if count > 0
         {
            journal.seek(JournalSeek::Head,)?;

            (0 .. count).for_each(|_| {
               journal.next_record().unwrap();
            },);
         }
         else
         {
            journal.seek(JournalSeek::Tail,)?;

            (count .. 0).for_each(|_| {
               journal.previous_record().unwrap();
            },);
         }
      },
      history_type => panic!("{} is not a valid history-type!", history_type),
   }

   // if false {
   //     journal_reader
   //         .seek(JournalSeek::Tail)
   //         .expect("Failed to seek to end of journal");
   //     let current_entry = journal_reader
   //         .previous_entry()
   //         .expect("Failed to get previous record")
   //         .unwrap();
   //     let fields = current_entry.fields;
   //     let mut json_map = JsonMap::new();
   //     let fields_iter = fields.into_iter();
   //     for (fields_key, fields_value) in fields_iter {
   //         json_map.insert(fields_key.into(), fields_value.to_string().into());
   //     }
   //     let json_value: JsonValue = json_map.into();
   //     let json_string = pretty(&json_value).unwrap();
   //     println!("{}", json_string);
   // }

   Ok((),)
}

fn main()
{
   main_wrapper().unwrap();
}
