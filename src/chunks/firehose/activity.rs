// Copyright 2022 Mandiant, Inc. All Rights Reserved
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with the License. You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed under the License
// is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and limitations under the License.

use crate::{
    catalog::CatalogChunk,
    chunks::firehose::{flags::FirehoseFormatters, message::MessageData},
    dsc::SharedCacheStrings,
    uuidtext::UUIDText,
};
use log::{debug, error};
use nom::{
    number::complete::{le_u32, le_u64},
    sequence::tuple,
    Needed,
};

#[derive(Debug, Clone, Default)]
pub struct FirehoseActivity {
    pub unknown_activity_id: u32,
    /// always 0x80000000?
    pub unknown_sentinal: u32,
    /// if flag 0x0010
    pub pid: u64,
    /// if flag 0x0001
    pub unknown_activity_id_2: u32,
    /// always 0x80000000? only if flag 0x0001
    pub unknown_sentinal_2: u32,
    /// if flag 0x0200
    pub unknown_activity_id_3: u32,
    /// always 0x80000000? only if flag 0x0200
    pub unknown_sentinal_3: u32,
    pub unknown_message_string_ref: u32,
    /// Appears to be used to calculate string offset for firehose events with Absolute flag
    pub unknown_pc_id: u32,
    pub firehose_formatters: FirehoseFormatters,
}

