#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum ProtocolType
{
   UDP,
   TCP,
}

impl Default for ProtocolType
{
   fn default() -> ProtocolType
   {
      ProtocolType::TCP
   }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum History
{
   Duration(String,),
   Absolute(String,),
   Count(i64,),
}

impl Default for History
{
   fn default() -> History
   {
      History::Count(-1,)
   }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum RunType
{
   Foreground,
   Daemon,
   Print,
   List,
}

impl Default for RunType
{
   fn default() -> RunType
   {
      RunType::Print
   }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum TargetType
{
   Filebeat,
}

impl Default for TargetType
{
   fn default() -> TargetType
   {
      TargetType::Filebeat
   }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct TargetRecord
{
   address :  String,
   port :     u32,
   protocol : ProtocolType,
   target :   TargetType,
}

impl Default for TargetRecord
{
   fn default() -> TargetRecord
   {
      TargetRecord {
         address :  "127.0.0.1".to_string(),
         port :     9000,
         protocol : ProtocolType::default(),
         target :   TargetType::default(),
      }
   }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct JDConfig
{
   configs :  Vec<String,>,
   verbose :  isize,
   state :    String,
   run_type : RunType,
   history :  History,
   targets :  Vec<TargetRecord,>,
}

impl Default for JDConfig
{
   fn default() -> JDConfig
   {
      JDConfig {
         configs :  vec![
            "/usr/share/journaldeliver/default.yaml".to_string(),
            "/var/lib/journaldeliver/default.yaml".to_string(),
            "/etc/journaldeliver/default.yaml".to_string(),
         ],
         verbose :  1,
         state :    "/var/lib/journaldeliver/cursor-location.yaml".to_string(),
         run_type : RunType::default(),
         history :  History::default(),
         targets :  vec![TargetRecord::default()],
      }
   }
}
