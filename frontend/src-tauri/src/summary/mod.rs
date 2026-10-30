//! Summary commands; the summary engine lives in `tingskriver_core::summary`.

pub use tingskriver_core::summary::*;

pub mod commands;
pub mod summary_engine;
pub mod template_commands;

// Re-export Tauri commands (with their generated __cmd__ variants)
pub use commands::{
    __cmd__api_cancel_summary, __cmd__api_detect_transcript_summary_language,
    __cmd__api_get_meeting_detected_summary_language, __cmd__api_get_meeting_summary_language,
    __cmd__api_get_summary, __cmd__api_process_transcript,
    __cmd__api_save_meeting_detected_summary_language, __cmd__api_save_meeting_summary,
    __cmd__api_save_meeting_summary_language, __tauri_command_name_api_cancel_summary,
    __tauri_command_name_api_detect_transcript_summary_language,
    __tauri_command_name_api_get_meeting_detected_summary_language,
    __tauri_command_name_api_get_meeting_summary_language,
    __tauri_command_name_api_get_summary, __tauri_command_name_api_process_transcript,
    __tauri_command_name_api_save_meeting_detected_summary_language,
    __tauri_command_name_api_save_meeting_summary,
    __tauri_command_name_api_save_meeting_summary_language, api_cancel_summary,
    api_detect_transcript_summary_language, api_get_meeting_detected_summary_language,
    api_get_meeting_summary_language, api_get_summary, api_process_transcript,
    api_save_meeting_detected_summary_language, api_save_meeting_summary,
    api_save_meeting_summary_language,
};

// Re-export template commands
pub use template_commands::{
    __cmd__api_get_template_details, __cmd__api_list_templates, __cmd__api_validate_template,
    __tauri_command_name_api_get_template_details, __tauri_command_name_api_list_templates,
    __tauri_command_name_api_validate_template,
    api_get_template_details, api_list_templates, api_validate_template,
};
