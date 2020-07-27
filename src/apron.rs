use regex::Regex;
use std::error::Error;
use std::num::{ParseIntError};
use simple_error::bail;
use std::convert::TryFrom;
use std::str::FromStr;

type AttributeId = u32;
type DeviceId = u32;
type DeviceStatus = String;

#[derive(Debug, Eq, PartialEq)]
struct ShortDevice {
    id: DeviceId,
    name: String,
}

#[derive(Debug, Eq, PartialEq)]
struct DeviceAttribute {
    id: AttributeId,
    description: String,
    attribute_type: String,
    supports_write: bool,
    supports_read: bool,
    current_value: Option<String>,
    setting_value: Option<String>,
}

#[derive(Debug, Eq, PartialEq)]
struct LongDevice {
    // These probably don't change often
    gang_id: Option<u32>,
    generic_device_type: Option<u8>,
    specific_device_type: Option<u8>,
    manufacturer_id: Option<u16>,
    product_type: Option<u16>,
    product_number: Option<u16>,

    id: DeviceId,
    status: DeviceStatus,
    name: String,
    attributes: Vec<DeviceAttribute>
}

trait DeviceController {
    fn list(&self) -> Result<Vec<ShortDevice>, Box<dyn Error>>;
    fn describe(&self, master_id: DeviceId) -> Result<LongDevice, Box<dyn Error>>;
    fn set(&mut self, master_id: DeviceId, attribute_id: AttributeId, value: &str) -> Result<(), Box<dyn Error>>;
}

struct AprontestController {
     runner: fn(command: String) -> Result<String, Box<dyn Error>>,
}

