use tauri::menu::{Menu, MenuItem, MenuItemKind};
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use tauri_plugin_opener::OpenerExt;

const ABOUT_ID: &str = "about-dadaduck";
const AUTHOR_GITHUB: &str = env!("CARGO_PKG_HOMEPAGE");

pub fn build(app: &AppHandle<Wry>) -> tauri::Result<Menu<Wry>> {
    // Keep native Edit, Window, Services and their platform shortcuts.
    let menu = Menu::default(app)?;
    #[cfg(target_os = "macos")]
    let target = menu.items()?.into_iter().next();
    #[cfg(not(target_os = "macos"))]
    let target = menu.get(tauri::menu::HELP_SUBMENU_ID);
    if let Some(MenuItemKind::Submenu(submenu)) = target {
        if !submenu.items()?.is_empty() {
            submenu.remove_at(0)?;
        }
        let about = MenuItem::with_id(app, ABOUT_ID, "关于小达鸭…", true, None::<&str>)?;
        submenu.prepend(&about)?;
    }
    Ok(menu)
}

pub fn handle(app: &AppHandle<Wry>, event: tauri::menu::MenuEvent) {
    if event.id().as_ref() != ABOUT_ID {
        return;
    }
    let handle = app.clone();
    let text = format!(
        "小达鸭 DadaDuck\n版本 {}\n\n作者  {}\n{}",
        app.package_info().version, env!("CARGO_PKG_AUTHORS"), AUTHOR_GITHUB
    );
    let dialog = app.dialog()
        .message(text)
        .title("关于小达鸭")
        .buttons(MessageDialogButtons::OkCancelCustom("作者 GitHub".into(), "关闭".into()));
    let dialog = match app.get_webview_window("main") {
        Some(window) => dialog.parent(&window),
        None => dialog,
    };
    dialog.show(move |open_profile| {
            if open_profile {
                if let Err(error) = handle.opener().open_url(AUTHOR_GITHUB, None::<&str>) {
                    handle.dialog().message(error.to_string()).title("无法打开 GitHub").show(|_| {});
                }
            }
        });
}
