// Copyright 2022 Mandiant, Inc. All Rights Reserved
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with the License. You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed under the License
// is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and limitations under the License.

use log::{debug, error};
use nom::number::complete::{be_u128, le_u16};
use nom::Needed;


#[derive(Clone, Copy)]
pub struct FirehoseFlags(u16);

impl std::fmt::Debug for FirehoseFlags {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:X}", self.0)
    }
}

impl From<u16> for FirehoseFlags {
    fn from(value: u16) -> Self {
        FirehoseFlags(value)
    }
}

impl FirehoseFlags {
    // has_current_aid flag
    const ACTIVITY_ID_CURRENT: u16 = 0x1;
    // has_private_data flag
    const PRIVATE_STRING_RANGE: u16 = 0x100;
    // has_subsystem flag. In Signpost log entries this is the subsystem flag

    /// message strings UUID flag
    const MESSAGE_STRINGS_UUID: u16 = 0x2;

    const SUBSYSTEM: u16 = 0x200;
    /// has_rules flag
    const HAS_RULES: u16 = 0x400;
    /// has_oversize flag
    const DATA_REF: u16 = 0x800;
    const HAS_NAME: u16 = 0x8000;

    pub fn has_current_aid(&self) -> bool {
        self.has_flag(Self::ACTIVITY_ID_CURRENT)
    }
    pub fn has_private_string(&self) -> bool {
        self.has_flag(Self::PRIVATE_STRING_RANGE)
    }
    pub fn has_message_strings_uuid(&self) -> bool {
        self.has_flag(Self::MESSAGE_STRINGS_UUID)
    }
    pub fn has_subsystem(&self) -> bool {
        self.has_flag(Self::SUBSYSTEM)
    }
    pub fn has_rules(&self) -> bool {
        self.has_flag(Self::HAS_RULES)
    }
    pub fn has_data_ref(&self) -> bool {
        self.has_flag(Self::DATA_REF)
    }
    pub fn has_name(&self) -> bool {
        self.has_flag(Self::HAS_NAME)
    }


    pub fn has_flag(&self, flag_mask: u16) -> bool {
        (self.0 & flag_mask) != 0
    }

    /// Get only sub flags
    const FLAGS_CHECK: u16 = 0xe;

    /// large_shared_cache flag - Offset to format string is larger than normal
    const LARGE_SHARED_CACHE: u16 = 0xc;
    /// has_large_offset flag - Offset to format string is larger than normal
    const LARGE_OFFSET: u16 = 0x20;
    ///  absolute flag - The log uses an alterantive index number that points to the UUID file name in the Catalog which contains the format string
    const ABSOLUTE: u16 = 0x8;
    /// main_exe flag. A UUID file contains the format string
    const MAIN_EXE: u16 = 0x2;
    /// shared_cache flag. DSC file contains the format string
    const SHARED_CACHE: u16 = 0x4;
    /// uuid_relative flag. The UUID file name is in the log data (instead of the Catalog)
    const UUID_RELATIVE: u16 = 0xa;

    pub fn is_large_offset(&self) -> bool {
        self.flags() == Self::LARGE_OFFSET
    }
    pub fn has_large_offset(&self) -> bool {
        self.has_flag(Self::LARGE_OFFSET)
    }
    pub fn is_large_shared_cache(&self) -> bool {
        self.flags() == Self::LARGE_SHARED_CACHE
    }
    pub fn has_large_shared_cache(&self) -> bool {
        self.has_flag(Self::LARGE_SHARED_CACHE)
    }
    pub fn is_absolute(&self) -> bool {
        self.flags() == Self::ABSOLUTE
    }
    pub fn has_absolute(&self) -> bool {
        self.has_flag(Self::ABSOLUTE)
    }
    pub fn is_main_exe(&self) -> bool {
        self.flags() == Self::MAIN_EXE
    }
    pub fn has_main_exe(&self) -> bool {
        self.has_flag(Self::MAIN_EXE)
    }
    pub fn is_shared_cache(&self) -> bool {
        self.flags() == Self::SHARED_CACHE
    }
    pub fn has_shared_cache(&self) -> bool {
        self.has_flag(Self::SHARED_CACHE)
    }
    pub fn is_uuid_relative(&self) -> bool {
        self.flags() == Self::UUID_RELATIVE
    }
    pub fn has_uuid_relative(&self) -> bool {
        self.has_flag(Self::UUID_RELATIVE)
    }