impl FirehoseActivity {
    /// Parse Activity Type Firehose log entry.
    //  Ex: tp 3536 + 60: activity create (has_current_aid, has_unique_pid, shared_cache, has_other_aid)
    pub fn parse_activity<'a>(
        mut input: &'a [u8],
        firehose_flags: u16,
        firehose_log_type: u8,
    ) -> nom::IResult<&'a [u8], FirehoseActivity> {
        let mut activity = FirehoseActivity::default();

        // Useraction activity type does not have first Activity ID or sentinel
        const USERACTION: u8 = 0x3;
        // Get first activity_id (if not useraction type)
        if firehose_log_type != USERACTION {
            let (firehose_input, (unknown_activity_id, unknown_sentinal)) =
                tuple((le_u32, le_u32))(input)?;
            activity.unknown_activity_id = unknown_activity_id;
            activity.unknown_sentinal = unknown_sentinal;
            input = firehose_input;
        }

        const UNIQUE_PID: u16 = 0x10; // has_unique_pid flag
        if (firehose_flags & UNIQUE_PID) != 0 {
            debug!("[macos-unifiedlogs] Activity Firehose log chunk has unique_pid flag");
            let (firehose_input, pid) = le_u64(input)?;
            activity.pid = pid;
            input = firehose_input;
        }

        const ACTIVITY_ID_CURRENT: u16 = 0x1; // has_current_aid flag
        if (firehose_flags & ACTIVITY_ID_CURRENT) != 0 {
            debug!("[macos-unifiedlogs] Activity Firehose log chunk has has_current_aid flag");
            let (firehose_input, (unknown_activity_id_2, unknown_sentinal_2)) =
                tuple((le_u32, le_u32))(input)?;
            activity.unknown_activity_id_2 = unknown_activity_id_2;
            activity.unknown_sentinal_2 = unknown_sentinal_2;
            input = firehose_input;
        }

        const ACTIVITY_ID_OTHER: u16 = 0x200; // has_other_current_aid flag. In Activity log entries this is another activity id flag
        if (firehose_flags & ACTIVITY_ID_OTHER) != 0 {
            debug!(
                "[macos-unifiedlogs] Activity Firehose log chunk has has_other_current_aid flag"
            );
            let (firehose_input, (unknown_activity_id_3, unknown_sentinal_3)) =
                tuple((le_u32, le_u32))(input)?;

            activity.unknown_activity_id_3 = unknown_activity_id_3;
            activity.unknown_sentinal_3 = unknown_sentinal_3;
            input = firehose_input;
        }

        let (input, firehose_unknown_pc_id) = le_u32(input)?;
        activity.unknown_pc_id = firehose_unknown_pc_id; // Unknown (Message string reference)?? PC ID?

        // Check for flags related to base string format location (shared string file (dsc) or UUID file)
        let (input, formatters) =
            FirehoseFormatters::firehose_formatter_flags(input, firehose_flags)?;
        activity.firehose_formatters = formatters;
        Ok((input, activity))
    }

    /// Get base log message string formatter from shared cache strings (dsc) or UUID text file for firehose activity log entries (chunks)
    pub fn get_firehose_activity_strings<'a>(
        firehose: &FirehoseActivity,
        strings_data: &'a [UUIDText],
        shared_strings: &'a [SharedCacheStrings],
        string_offset: u64,
        first_proc_id: u64,
        second_proc_id: u32,
        catalogs: &CatalogChunk,
    ) -> nom::IResult<&'a [u8], MessageData> {
        if firehose.firehose_formatters.shared_cache
            || (firehose.firehose_formatters.large_shared_cache != 0
                && firehose.firehose_formatters.has_large_offset != 0)
        {
            if firehose.firehose_formatters.has_large_offset != 0 {
                let mut large_offset = firehose.firehose_formatters.has_large_offset;
                let extra_offset_value;
                // large_shared_cache should be double the value of has_large_offset
                // Ex: has_large_offset = 1, large_shared_cache = 2
                // If the value do not match then there is an issue with shared string offset
                // Can recover by using large_shared_cache
                // Apple records this as an error: "error: ~~> <Invalid shared cache code pointer offset>"
                //   But is still able to get string formatter
                if large_offset != firehose.firehose_formatters.large_shared_cache / 2
                    && !firehose.firehose_formatters.shared_cache
                {
                    large_offset = firehose.firehose_formatters.large_shared_cache / 2;
                    // Combine large offset value with current string offset to get the true offset
                    extra_offset_value = format!("{:X}{:08X}", large_offset, string_offset);
                } else if firehose.firehose_formatters.shared_cache {
                    // Large offset is 8 if shared_cache flag is set
                    large_offset = 8;
                    let add_offset = 0x10000000 * u64::from(large_offset);
                    extra_offset_value = format!("{:X}", add_offset + string_offset);
                } else {
                    extra_offset_value = format!("{:X}{:08X}", large_offset, string_offset);
                }

                let extra_offset_value_result = u64::from_str_radix(&extra_offset_value, 16);
                match extra_offset_value_result {
                    Ok(offset) => {
                        return MessageData::extract_shared_strings(
                            shared_strings,
                            strings_data,
                            offset,
                            first_proc_id,
                            second_proc_id,
                            catalogs,
                            string_offset,
                        );
                    }
                    Err(err) => {
                        // We should not get errors since we are combining two numbers to create the offset
                        error!(
                            "Failed to get shared string offset to format string for activity firehose entry: {:?}",
                            err
                        );
                        return Err(nom::Err::Incomplete(Needed::Unknown));
                    }
                }
            }
            MessageData::extract_shared_strings(
                shared_strings,
                strings_data,
                string_offset,
                first_proc_id,
                second_proc_id,
                catalogs,
                string_offset,
            )
        } else {
            if firehose.firehose_formatters.absolute {
                let extra_offset_value = format!(
                    "{:X}{:08X}",
                    firehose.firehose_formatters.main_exe_alt_index, firehose.unknown_pc_id,
                );
                let offset_result = u64::from_str_radix(&extra_offset_value, 16);
                match offset_result {
                    Ok(offset) => {
                        return MessageData::extract_absolute_strings(
                            strings_data,
                            offset,
                            string_offset,
                            first_proc_id,
                            second_proc_id,
                            catalogs,
                            string_offset,
                        );
                    }
                    Err(err) => {
                        // We should not get errors since we are combining two numbers to create the offset
                        error!("Failed to get absolute offset to format string for activity firehose entry: {:?}", err);
                        return Err(nom::Err::Incomplete(Needed::Unknown));
                    }
                }
            }
            if !firehose.firehose_formatters.uuid_relative.is_empty() {
                return MessageData::extract_alt_uuid_strings(
                    strings_data,
                    string_offset,
                    &firehose.firehose_formatters.uuid_relative,
                    first_proc_id,
                    second_proc_id,
                    catalogs,
                    string_offset,
                );
            }
            MessageData::extract_format_strings(
                strings_data,
                string_offset,
                first_proc_id,
                second_proc_id,
                catalogs,
                string_offset,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunks::firehose::firehose_log::FirehoseItem;
    use crate::filesystem::LogarchiveProvider;
    use crate::parser::{collect_shared_strings, collect_strings, parse_log};
    use std::path::PathBuf;

    #[test]
    fn test_parse_activity() {
        let test_data = [
            178, 251, 0, 0, 0, 0, 0, 128, 236, 0, 0, 0, 0, 0, 0, 0, 178, 251, 0, 0, 0, 0, 0, 128,
            179, 251, 0, 0, 0, 0, 0, 128, 64, 63, 24, 18, 1, 0, 2, 0,
        ];
        let test_flags = 573;
        let log_type: u8 = 0x1;
        let (_, results) =
            FirehoseActivity::parse_activity(&test_data, test_flags, log_type).unwrap();
        assert_eq!(results.unknown_activity_id, 64434);
        assert_eq!(results.unknown_sentinal, 2147483648);
        assert_eq!(results.pid, 236);
        assert_eq!(results.unknown_activity_id_2, 64434);
        assert_eq!(results.unknown_sentinal_2, 2147483648);
        assert_eq!(results.unknown_activity_id_3, 64435);
        assert_eq!(results.unknown_sentinal_3, 2147483648);
        assert_eq!(results.unknown_message_string_ref, 0);
        assert!(!results.firehose_formatters.main_exe);
        assert!(!results.firehose_formatters.absolute);
        assert!(!results.firehose_formatters.shared_cache);
        assert!(!results.firehose_formatters.main_plugin);
        assert!(!results.firehose_formatters.pc_style);
        assert_eq!(results.firehose_formatters.main_exe_alt_index, 0);
        assert_eq!(results.firehose_formatters.uuid_relative, "");
        assert_eq!(results.unknown_pc_id, 303578944);
        assert_eq!(results.firehose_formatters.has_large_offset, 1);
        assert_eq!(results.firehose_formatters.large_shared_cache, 2);
    }

    #[test]
    fn test_get_firehose_activity_big_sur() {
        let mut test_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        test_path.push("tests/test_data/system_logs_big_sur.logarchive");
        let provider = LogarchiveProvider::new(test_path.as_path());
        let string_results = collect_strings(&provider).unwrap();

        test_path.push("dsc");
        let shared_strings_results = collect_shared_strings(&provider).unwrap();
        test_path.pop();

        test_path.push("Persist/0000000000000004.tracev3");
        let handle = std::fs::File::open(test_path).unwrap();
        let log_data = parse_log(handle).unwrap();

        for catalog_data in log_data.catalog_data {
            for preamble in catalog_data.firehose {
                for firehose in preamble.public_data {
                    if let FirehoseItem::Acticity(activity) = firehose.item {
                        let (_, message_data) = FirehoseActivity::get_firehose_activity_strings(
                            &activity,
                            &string_results,
                            &shared_strings_results,
                            u64::from(firehose.format_string_location),
                            preamble.first_number_proc_id,
                            preamble.second_number_proc_id,
                            &catalog_data.catalog,
                        )
                        .unwrap();
                        assert_eq!(
                            message_data.format_string,
                            "Internal: Check the state of a node"
                        );
                        assert_eq!(message_data.library, "/usr/libexec/opendirectoryd");
                        assert_eq!(message_data.process, "/usr/libexec/opendirectoryd");
                        assert_eq!(
                            message_data.process_uuid,
                            "B736DF1625F538248E9527A8CEC4991E"
                        );
                        assert_eq!(
                            message_data.library_uuid,
                            "B736DF1625F538248E9527A8CEC4991E"
                        );
                        return;
                    }

                   
                }
            }
        }
    }
}
