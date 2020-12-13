use std::convert::{TryFrom, TryInto};
use std::error::Error;
use std::num::ParseIntError;
use std::str::FromStr;

use regex::Regex;
use simple_error::{bail, simple_error};
use std::collections::HashMap;
use subprocess;

pub type AttributeId = u32;
pub type DeviceId = u32;
pub type DeviceStatus = String;

#[derive(Debug, Eq, PartialEq)]
pub struct ShortDevice {
    pub id: DeviceId,
    pub name: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttributeType {
    Bool,
    String,
    UInt8,
    UInt16,
    UInt32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AttributeValue {
    NoValue,
    Bool(bool),
    String(String),
    UInt8(u8),
    UInt16(u16),
    UInt32(u32),
}

impl AttributeType {
    pub fn parse(&self, s: &str) -> Result<AttributeValue, Box<dyn Error>> {
        let payload_str = s.trim();
        Ok(match self {
            AttributeType::UInt8 => AttributeValue::UInt8(payload_str.parse::<u8>()?),
            AttributeType::UInt16 => AttributeValue::UInt16(payload_str.parse::<u16>()?),
            AttributeType::UInt32 => AttributeValue::UInt32(payload_str.parse::<u32>()?),
            AttributeType::String => AttributeValue::String(payload_str.to_string()),
            AttributeType::Bool => {
                AttributeValue::Bool(match payload_str.to_ascii_lowercase().as_str() {
                    "true" | "1" | "yes" | "on" => true,
                    "false" | "0" | "no" | "off" => false,
                    _ => bail!("Bad boolean value: {}", payload_str),
                })
            }
        })
    }

    pub fn parse_json(&self, s: &serde_json::Value) -> Result<AttributeValue, Box<dyn Error>> {
        Ok(match (s, self) {
            (serde_json::Value::String(s), AttributeType::String) => {
                AttributeValue::String(s.clone())
            }
            (v, AttributeType::String) => AttributeValue::String(v.to_string()),
            (serde_json::Value::Number(n), AttributeType::UInt8) => AttributeValue::UInt8(
                n.as_u64()
                    .ok_or_else(|| simple_error!("{} is not a u64", n))?
                    .try_into()?,
            ),
            (serde_json::Value::Number(n), AttributeType::UInt16) => AttributeValue::UInt16(
                n.as_u64()
                    .ok_or_else(|| simple_error!("{} is not a u64", n))?
                    .try_into()?,
            ),
            (serde_json::Value::Number(n), AttributeType::UInt32) => AttributeValue::UInt32(
                n.as_u64()
                    .ok_or_else(|| simple_error!("{} is not a u64", n))?
                    .try_into()?,
            ),
            (serde_json::Value::Bool(v), AttributeType::Bool) => AttributeValue::Bool(*v),
            (v, _) => {
                bail!("unknown value for type {:?}: {}", self, v);
            }
        })
    }
}

impl AttributeValue {
    pub fn attribute_type(&self) -> Option<AttributeType> {
        match self {
            AttributeValue::NoValue => None,
            AttributeValue::Bool(_) => Some(AttributeType::Bool),
            AttributeValue::String(_) => Some(AttributeType::String),
            AttributeValue::UInt8(_) => Some(AttributeType::UInt8),
            AttributeValue::UInt16(_) => Some(AttributeType::UInt16),
            AttributeValue::UInt32(_) => Some(AttributeType::UInt32),
        }
    }

    pub fn or<'a>(&'a self, other: &'a AttributeValue) -> &'a AttributeValue {
        if *self == AttributeValue::NoValue {
            other
        } else {
            self
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        match self {
            AttributeValue::NoValue => serde_json::Value::Null,
            AttributeValue::Bool(b) => serde_json::Value::Bool(*b),
            AttributeValue::UInt8(i) => serde_json::Value::Number(serde_json::Number::from(*i)),
            AttributeValue::UInt16(i) => serde_json::Value::Number(serde_json::Number::from(*i)),
            AttributeValue::UInt32(i) => serde_json::Value::Number(serde_json::Number::from(*i)),
            AttributeValue::String(s) => serde_json::Value::String(s.clone()),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct DeviceAttribute {
    pub id: AttributeId,
    pub description: String,
    pub attribute_type: AttributeType,
    pub supports_write: bool,
    pub supports_read: bool,
    pub current_value: AttributeValue,
    pub setting_value: AttributeValue,
}

#[derive(Debug, Eq, PartialEq)]
pub struct LongDevice {
    // These probably don't change often
    pub gang_id: Option<u32>,
    pub generic_device_type: Option<u8>,
    pub specific_device_type: Option<u8>,
    pub manufacturer_id: Option<u16>,
    pub product_type: Option<u16>,
    pub product_number: Option<u16>,

    pub id: DeviceId,
    pub status: DeviceStatus,
    pub name: String,
    pub attributes: Vec<DeviceAttribute>,
}

impl LongDevice {
    pub fn attribute(&self, s: &str) -> Option<&DeviceAttribute> {
        self.attributes.iter().find(|x| x.description == s)
    }
}

pub trait DeviceController: Send {
    fn list(&self) -> Result<Vec<ShortDevice>, Box<dyn Error>>;
    fn describe(&self, master_id: DeviceId) -> Result<LongDevice, Box<dyn Error>>;
    fn set(
        &mut self,
        master_id: DeviceId,
        attribute_id: AttributeId,
        value: &AttributeValue,
    ) -> Result<(), Box<dyn Error>>;
}

pub struct AprontestController {
    runner: fn(command: &[&str]) -> Result<String, Box<dyn Error>>,
}

impl AprontestController {
    pub fn new() -> AprontestController {
        AprontestController {
            runner: |cmd| {
                let result = subprocess::Exec::cmd(cmd[0]).args(&cmd[1..]).capture()?;
                if !result.success() {
                    bail!("Calling aprontest failed. Something went horribly wrong.\nCommand: {}\nStderr: {}", cmd.join(" "), result.stderr_str())
                };
                Ok(result.stdout_str())
            },
        }
    }
}

lazy_static! {
    static ref DEVICE_REGEX_STR: String = r"\s*(?P<id>\d+)\s*\|\s*(?P<interconnect>[^ |]*)\s*\|\s*(?P<name>[^\n]+)".to_owned();
    static ref LIST_REGEX: Regex = Regex::new(&(r"(?ms)^Found \d+ devices in .*MASTERID\s*\|\s*INTERCONNECT\s*\|\s*USERNAME(?P<devices>(?:".to_owned() + &DEVICE_REGEX_STR+ ")*)")).unwrap();
    static ref DEVICE_REGEX : Regex = Regex::new(&DEVICE_REGEX_STR).unwrap();

    static ref ATTRIBUTE_REGEX_STR: String = r"\s*(?P<id>\d+)\s*\|\s*(?P<description>[^\|]+)\s*\|\s*(?P<type>[^ ]+)\s*\|\s*(?P<mode>[^ ]+)\s*\|\s*(?P<get>[^ ]*)\s*\| *(?P<set>[^\n ]*)".to_owned();
    static ref LONG_DEVICE_REGEX : Regex = Regex::new(&((
    "".to_owned() +
    r"(?ms)(?:Gang ID: (?P<gang_id>(0x)?[0-9a-fA-F]+)\n)?" +
    // r"(?:[^\n]+\n)*" +
    r"(?:Generic/Specific device types: (?P<generic_device_type>(0x)?[0-9a-fA-F]+)/(?P<specific_device_type>(0x)?[0-9a-fA-F]+)\n)?" +
    // r"(?:[^\n]+\n)*" +
    r"(?:Manufacturer ID: (?P<manufacturer_id>(0x)?[0-9A-Fa-f]+) Product Type: (?P<product_type>(0x)?[0-9A-Fa-f]+) Product Number: (?P<product_number>(0x)?[0-9A-Fa-f]+)\n)?" +
    // r"(?:[^\n]+\n)*" +
    r"(?:Device is (?P<device_status>[^,]+)[^\n]+\n)?" +
    r"(?:[^\n]+\n)*" +
    r"(?P<name>[^\n]+)\n" +
    r"\s*ATTRIBUTE\s*\|\s*DESCRIPTION\s*\|\s*TYPE\s*\|\s*MODE\s*\|\s*GET\s*\|\s*SET" +
    r"(?P<attributes>(?:").to_owned() + &ATTRIBUTE_REGEX_STR + ")*)"
    )).unwrap();
    static ref ATTRIBUTE_REGEX : Regex = Regex::new(&ATTRIBUTE_REGEX_STR).unwrap();
}

trait Numberish {
    fn parse_numberish<T: TryFrom<u64>>(&self) -> Result<T, ParseIntError>;
}

impl Numberish for str {
    fn parse_numberish<T: TryFrom<u64>>(&self) -> Result<T, ParseIntError> {
        let inu64 = if let Some(number) = self.strip_prefix("0x") {
            u64::from_str_radix(number.trim_start_matches("0"), 16)?
        } else {
            self.parse()?
        };

        match T::try_from(inu64) {
            Ok(v) => Ok(v),
            Err(_) => Err(u8::from_str("257").unwrap_err()),
        }
    }
}

fn parse_attr_value(t: AttributeType, v: &str) -> Result<AttributeValue, Box<dyn Error>> {
    Ok(match v {
        "" => AttributeValue::NoValue,
        v => match t {
            AttributeType::UInt8 => AttributeValue::UInt8(v.parse()?),
            AttributeType::UInt16 => AttributeValue::UInt16(v.parse()?),
            AttributeType::UInt32 => AttributeValue::UInt32(v.parse()?),
            AttributeType::Bool => AttributeValue::Bool(match v {
                "TRUE" => true,
                "FALSE" => false,
                _ => bail!("Bad attribute value: {}", v),
            }),
            AttributeType::String => AttributeValue::String(v.to_string()),
        },
    })
}

impl DeviceController for AprontestController {
    fn list(&self) -> Result<Vec<ShortDevice>, Box<dyn Error>> {
        let stdout = (self.runner)(&["aprontest", "-l"])?;
        let devices = match LIST_REGEX.captures(&stdout) {
            Some(v) => v,
            _ => bail!("Output doesn't match regex:\n{}", stdout),
        }
        .name("devices")
        .unwrap()
        .as_str();

        Ok(DEVICE_REGEX
            .captures_iter(devices)
            .map(|m| ShortDevice {
                id: m.name("id").unwrap().as_str().parse().unwrap(),
                name: m.name("name").unwrap().as_str().to_string(),
            })
            .collect())
    }

    fn describe(&self, master_id: DeviceId) -> Result<LongDevice, Box<dyn Error>> {
        let stdout = (self.runner)(&["aprontest", "-l", "-m", &format!("{}", master_id)])?;

        let parsed = match LONG_DEVICE_REGEX.captures(&stdout) {
            Some(v) => v,
            _ => bail!("Output does not match regex:\n{}", stdout),
        };

        Ok(LongDevice {
            gang_id: parsed
                .name("gang_id")
                .map(|v| v.as_str().parse_numberish())
                .transpose()?,
            generic_device_type: parsed
                .name("generic_device_type")
                .map(|v| v.as_str().parse_numberish())
                .transpose()?,
            specific_device_type: parsed
                .name("specific_device_type")
                .map(|v| v.as_str().parse_numberish())
                .transpose()?,
            manufacturer_id: parsed
                .name("manufacturer_id")
                .map(|v| v.as_str().parse_numberish())
                .transpose()?,
            product_type: parsed
                .name("product_type")
                .map(|v| v.as_str().parse_numberish())
                .transpose()?,
            product_number: parsed
                .name("product_number")
                .map(|v| v.as_str().parse_numberish())
                .transpose()?,
            id: master_id,
            status: parsed
                .name("device_status")
                .map_or("", |v| v.as_str())
                .to_string(),
            name: parsed.name("name").map_or("", |v| v.as_str()).to_string(),
            attributes: ATTRIBUTE_REGEX
                .captures_iter(parsed.name("attributes").unwrap().as_str())
                .map(|m| -> Result<DeviceAttribute, Box<dyn Error>> {
                    let attribute_type = match m.name("type").unwrap().as_str() {
                        "UINT8" => AttributeType::UInt8,
                        "UINT16" => AttributeType::UInt16,
                        "UINT32" => AttributeType::UInt32,
                        "BOOL" => AttributeType::Bool,
                        "STRING" => AttributeType::String,
                        _ => bail!("Bad attribute type: {}", m.name("type").unwrap().as_str()),
                    };
                    Ok(DeviceAttribute {
                        id: m.name("id").unwrap().as_str().parse()?,
                        description: m.name("description").unwrap().as_str().trim().to_string(),
                        attribute_type,
                        supports_write: m.name("mode").unwrap().as_str().contains("W"),
                        supports_read: m.name("mode").unwrap().as_str().contains("R"),
                        current_value: parse_attr_value(
                            attribute_type,
                            m.name("get").unwrap().as_str().trim(),
                        )?,
                        setting_value: parse_attr_value(
                            attribute_type,
                            m.name("set").unwrap().as_str().trim(),
                        )?,
                    })
                })
                .collect::<Result<Vec<DeviceAttribute>, Box<dyn Error>>>()?,
        })
    }

    fn set(
        &mut self,
        master_id: DeviceId,
        attribute_id: AttributeId,
        value: &AttributeValue,
    ) -> Result<(), Box<dyn Error>> {
        let value = match value {
            AttributeValue::NoValue => bail!("Invalid attribute value: none"),
            AttributeValue::UInt8(v) => format!("{}", v),
            AttributeValue::UInt16(v) => format!("{}", v),
            AttributeValue::UInt32(v) => format!("{}", v),
            AttributeValue::Bool(v) => if *v { "TRUE" } else { "FALSE" }.to_string(),
            AttributeValue::String(v) => v.clone(),
        };
        (self.runner)(&[
            "aprontest",
            "-u",
            "-m",
            &format!("{}", master_id),
            "-t",
            &format!("{}", attribute_id),
            "-v",
            &value,
        ])?;
        Ok(())
    }
}

pub struct FakeController {
    attr_values: HashMap<(DeviceId, AttributeId), AttributeValue>,
}

impl FakeController {
    pub fn new() -> FakeController {
        FakeController {
            attr_values: HashMap::new(),
        }
    }
}

impl DeviceController for FakeController {
    fn list(&self) -> Result<Vec<ShortDevice>, Box<dyn Error>> {
        Ok(vec![ShortDevice {
            id: 2,
            name: "Bedroom Fan".to_string(),
        }])
    }

    fn describe(&self, master_id: u32) -> Result<LongDevice, Box<dyn Error>> {
        match master_id {
            2 => Ok(LongDevice {
                gang_id: Some(0x03),
                generic_device_type: Some(0x11),
                specific_device_type: Some(0x08),
                manufacturer_id: Some(0x63),
                product_type: Some(0x4944),
                product_number: Some(0x3131),
                id: 2,
                status: "ONLINE".to_string(),
                name: "Bedroom Fan".to_string(),
                attributes: vec![
                    DeviceAttribute {
                        id: 1,
                        description: "GenericValue".to_string(),
                        attribute_type: AttributeType::UInt8,
                        supports_write: true,
                        supports_read: true,
                        current_value: self
                            .attr_values
                            .get(&(master_id, 1 as AttributeId))
                            .unwrap_or(&AttributeValue::UInt8(0))
                            .clone(),
                        setting_value: self
                            .attr_values
                            .get(&(master_id, 1 as AttributeId))
                            .unwrap_or(&AttributeValue::UInt8(0))
                            .clone(),
                    },
                    DeviceAttribute {
                        id: 3,
                        description: "Level".to_string(),
                        attribute_type: AttributeType::UInt8,
                        supports_write: true,
                        supports_read: true,
                        current_value: self
                            .attr_values
                            .get(&(master_id, 3 as AttributeId))
                            .unwrap_or(&AttributeValue::UInt8(0))
                            .clone(),
                        setting_value: self
                            .attr_values
                            .get(&(master_id, 3 as AttributeId))
                            .unwrap_or(&AttributeValue::UInt8(0))
                            .clone(),
                    },
                    DeviceAttribute {
                        id: 4,
                        description: "Up_Down".to_string(),
                        attribute_type: AttributeType::Bool,
                        supports_write: true,
                        supports_read: false,
                        current_value: AttributeValue::NoValue,
                        setting_value: AttributeValue::NoValue,
                    },
                    DeviceAttribute {
                        id: 5,
                        description: "StopMovement".to_string(),
                        attribute_type: AttributeType::Bool,
                        supports_write: true,
                        supports_read: false,
                        current_value: AttributeValue::NoValue,
                        setting_value: AttributeValue::NoValue,
                    },
                ],
            }),

            _ => bail!("Device id {} not found", master_id),
        }
    }

    fn set(
        &mut self,
        master_id: u32,
        attribute_id: u32,
        value: &AttributeValue,
    ) -> Result<(), Box<dyn Error>> {
        if master_id != 2
            || attribute_id < 1
            || attribute_id > 5
            || *value == AttributeValue::NoValue
        {
            bail!("Invalid inputs: {}/{}", master_id, attribute_id)
        }
        self.attr_values
            .insert((master_id, attribute_id), value.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_LIST_STRING: &str = r###"
Found 2 devices in database...
MASTERID |     INTERCONNECT |                         USERNAME
       2 |            ZWAVE |                      Bedroom Fan
       4 |            ZWAVE |                   Bedroom Lights

Found 0 master groups in database...
GROUP ID |             NAME |            RADIO |

Found 0 control groups in database...
GROUP ID |             NAME |            RADIO |
"###;

    #[test]
    fn list() {
        let controller = AprontestController {
            runner: |_| Ok(TEST_LIST_STRING.to_string()),
        };

        assert_eq!(
            vec![
                ShortDevice {
                    id: 2,
                    name: "Bedroom Fan".to_string()
                },
                ShortDevice {
                    id: 4,
                    name: "Bedroom Lights".to_string()
                }
            ],
            controller.list().unwrap()
        )
    }

    const TEST_DESCRIBE_STRING: &str = r###"
Gang ID: 0x00000003
Generic/Specific device types: 0x11/0x08
Manufacturer ID: 0x0063 Product Type: 0x4944 Product Number: 0x3131
Device is ONLINE, 0 failed tx attempts, 6 seconds since last msg rx'ed, polling period 10 seconds
Device has 4 attributes...
Bedroom Fan
   ATTRIBUTE |                         DESCRIPTION |   TYPE | MODE |                              GET |                              SET
           1 |                        GenericValue |  UINT8 |  R/W |                                0 |                                0
           3 |                               Level |  UINT8 |  R/W |                                0 |                                0
           4 |                             Up_Down |   BOOL |    W |                                  |
           5 |                        StopMovement |   BOOL |    W |                                  |
"###;

    #[test]
    fn describe() {
        let controller = AprontestController {
            runner: |_| Ok(TEST_DESCRIBE_STRING.to_string()),
        };

        assert_eq!(
            LongDevice {
                gang_id: Some(0x03),
                generic_device_type: Some(0x11),
                specific_device_type: Some(0x08),
                manufacturer_id: Some(0x63),
                product_type: Some(0x4944),
                product_number: Some(0x3131),
                id: 2,
                status: "ONLINE".to_string(),
                name: "Bedroom Fan".to_string(),
                attributes: vec![
                    DeviceAttribute {
                        id: 1,
                        description: "GenericValue".to_string(),
                        attribute_type: AttributeType::UInt8,
                        supports_write: true,
                        supports_read: true,
                        current_value: AttributeValue::UInt8(0),
                        setting_value: AttributeValue::UInt8(0),
                    },
                    DeviceAttribute {
                        id: 3,
                        description: "Level".to_string(),
                        attribute_type: AttributeType::UInt8,
                        supports_write: true,
                        supports_read: true,
                        current_value: AttributeValue::UInt8(0),
                        setting_value: AttributeValue::UInt8(0),
                    },
                    DeviceAttribute {
                        id: 4,
                        description: "Up_Down".to_string(),
                        attribute_type: AttributeType::Bool,
                        supports_write: true,
                        supports_read: false,
                        current_value: AttributeValue::NoValue,
                        setting_value: AttributeValue::NoValue,
                    },
                    DeviceAttribute {
                        id: 5,
                        description: "StopMovement".to_string(),
                        attribute_type: AttributeType::Bool,
                        supports_write: true,
                        supports_read: false,
                        current_value: AttributeValue::NoValue,
                        setting_value: AttributeValue::NoValue,
                    }
                ]
            },
            controller.describe(2).unwrap()
        )
    }

    const TEST_OLD_LIST_STRING: &str = r###"
Found 4 devices in database...
MASTERID |     INTERCONNECT |                         USERNAME
       1 |           ZIGBEE |                         LV_Lamp1
       2 |           ZIGBEE |                         LV_Lamp2
       3 |           ZIGBEE |                      Fireplace-L
       4 |           ZIGBEE |                      Fireplace-R
"###;

    #[test]
    fn older_list() {
        let controller = AprontestController {
            runner: |_| Ok(TEST_OLD_LIST_STRING.to_string()),
        };

        assert_eq!(
            vec![
                ShortDevice {
                    id: 1,
                    name: "LV_Lamp1".to_string()
                },
                ShortDevice {
                    id: 2,
                    name: "LV_Lamp2".to_string()
                },
                ShortDevice {
                    id: 3,
                    name: "Fireplace-L".to_string()
                },
                ShortDevice {
                    id: 4,
                    name: "Fireplace-R".to_string()
                }
            ],
            controller.list().unwrap()
        )
    }

    const TEST_OLD_DESCRIBE_STRING: &str = r###"
Device has 2 attributes...
LV_Lamp1
ATTRIBUTE |               DESCRIPTION |   TYPE | MODE |          GET |     SET
        1 |                    On_Off | STRING |  R/W |           ON |      ON
        2 |                     Level |  UINT8 |  R/W |            0 |       0
"###;

    #[test]
    fn old_describe() {
        let controller = AprontestController {
            runner: |_| Ok(TEST_OLD_DESCRIBE_STRING.to_string()),
        };

        assert_eq!(
            LongDevice {
                gang_id: None,
                generic_device_type: None,
                specific_device_type: None,
                manufacturer_id: None,
                product_type: None,
                product_number: None,
                id: 2,
                status: "".to_string(),
                name: "LV_Lamp1".to_string(),
                attributes: vec![
                    DeviceAttribute {
                        id: 1,
                        description: "On_Off".to_string(),
                        attribute_type: AttributeType::String,
                        supports_write: true,
                        supports_read: true,
                        current_value: AttributeValue::String("ON".to_string()),
                        setting_value: AttributeValue::String("ON".to_string()),
                    },
                    DeviceAttribute {
                        id: 2,
                        description: "Level".to_string(),
                        attribute_type: AttributeType::UInt8,
                        supports_write: true,
                        supports_read: true,
                        current_value: AttributeValue::UInt8(0),
                        setting_value: AttributeValue::UInt8(0),
                    },
                ]
            },
            controller.describe(2).unwrap()
        )
    }

    const OTHER_TYPES_DESCRIBE: &str = r###"
Gang ID: 0x7ce8f9f9
Manufacturer ID: 0x10dc, Product Number: 0xdfbf
Device is ONLINE, 0 failed tx attempts, 4 seconds since last msg rx'ed, polling period 0 seconds
Device has 14 attributes...
New HA Dimmable Light
   ATTRIBUTE |                         DESCRIPTION |   TYPE | MODE |                              GET |                              SET
           1 |                              On_Off | STRING |  R/W |                              OFF |                              OFF
           2 |                               Level |  UINT8 |  R/W |                              254 |
           4 |                         NameSupport |  UINT8 |    R |                                0 |
       61440 |                          ZCLVersion |  UINT8 |    R |                                1 |
       61441 |                  ApplicationVersion |  UINT8 |    R |                                2 |
       61442 |                        StackVersion |  UINT8 |    R |                                2 |
       61443 |                           HWVersion |  UINT8 |    R |                                1 |
       61444 |                    ManufacturerName | STRING |    R |                               GE |
       61445 |                     ModelIdentifier | STRING |    R |                        SoftWhite |
       61446 |                            DateCode | STRING |    R |                         20150515 |
       61447 |                         PowerSource |  UINT8 |    R |                                1 |
      258048 |                        IdentifyTime | UINT16 |  R/W |                                0 |
     1699842 |               ZB_CurrentFileVersion | UINT32 |    R |                         33554952 |
  4294901760 |                   WK_TransitionTime | UINT16 |  R/W |                                  |
    "###;

    #[test]
    fn types_describe() {
        let controller = AprontestController {
            runner: |_| Ok(OTHER_TYPES_DESCRIBE.to_string()),
        };

        let result = controller.describe(2).unwrap();
        assert_eq!(14, result.attributes.len());
        assert_eq!(
            AttributeType::UInt32,
            result.attributes[result.attributes.len() - 2].attribute_type
        );
        assert_eq!(
            AttributeValue::UInt32(33554952),
            result.attributes[result.attributes.len() - 2].current_value
        );
    }

    #[tokio::test]
    async fn test_json_serialization() {
        let tests = [
            AttributeValue::String("hi".into()),
            AttributeValue::String("true".into()),
            AttributeValue::String("false".into()),
            AttributeValue::String("0".into()),
            AttributeValue::String("".into()),
            AttributeValue::Bool(true),
            AttributeValue::Bool(false),
            AttributeValue::UInt8(u8::max_value()),
            AttributeValue::UInt16(u16::max_value()),
            AttributeValue::UInt32(u32::max_value()),
        ];

        for test in tests.iter() {
            let atype = test.attribute_type().unwrap();
            let json_output = test.to_json();
            assert_eq!(test, &atype.parse_json(&json_output).unwrap());
            assert_eq!(
                test,
                &atype
                    .parse(
                        &json_output
                            .as_str()
                            .map(String::from)
                            .unwrap_or_else(|| json_output.to_string())
                    )
                    .unwrap()
            );
        }

        assert_eq!(serde_json::Value::Null, AttributeValue::NoValue.to_json());
    }
}
