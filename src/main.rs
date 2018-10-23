#![feature(uniform_paths, try_from, stmt_expr_attributes)]
#![allow(unknown_lints)]
#![warn(clippy::all)]

#[macro_use]
extern crate serde_derive;

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

use serde_json::{
   Map as JsonMap,
   Value as JsonValue,
};

use serde_yaml::{
   from_str as yaml_from_str,
   to_string as to_yaml_string,
   to_writer as yaml_to_writer,
   Value as YamlValue,
};

use std::{
   collections::BTreeMap,
   fs::OpenOptions,
   io::{
      Read,
      Seek,
      SeekFrom,
      Write,
   },
   iter::FromIterator,
   net::{
      IpAddr,
      SocketAddr,
      TcpStream,
   },
   path::Path,
   result::Result as StdResult,
   sync::mpsc,
   thread,
   time::{
      Duration as StdDuration,
      Instant as StdInstant,
   },
};

use systemd::journal::{
   Journal,
   JournalFiles,
   JournalSeek,
};

type Result<T,> = StdResult<T, FailError,>;

#[derive(Serialize, Deserialize, Debug, Default, PartialEq, Clone)]
struct CursorRecord
{
   position : String,
}

#[derive(Serialize, Deserialize, Debug, Default, PartialEq, Clone)]
struct HostRecord
{
   host :     String,
   port :     u16,
   protocol : String,
}

fn send_json_to_remote_host(
   connection : &HostRecord,
   journal_entry : &mpsc::Receiver<JsonValue,>,
)
{
   loop
   {
      let ip : IpAddr = connection
         .host
         .parse()
         .unwrap_or_else(|_| "127.0.0.1".parse().unwrap(),);
      let address = SocketAddr::new(ip, connection.port,);
      let stream_result = TcpStream::connect(&address,);
      if let Ok(mut stream,) = stream_result
      {
         loop
         {
            let entry_result = journal_entry.recv();
            match entry_result
            {
               Ok(value,) =>
               {
                  let entry_json_string = value.to_string();
                  let write_result = stream.write_fmt(format_args!("{}\n", entry_json_string),);
                  match write_result
                  {
                     Ok((),) => (),
                     _ => break,
                  }
               },
               _ => break,
            }
         }
      }
      thread::sleep(Duration::seconds(17,).to_std().unwrap(),);
   }
}

fn read_write_cursor_thread(
   path : &str,
   cursor_sender : &mpsc::Sender<CursorRecord,>,
   cursor_receiver : &mpsc::Receiver<CursorRecord,>,
)
{
   let mut pit = StdInstant::now();
   let mut yaml_string = String::default();

   // Open cursor file
   let mut cursor_file = OpenOptions::new()
      .read(true,)
      .write(true,)
      .create(true,)
      .open(&path,)
      .unwrap_or_else(|error| panic!("{:#?}\nwhile trying to open file: {}", error, &path),);
   cursor_file.seek(SeekFrom::Start(0,),).unwrap_or_default();
   cursor_file.read_to_string(&mut yaml_string,).unwrap();

   let mut local_cursor_value : CursorRecord = yaml_from_str(&yaml_string,).unwrap_or_default();
   cursor_sender.send(local_cursor_value.clone(),).unwrap();
   loop
   {
      if let Ok(cursor,) = cursor_receiver.recv()
      {
         local_cursor_value = cursor;
      }
      if pit.elapsed() > StdDuration::from_millis(1234,)
      {
         cursor_file.seek(SeekFrom::Start(0,),).unwrap_or_default();
         cursor_file.set_len(0,).unwrap_or_default();
         yaml_to_writer(&cursor_file, &local_cursor_value,).unwrap_or((),);
         cursor_file.write(b"\n",).unwrap_or_default();
         pit = StdInstant::now();
      }
   }
}

