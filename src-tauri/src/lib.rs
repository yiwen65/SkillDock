mod agents;
mod config;
mod git_ops;
mod links;
mod model;
mod scanner;
mod tasks;
mod workspace;

pub use agents::*;
pub use config::*;
pub use git_ops::*;
pub use links::*;
pub use model::*;
pub use scanner::*;
pub use tasks::*;
pub use workspace::*;

pub fn health_check() -> Workspace {
    Workspace::placeholder()
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn health_check_command() -> Workspace {
    health_check()
}

#[cfg(feature = "desktop")]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            set_app_handle(app.handle().clone());
            #[cfg(target_os = "linux")]
            {
                use tauri::Manager;
                if let Some(window) = app.get_webview_window("main") {
                    let icon = tauri::include_image!("icons/128x128.png");
                    let _ = window.set_icon(icon);
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            health_check_command,
            load_workspace_config_command,
            save_workspace_config_command,
            load_user_config_command,
            save_user_config_command,
            patch_user_preferences_command,
            list_agent_profile_states_command,
            save_agent_profiles_command,
            create_agent_profile_dir_command,
            default_install_targets_command,
            select_workspace_command,
            restore_recent_workspace_command,
            scan_workspace_command,
            read_skill_markdown_preview_command,
            open_workspace_path_command,
            get_task_status_command,
            get_task_logs_command,
            recent_task_records_command,
            cancel_task_command,
            import_project_command,
            delete_project_command,
            check_project_updates_command,
            check_all_project_updates_command,
            pull_project_command,
            pull_all_projects_command,
            preview_link_skill_command,
            link_skill_command,
            preview_link_skills_batch_command,
            link_skills_batch_command,
            preview_unlink_skill_command,
            unlink_skill_command,
            preview_unlink_skills_batch_command,
            unlink_skills_batch_command
        ])
        .run(tauri::generate_context!())
        .expect("error while running SkillDock app");
}