    pub fn flags(&self) -> u16 {
        self.0 & Self::FLAGS_CHECK
    }
}


#[derive(Debug, Clone, Default)]
pub struct FirehoseFormatters {
    pub main_exe: bool,
    pub shared_cache: bool,
    pub has_large_offset: u16,
    pub large_shared_cache: u16,
    pub absolute: bool,
    pub uuid_relative: String,
    /// Not seen yet
    pub main_plugin: bool,
    /// Not seen yet
    pub pc_style: bool,
    /// If log entry uses an alternative uuid file index (ex: absolute). This value gets prepended to the unknown_pc_id/offset
    pub main_exe_alt_index: u16,
}

impl FirehoseFormatters {
    /// Identify formatter flags associated with the log entry. Formatter flags determine the file where the base format string is located
    pub fn firehose_formatter_flags<'a>(
        mut input: &'a [u8],
        flags: impl Into<FirehoseFlags>,
    ) -> nom::IResult<&'a [u8], FirehoseFormatters> {
        let mut formatter_flags = FirehoseFormatters::default();

        let flags = flags.into();

        if flags.is_large_offset() {
            debug!("[macos-unifiedlogs] Firehose flag: has_large_offset");
            let (firehose_input, has_large_offset) = le_u16(input)?;
            formatter_flags.has_large_offset = has_large_offset;
            input = firehose_input;

            if flags.has_large_shared_cache() {
                debug!(
                    "[macos-unifiedlogs] Firehose flag: large_shared_cache and has_large_offset"
                );
                let (firehose_input, large_shared_cache) = le_u16(input)?;
                formatter_flags.large_shared_cache = large_shared_cache;
                input = firehose_input;
            }
        } else if flags.is_large_shared_cache() {
            debug!("[macos-unifiedlogs] Firehose flag: large_shared_cache");
            if flags.has_large_offset() {
                let (firehose_input, has_large_offset) = le_u16(input)?;
                formatter_flags.has_large_offset = has_large_offset;
                input = firehose_input;
            }

            let (firehose_input, large_shared_cache) = le_u16(input)?;
            formatter_flags.large_shared_cache = large_shared_cache;
            input = firehose_input;
        } else if flags.is_absolute() {
            debug!("[macos-unifiedlogs] Firehose flag: absolute");
            formatter_flags.absolute = true;
            if !flags.has_message_strings_uuid() {
                debug!("[macos-unifiedlogs] Firehose flag: alt index absolute flag");
                let (firehose_input, main_exe_alt_index) = le_u16(input)?;
                formatter_flags.main_exe_alt_index = main_exe_alt_index;
                input = firehose_input;
            }
        } else if flags.is_main_exe() {
            debug!("[macos-unifiedlogs] Firehose flag: main_exe");
            formatter_flags.main_exe = true
        } else if flags.is_shared_cache() {
            debug!("[macos-unifiedlogs] Firehose flag: shared_cache");
            formatter_flags.shared_cache = true;
            if flags.has_large_offset() {
                let (firehose_input, has_large_offset) = le_u16(input)?;
                formatter_flags.has_large_offset = has_large_offset;
                input = firehose_input;
            }
        } else if flags.is_uuid_relative() {
            debug!("[macos-unifiedlogs] Firehose flag: uuid_relative");
            let (firehose_input, uuid_relative) = be_u128(input)?;
            formatter_flags.uuid_relative = format!("{:X}", uuid_relative);
            input = firehose_input;
        } else {
            error!("[macos-unifiedlogs] Unknown Firehose formatter flag: {flags:?}",);
            debug!("[macos-unifiedlogs] Firehose data: {:X?}", input);
            return Err(nom::Err::Incomplete(Needed::Unknown));
        }

        Ok((input, formatter_flags))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_firehose_formatter_flags_has_large_offset() {
        let test_data = [
            1, 0, 2, 0, 14, 0, 34, 2, 0, 4, 135, 16, 0, 0, 34, 4, 0, 0, 5, 0, 100, 101, 110, 121, 0,
        ];
        let test_flags = 557;
        let (_, results) =
            FirehoseFormatters::firehose_formatter_flags(&test_data, test_flags).unwrap();
        assert_eq!(results.has_large_offset, 1);
        assert_eq!(results.large_shared_cache, 2);
    }

    #[test]
    fn test_firehose_formatter_flags_message_strings_uuid_message_alt_index() {
        let test_data = [8, 0, 17, 166, 251, 2, 128, 255, 0, 0];
        let test_flags = 8;
        let (_, results) =
            FirehoseFormatters::firehose_formatter_flags(&test_data, test_flags).unwrap();
        assert_eq!(results.main_exe_alt_index, 8)
    }

    #[test]
    fn test_firehose_formatter_flags_message_strings_uuid() {
        let test_data = [186, 0, 0, 0];
        let test_flags = 514;
        let (_, results) =
            FirehoseFormatters::firehose_formatter_flags(&test_data, test_flags).unwrap();
        assert!(results.main_exe);
    }

    #[test]
    fn test_firehose_formatter_flags_shared_cache_dsc_uuid() {
        let test_data = [
            23, 1, 34, 1, 66, 4, 0, 0, 35, 0, 83, 65, 83, 83, 101, 115, 115, 105, 111, 110, 83,
            116, 97, 116, 101, 70, 111, 114, 85, 115, 101, 114, 58, 49, 50, 52, 54, 58, 32, 101,
            110, 116, 101, 114, 0,
        ];
        let test_flags = 516;
        let (_, results) =
            FirehoseFormatters::firehose_formatter_flags(&test_data, test_flags).unwrap();
        assert!(results.shared_cache);
    }

    #[test]
    fn test_firehose_formatter_flags_absolute_message_alt_uuid() {
        let test_data = [
            128, 255, 2, 13, 34, 4, 0, 0, 6, 0, 34, 4, 6, 0, 11, 0, 34, 4, 17, 0, 7, 0, 2, 4, 8, 0,
            0, 0, 2, 8, 0, 0, 0, 0, 0, 0, 0, 0, 2, 4, 0, 0, 0, 0, 2, 8, 0, 0, 0, 0, 0, 0, 0, 0, 34,
            4, 24, 0, 3, 0, 34, 4, 27, 0, 3, 0, 2, 8, 156, 17, 7, 98, 0, 0, 0, 0, 2, 8, 156, 17, 7,
            98, 0, 0, 0, 0, 2, 4, 0, 0, 0, 0, 34, 4, 30, 0, 3, 0, 65, 67, 77, 82, 77, 0, 95, 108,
            111, 103, 80, 111, 108, 105, 99, 121, 0, 83, 65, 86, 73, 78, 71, 0, 78, 79, 0, 78, 79,
            0, 78, 79, 0,
        ];
        let test_flags = 8;
        let (_, results) =
            FirehoseFormatters::firehose_formatter_flags(&test_data, test_flags).unwrap();
        assert!(results.absolute);
        assert_eq!(results.main_exe_alt_index, 65408);
    }

    #[test]
    fn test_firehose_formatter_flags_uuid_relative() {
        let test_data = [
            123, 13, 55, 117, 241, 144, 62, 33, 186, 19, 4, 71, 196, 27, 135, 67, 0, 0,
        ];
        let test_flags = 0xa;
        let (_, results) =
            FirehoseFormatters::firehose_formatter_flags(&test_data, test_flags).unwrap();
        assert_eq!(results.uuid_relative, "7B0D3775F1903E21BA130447C41B8743");
    }
}