lazy_static! {
    static ref DEVICE_REGEX_STR: String = r"\s*(?P<id>\d+)\s*\|\s*(?P<interconnect>[^ |]*)\s*\|\s*(?P<name>[^\n]+)".to_owned();
    static ref LIST_REGEX: Regex = Regex::new(&(r"(?ms)^Found \d+ devices in .*MASTERID\s*\|\s*INTERCONNECT\s*\|\s*USERNAME(?P<devices>(?:".to_owned() + &DEVICE_REGEX_STR+ ")*)")).unwrap();
    static ref DEVICE_REGEX : Regex = Regex::new(&DEVICE_REGEX_STR).unwrap();

    static ref ATTRIBUTE_REGEX_STR: String = r"\s*(?P<id>\d+)\s*\|\s*(?P<description>[^\|]+)\s*\|\s*(?P<type>[^ ]+)\s*\|\s*(?P<mode>[^ ]+)\s*\|\s*(?P<get>[^ ]*)\s*\| *(?P<set>[^\n ]*)".to_owned();
    static ref LONG_DEVICE_REGEX : Regex = Regex::new(&((
    "".to_owned() +
    r"(?ms)Gang ID: (?P<gang_id>(0x)?[0-9a-fA-F]+)\n" +
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

impl DeviceController for AprontestController {
    fn list(&self) -> Result<Vec<ShortDevice>, Box<dyn Error>> {
        let stdout = (self.runner)("aprontest -l".to_string())?;
        let devices = match LIST_REGEX.captures(&stdout) {
            Some(v) => v,
            _ => bail!("Output doesn't match regex:\n{}", stdout)
        }.name("devices").unwrap().as_str();

        Ok(DEVICE_REGEX.captures_iter(devices)
            .map(|m| ShortDevice {
                id: m.name("id").unwrap().as_str().parse().unwrap(),
                name: m.name("name").unwrap().as_str().to_string(),
            })
            .collect())
    }

    fn describe(&self, master_id: DeviceId) -> Result<LongDevice, Box<dyn Error>> {
        let stdout = (self.runner)(format!("aprontest -l -m {}", master_id))?;

        let parsed = match LONG_DEVICE_REGEX.captures(&stdout) {
            Some(v) => v,
            _ => bail!("Output does not match regex:\n{}", stdout),
        };

        Ok(LongDevice {
            gang_id: parsed.name("gang_id").map(|v| v.as_str().parse_numberish()).transpose()?,
            generic_device_type: parsed.name("generic_device_type").map(|v| v.as_str().parse_numberish()).transpose()?,
            specific_device_type: parsed.name("specific_device_type").map(|v| v.as_str().parse_numberish()).transpose()?,
            manufacturer_id: parsed.name("manufacturer_id").map(|v| v.as_str().parse_numberish()).transpose()?,
            product_type: parsed.name("product_type").map(|v| v.as_str().parse_numberish()).transpose()?,
            product_number: parsed.name("product_number").map(|v| v.as_str().parse_numberish()).transpose()?,
            id: master_id,
            status: parsed.name("device_status").map_or("", |v| v.as_str()).to_string(),
            name: parsed.name("name").map_or("", |v| v.as_str()).to_string(),
            attributes: ATTRIBUTE_REGEX.captures_iter(parsed.name("attributes").unwrap().as_str())
                .map(|m| -> Result<DeviceAttribute, Box<dyn Error>> {
                    Ok(DeviceAttribute {
                        id: m.name("id").unwrap().as_str().parse()?,
                        description: m.name("description").unwrap().as_str().trim().to_string(),
                        attribute_type: m.name("type").unwrap().as_str().to_string(),
                        supports_write: m.name("mode").unwrap().as_str().contains("W"),
                        supports_read: m.name("mode").unwrap().as_str().contains("R"),
                        current_value: match m.name("get").unwrap().as_str().trim() {
                            "" => None,
                            v => Some(v.to_string()),
                        },
                        setting_value: match m.name("set").unwrap().as_str().trim() {
                            "" => None,
                            v => Some(v.to_string()),
                        }
                    })
                })
                .collect::<Result<Vec<DeviceAttribute>, Box<dyn Error>>>()?
        })
    }

    fn set(&mut self, master_id: DeviceId, attribute_id: AttributeId, value: &str) -> Result<(), Box<dyn Error>> {
        (self.runner)(format!("aprontest -u -m {} -t {} -v {}", master_id, attribute_id, value))?;
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
            runner: |_| Ok(TEST_LIST_STRING.to_string())
        };

        assert_eq!(vec![ShortDevice{ id: 2, name: "Bedroom Fan".to_string() }, ShortDevice{ id: 4, name: "Bedroom Lights".to_string() }], controller.list().unwrap())
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

        assert_eq!(LongDevice {
            gang_id: Some(0x03),
            generic_device_type: Some(0x11),
            specific_device_type: Some(0x08),
            manufacturer_id: Some(0x63),
            product_type: Some(0x4944),
            product_number: Some(0x3131),
            id: 2,
            status: "ONLINE".to_string(),
            name: "Bedroom Fan".to_string(),
            attributes: vec![DeviceAttribute { id: 1,
                description: "GenericValue".to_string(),
                attribute_type: "UINT8".to_string(),
                supports_write: true,
                supports_read: true,
                current_value: Some("0".to_string()),
                setting_value: Some("0".to_string())
            },
                             DeviceAttribute { id: 3,
                                 description: "Level".to_string(),
                                 attribute_type: "UINT8".to_string(),
                                 supports_write: true,
                                 supports_read: true,
                                 current_value: Some("0".to_string()),
                                 setting_value: Some("0".to_string())
                             },
                             DeviceAttribute { id: 4,
                                 description: "Up_Down".to_string(),
                                 attribute_type: "BOOL".to_string(),
                                 supports_write: true,
                                 supports_read: false,
                                 current_value: None,
                                 setting_value: None
                             },
                             DeviceAttribute { id: 5,
                                 description: "StopMovement".to_string(),
                                 attribute_type: "BOOL".to_string(),
                                 supports_write: true,
                                 supports_read: false,
                                 current_value: None,
                                 setting_value: None
                             }]
        }, controller.describe(2).unwrap())
    }
}
