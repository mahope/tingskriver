//! Built-in AI model commands; the engine lives in
//! `tingskriver_core::summary::summary_engine`.

pub use tingskriver_core::summary::summary_engine::*;

pub mod commands;

pub use commands::{
    __cmd__builtin_ai_cancel_download, __cmd__builtin_ai_delete_model,
    __cmd__builtin_ai_download_model, __cmd__builtin_ai_get_available_summary_model,
    __cmd__builtin_ai_get_model_info, __cmd__builtin_ai_get_recommended_model, __cmd__builtin_ai_is_model_ready,
    __cmd__builtin_ai_list_models, __tauri_command_name_builtin_ai_cancel_download,
    __tauri_command_name_builtin_ai_delete_model, __tauri_command_name_builtin_ai_download_model,
    __tauri_command_name_builtin_ai_get_available_summary_model,
    __tauri_command_name_builtin_ai_get_model_info,
    __tauri_command_name_builtin_ai_get_recommended_model,
    __tauri_command_name_builtin_ai_is_model_ready, __tauri_command_name_builtin_ai_list_models,
    builtin_ai_cancel_download, builtin_ai_delete_model, builtin_ai_download_model,
    builtin_ai_get_available_summary_model, builtin_ai_get_model_info, builtin_ai_get_recommended_model, builtin_ai_is_model_ready,
    builtin_ai_list_models, init_model_manager, ModelManagerState,
};