fn get_configs(command_line_args : Config) -> Result<Config,>
{
   // Load the default config file
   let default_yaml_config = include_str!("../configs/defaults.yaml");

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
   let app_matches = app_from_crate!()
      .args(&[
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
               "How much history should be pre-loaded starting at some absolute point in time. \
                (YYYY-MM-DD T HH:mm:SS + TZ)",
            ),
         Arg::with_name("history-count",)
            .long("history-count",)
            .visible_alias("count",)
            .alias("hc",)
            .takes_value(true,)
            .allow_hyphen_values(true,)
            .conflicts_with_all(&["history-duration", "history-absolute",],)
            .help("How much history should be pre-loaded with this number of previous records.",),
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
         Arg::with_name("last-cursor-location",)
            .long("last-cursor-location",)
            .alias("lcl",)
            .visible_alias("cursor",)
            .short("l",)
            .takes_value(true,)
            .help("Path to the yaml file containing cursor of the last message passed",),
         Arg::with_name("host-name",)
            .long("host-name",)
            .visible_alias("hn",)
            .short("h",)
            .takes_value(true,)
            .help("The host name or IP where data should be sent.",),
         Arg::with_name("host-port",)
            .long("host-port",)
            .visible_alias("hp",)
            .short("p",)
            .takes_value(true,)
            .validator(|value| {
               let port = value.as_str().parse::<u16>().unwrap_or(0,);
               if port > 0 && port < 65535
               {
                  return Ok((),);
               }
               Err(String::from(
                  "The port should be an integer between 1 and 65534.",
               ),)
            },)
            .help("The host port number where data should be sent.",),
         Arg::with_name("host-type",)
            .long("host-type",)
            .visible_alias("ht",)
            .short("t",)
            .takes_value(true,)
            .possible_values(&["filebeat",],)
            .help("The type of the remote host to send data too.",),
         Arg::with_name("host-protocol",)
            .long("host-protocol",)
            .visible_alias("pr",)
            .short("P",)
            .possible_values(&["tcp", "udp",],)
            .takes_value(true,)
            .help("The host protocol to use.",),
      ],)
      .get_matches();

   // Process all the arguments presented
   for (arg_name, arg_value,) in app_matches.args.into_iter()
   {
      let vals = &arg_value.vals;

      #[allow(clippy::get_unwrap)]
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
         "host-port" =>
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
            config.set("run-mode", ConfigValue::from(arg_name.to_string(),),)?;
         },
         "host-name" | "host-type" | "host-protocol" | "last-cursor-location" =>
         {
            config.set(
               arg_name,
               ConfigValue::from(vals.get(0,).unwrap().to_str().unwrap(),),
            )?;
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
               "\nOh Shoot!\nI forgot to write code to process '{}' having the value of:- \n{:#?} \
                at ",
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
   let verbose = config.get_int("verbose",).unwrap_or(0,);
   let mut local_cursor_value = CursorRecord::default();
   let cursor_location_file = config.get_str("last-cursor-location",)?;
   let (json_value_sender, json_value_receiver,) = mpsc::channel::<JsonValue,>();
   let (cursor_exists_sender, cursor_exists_receiver,) = mpsc::channel::<CursorRecord,>();
   let (cursor_value_sender, cursor_value_receiver,) = mpsc::channel::<CursorRecord,>();
   let thread_builder = thread::Builder::new().name("read_write_cursor_thread".into(),);

   if verbose >= 5
   {
      eprintln!("{:#?}", config);
   }

   if config.get_bool("list-config-files",).unwrap_or(false,)
   {
      for filename in config.get_array("configs",).unwrap_or_default().into_iter()
      {
         eprintln!(
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

   let _cursor_tread = thread_builder.spawn(move || {
      read_write_cursor_thread(
         cursor_location_file.as_str(),
         &cursor_exists_sender,
         &cursor_value_receiver,
      )
   },);

   let remote_host = HostRecord {
      host :     config
         .get_str("host-name",)
         .unwrap_or_else(|_| "127.0.0.1".to_string(),),
      port :     config
         .get_int("host-port",)
         .unwrap_or(9000,)
         .to_string()
         .parse::<u16>()
         .unwrap(),
      protocol : config
         .get_str("host-protocol",)
         .unwrap_or_else(|_| "tcp".to_string(),),
   };
   let _network_thread =
      thread::spawn(move || send_json_to_remote_host(&remote_host, &json_value_receiver,),);

   let mut journal = Journal::open(JournalFiles::All, false, false,)?;

   // Seek to an appropriate postion if the cursor is not set
   if let Ok(cursor,) = cursor_exists_receiver.recv()
   {
      local_cursor_value = cursor;
   }
   if local_cursor_value == CursorRecord::default()
   {
      match config.get_str("history-type",)?.as_str()
      {
         "duration" =>
         {
            let duration = Duration::from_std(parse_duration(
               config.get_str("history-duration",)?.as_str(),
            )?,)?;

            if verbose > 1
            {
               eprintln!("   Seek Duration: {:?}", duration);
            }

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

            if verbose > 1
            {
               eprintln!("   Seek Absolute: {:?}", absolute);
            }

            journal.seek(JournalSeek::ClockRealtime {
               usec : absolute,
            },)?;
         },
         "count" =>
         {
            let count : i64 = config.get_int("history-count",)?;

            if verbose > 1
            {
               eprintln!("   Seek Records: {:?}", count);
            }

            if count > 0
            {
               journal.seek(JournalSeek::Head,)?;
               // Make sure we are at the begining
               loop
               {
                  if let Ok(None,) = journal.previous_record()
                  {
                     break;
                  }
               }

               for _ in 0 .. count
               {
                  if let Ok(None,) = journal.next_record()
                  {
                     break;
                  }
               }

               (0 .. count).for_each(|_| {
                  journal.next_record().unwrap();
               },);
            }
            else if count < 0
            {
               journal.seek(JournalSeek::Tail,)?;
               // Tail does not always go to the end
               loop
               {
                  if let Ok(None,) = journal.next_record()
                  {
                     break;
                  }
               }

               for _ in count .. 0
               {
                  if let Ok(None,) = journal.previous_record()
                  {
                     break;
                  }
               }
            }
            else
            {
               journal.seek(JournalSeek::Tail,)?;
               // Tail does not always go to the end
               loop
               {
                  if let Ok(None,) = journal.next_record()
                  {
                     break;
                  }
               }
            }
         },
         history_type => panic!("{} is not a valid history-type!", history_type),
      }
   }

   loop 
   {
      let candidate = journal.next_record()?;
      let record = match candidate 
      {
         Some(matched_record) => matched_record,
         None => 
         { 
            loop 
            {
               if let Some(matched_record) = journal.await_next_record(None)? 
               {
                  break matched_record;
               }
            }
         }
      };

      local_cursor_value.position = journal.cursor().unwrap_or_default();
      if  local_cursor_value != CursorRecord::default()
      {
         let timestamp : DateTime<Utc,> = journal
            .timestamp()
            .unwrap_or_else(|_| Utc::now().into(),)
            .into();    
         let timestamp_str = timestamp.to_rfc3339().replace("+00:00", "Z",);
         let mut json_map = JsonMap::new();
         json_map.insert("@timestamp".into(), timestamp_str.clone().into(),);
         json_map.insert("journald.timestamp".into(), timestamp_str.into(),);
         json_map.insert(
            "journald.cursor".into(),
            local_cursor_value.position.clone().into(),
         );
         record.into_iter().for_each(|(record_key, record_value,)| 
         {
            json_map.insert(
                     record_key
                        .replace("_", ".",)
                        .to_lowercase()
                        .trim_left_matches('.',)
                        .replace("source", "originator",)
                        .replace("message.", "originator.",),
                     record_value.as_str().into(),
                  );
         },);
         let json_value : JsonValue = json_map.into();
         json_value_sender
               .send(json_value.clone(),)
               .unwrap_or_default();
         cursor_value_sender
               .send(local_cursor_value.clone(),)
               .unwrap_or_default();
         if config.get_str("run-mode",).unwrap_or_else(|_| "".into(),) == "foreground"
         {
            match verbose
            {
               3 | 4 | 5 | 6 =>
               {
                  let json_string = serde_json::to_string(&json_value,)?;
                  println!("{}", json_string);
               },
               7 | 8 | 9 =>
               {
                  let json_string_pretty = serde_json::to_string_pretty(&json_value,)?;
                  println!("{}", json_string_pretty);
               },
               _ => (),
            }
         }
      }
   }
}

fn main()
{
   main_wrapper().unwrap();
}
