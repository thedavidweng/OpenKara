use serde::Deserialize;
use tauri::{
    menu::{AboutMetadata, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu},
    AppHandle, Emitter, Runtime,
};

#[cfg(target_os = "macos")]
use tauri::{menu::MenuItemKind, Manager};

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const MENU_ACTION_EVENT: &str = "openkara://menu-action";
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const MENU_ACTION_IMPORT_FILES: &str = "import-files";
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const MENU_ACTION_OPEN_SETTINGS: &str = "open-settings";
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const MENU_ACTION_SWITCH_LIBRARY: &str = "switch-library";
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const MENU_ACTION_TOGGLE_SIDEBAR: &str = "toggle-sidebar";
pub const MENU_ACTION_COPY_DEBUG_INFO: &str = "copy-debug-info";

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MENU_ITEM_IMPORT_FILES: &str = "file.import";
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MENU_ITEM_OPEN_SETTINGS: &str = "app.settings";
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MENU_ITEM_SWITCH_LIBRARY: &str = "app.switch-library";
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MENU_ITEM_TOGGLE_SIDEBAR: &str = "view.toggle-sidebar";
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MENU_ITEM_COPY_DEBUG_INFO: &str = "help.copy-debug-info";
const MENU_LABEL_FILE: &str = "File";
const MENU_LABEL_EDIT: &str = "Edit";
const MENU_LABEL_WINDOW: &str = "Window";
const MENU_LABEL_HELP: &str = "Help";
const MENU_LABEL_IMPORT: &str = "Import";
const MENU_LABEL_COPY_DEBUG_INFO: &str = "Copy debug info";
const MENU_LABEL_SWITCH_LIBRARY: &str = "Switch Library…";
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MENU_LABEL_SETTINGS: &str = "Settings";
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MENU_LABEL_VIEW: &str = "View";
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MENU_LABEL_TOGGLE_SIDEBAR: &str = "Toggle Sidebar";
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MENU_SUBMENU_APP: &str = "app.menu";
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MENU_SUBMENU_FILE: &str = "file.menu";
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MENU_SUBMENU_EDIT: &str = "edit.menu";
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MENU_SUBMENU_VIEW: &str = "view.menu";
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MENU_SUBMENU_WINDOW: &str = "window.menu";
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MENU_SUBMENU_HELP: &str = "help.menu";
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const ABOUT_AUTHOR_CREDIT: &str = "@David Weng";
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const ABOUT_REPOSITORY_URL: &str = "https://github.com/thedavidweng/OpenKara";

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeAppMenuLabels {
    pub file: String,
    pub edit: String,
    pub view: String,
    pub window: String,
    pub help: String,
    pub import: String,
    pub settings: String,
    pub switch_library: String,
    pub toggle_sidebar: String,
    pub copy_debug_info: String,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn build_about_metadata<R: Runtime>(app_handle: &AppHandle<R>) -> AboutMetadata<'static> {
    let pkg_info = app_handle.package_info();
    let config = app_handle.config();
    let build_hash = option_env!("GIT_BUILD_HASH").unwrap_or("unknown");
    let version_with_hash = format!("{} ({})", pkg_info.version, build_hash);

    AboutMetadata {
        name: Some(pkg_info.name.clone()),
        version: Some(version_with_hash),
        copyright: config.bundle.copyright.clone(),
        authors: Some(vec!["@David Weng".to_owned()]),
        credits: Some(ABOUT_AUTHOR_CREDIT.to_owned()),
        website: Some(ABOUT_REPOSITORY_URL.to_owned()),
        ..Default::default()
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn build_app_menu<R: Runtime>(app_handle: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    #[cfg(target_os = "macos")]
    let about_metadata = build_about_metadata(app_handle);
    #[cfg(target_os = "macos")]
    let app_menu_label = app_handle.package_info().name.as_str();

    let import_item = MenuItem::with_id(
        app_handle,
        MENU_ITEM_IMPORT_FILES,
        MENU_LABEL_IMPORT,
        true,
        Some("CmdOrCtrl+O"),
    )?;

    let window_menu = Submenu::with_id_and_items(
        app_handle,
        MENU_SUBMENU_WINDOW,
        MENU_LABEL_WINDOW,
        true,
        &[
            &PredefinedMenuItem::minimize(app_handle, None)?,
            &PredefinedMenuItem::maximize(app_handle, None)?,
            #[cfg(target_os = "macos")]
            &PredefinedMenuItem::separator(app_handle)?,
            &PredefinedMenuItem::close_window(app_handle, None)?,
        ],
    )?;

    let copy_debug_info_item = MenuItem::with_id(
        app_handle,
        MENU_ITEM_COPY_DEBUG_INFO,
        MENU_LABEL_COPY_DEBUG_INFO,
        true,
        None::<&str>,
    )?;

    let help_menu = Submenu::with_id_and_items(
        app_handle,
        MENU_SUBMENU_HELP,
        MENU_LABEL_HELP,
        true,
        &[
            &copy_debug_info_item,
            #[cfg(target_os = "macos")]
            &PredefinedMenuItem::separator(app_handle)?,
        ],
    )?;

    let menu = Menu::with_items(
        app_handle,
        &[
            #[cfg(target_os = "macos")]
            &Submenu::with_id_and_items(
                app_handle,
                MENU_SUBMENU_APP,
                app_menu_label,
                true,
                &[
                    &PredefinedMenuItem::about(app_handle, None, Some(about_metadata.clone()))?,
                    &PredefinedMenuItem::separator(app_handle)?,
                    &MenuItem::with_id(
                        app_handle,
                        MENU_ITEM_OPEN_SETTINGS,
                        MENU_LABEL_SETTINGS,
                        true,
                        Some("CmdOrCtrl+,"),
                    )?,
                    &MenuItem::with_id(
                        app_handle,
                        MENU_ITEM_SWITCH_LIBRARY,
                        MENU_LABEL_SWITCH_LIBRARY,
                        true,
                        None::<&str>,
                    )?,
                    &PredefinedMenuItem::separator(app_handle)?,
                    &PredefinedMenuItem::services(app_handle, None)?,
                    &PredefinedMenuItem::separator(app_handle)?,
                    &PredefinedMenuItem::hide(app_handle, None)?,
                    &PredefinedMenuItem::hide_others(app_handle, None)?,
                    &PredefinedMenuItem::separator(app_handle)?,
                    &PredefinedMenuItem::quit(app_handle, None)?,
                ],
            )?,
            &Submenu::with_id_and_items(
                app_handle,
                MENU_SUBMENU_FILE,
                MENU_LABEL_FILE,
                true,
                &[
                    &import_item,
                    &PredefinedMenuItem::separator(app_handle)?,
                    #[cfg(not(target_os = "macos"))]
                    &MenuItem::with_id(
                        app_handle,
                        MENU_ITEM_SWITCH_LIBRARY,
                        MENU_LABEL_SWITCH_LIBRARY,
                        true,
                        None::<&str>,
                    )?,
                    #[cfg(not(target_os = "macos"))]
                    &PredefinedMenuItem::separator(app_handle)?,
                    &PredefinedMenuItem::close_window(app_handle, None)?,
                    #[cfg(not(target_os = "macos"))]
                    &PredefinedMenuItem::quit(app_handle, None)?,
                ],
            )?,
            &Submenu::with_id_and_items(
                app_handle,
                MENU_SUBMENU_EDIT,
                MENU_LABEL_EDIT,
                true,
                &[
                    &PredefinedMenuItem::undo(app_handle, None)?,
                    &PredefinedMenuItem::redo(app_handle, None)?,
                    &PredefinedMenuItem::separator(app_handle)?,
                    &PredefinedMenuItem::cut(app_handle, None)?,
                    &PredefinedMenuItem::copy(app_handle, None)?,
                    &PredefinedMenuItem::paste(app_handle, None)?,
                    &PredefinedMenuItem::select_all(app_handle, None)?,
                ],
            )?,
            #[cfg(target_os = "macos")]
            &Submenu::with_id_and_items(
                app_handle,
                MENU_SUBMENU_VIEW,
                MENU_LABEL_VIEW,
                true,
                &[
                    &MenuItem::with_id(
                        app_handle,
                        MENU_ITEM_TOGGLE_SIDEBAR,
                        MENU_LABEL_TOGGLE_SIDEBAR,
                        true,
                        Some("CmdOrCtrl+B"),
                    )?,
                    &PredefinedMenuItem::separator(app_handle)?,
                    &PredefinedMenuItem::fullscreen(app_handle, None)?,
                ],
            )?,
            &window_menu,
            &help_menu,
        ],
    )?;

    Ok(menu)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn handle_menu_event<R: Runtime>(app_handle: &AppHandle<R>, event: MenuEvent) {
    match event.id().as_ref() {
        MENU_ITEM_IMPORT_FILES => {
            let _ = app_handle.emit_to("main", MENU_ACTION_EVENT, MENU_ACTION_IMPORT_FILES);
        }
        MENU_ITEM_OPEN_SETTINGS => {
            let _ = app_handle.emit_to("main", MENU_ACTION_EVENT, MENU_ACTION_OPEN_SETTINGS);
        }
        MENU_ITEM_SWITCH_LIBRARY => {
            let _ = app_handle.emit_to("main", MENU_ACTION_EVENT, MENU_ACTION_SWITCH_LIBRARY);
        }
        MENU_ITEM_TOGGLE_SIDEBAR => {
            let _ = app_handle.emit_to("main", MENU_ACTION_EVENT, MENU_ACTION_TOGGLE_SIDEBAR);
        }
        MENU_ITEM_COPY_DEBUG_INFO => {
            let _ = app_handle.emit_to("main", MENU_ACTION_EVENT, MENU_ACTION_COPY_DEBUG_INFO);
        }
        _ => {}
    }
}

#[cfg(target_os = "macos")]
fn set_menu_item_text<R: Runtime>(item: MenuItemKind<R>, text: &str) -> tauri::Result<()> {
    match item {
        MenuItemKind::MenuItem(item) => item.set_text(text),
        MenuItemKind::Submenu(item) => item.set_text(text),
        MenuItemKind::Predefined(item) => item.set_text(text),
        MenuItemKind::Check(item) => item.set_text(text),
        MenuItemKind::Icon(item) => item.set_text(text),
    }
}

#[cfg(target_os = "macos")]
fn set_root_menu_item_text<R: Runtime>(menu: &Menu<R>, id: &str, text: &str) -> tauri::Result<()> {
    if let Some(item) = menu.get(id) {
        set_menu_item_text(item, text)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn set_submenu_item_text<R: Runtime>(
    menu: &Menu<R>,
    submenu_id: &str,
    item_id: &str,
    text: &str,
) -> tauri::Result<()> {
    if let Some(MenuItemKind::Submenu(submenu)) = menu.get(submenu_id) {
        if let Some(item) = submenu.get(item_id) {
            set_menu_item_text(item, text)?;
        }
    }
    Ok(())
}

#[tauri::command]
pub fn set_native_app_menu_labels(
    app_handle: AppHandle,
    labels: NativeAppMenuLabels,
) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    if let Some(menu) = app_handle
        .get_webview_window("main")
        .and_then(|window| window.menu())
    {
        set_root_menu_item_text(&menu, MENU_SUBMENU_FILE, &labels.file)?;
        set_root_menu_item_text(&menu, MENU_SUBMENU_EDIT, &labels.edit)?;
        set_root_menu_item_text(&menu, MENU_SUBMENU_VIEW, &labels.view)?;
        set_root_menu_item_text(&menu, MENU_SUBMENU_WINDOW, &labels.window)?;
        set_root_menu_item_text(&menu, MENU_SUBMENU_HELP, &labels.help)?;
        set_submenu_item_text(
            &menu,
            MENU_SUBMENU_APP,
            MENU_ITEM_OPEN_SETTINGS,
            &labels.settings,
        )?;
        set_submenu_item_text(
            &menu,
            MENU_SUBMENU_APP,
            MENU_ITEM_SWITCH_LIBRARY,
            &labels.switch_library,
        )?;
        set_submenu_item_text(
            &menu,
            MENU_SUBMENU_FILE,
            MENU_ITEM_IMPORT_FILES,
            &labels.import,
        )?;
        set_submenu_item_text(
            &menu,
            MENU_SUBMENU_FILE,
            MENU_ITEM_SWITCH_LIBRARY,
            &labels.switch_library,
        )?;
        set_submenu_item_text(
            &menu,
            MENU_SUBMENU_VIEW,
            MENU_ITEM_TOGGLE_SIDEBAR,
            &labels.toggle_sidebar,
        )?;
        set_submenu_item_text(
            &menu,
            MENU_SUBMENU_HELP,
            MENU_ITEM_COPY_DEBUG_INFO,
            &labels.copy_debug_info,
        )?;
    }

    #[cfg(not(target_os = "macos"))]
    let _ = (app_handle, labels);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_action_event_name_is_stable() {
        assert_eq!(MENU_ACTION_EVENT, "openkara://menu-action");
    }

    #[test]
    fn menu_actions_match_frontend_runtime_contract() {
        assert_eq!(MENU_ACTION_IMPORT_FILES, "import-files");
        assert_eq!(MENU_ACTION_OPEN_SETTINGS, "open-settings");
        assert_eq!(MENU_ACTION_SWITCH_LIBRARY, "switch-library");
        assert_eq!(MENU_ACTION_TOGGLE_SIDEBAR, "toggle-sidebar");
    }

    #[test]
    fn mac_about_credit_is_stable() {
        assert_eq!(ABOUT_AUTHOR_CREDIT, "@David Weng");
    }

    #[test]
    fn mac_about_repository_link_is_stable() {
        assert_eq!(
            ABOUT_REPOSITORY_URL,
            "https://github.com/thedavidweng/OpenKara"
        );
    }
}
