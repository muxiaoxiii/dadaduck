use tauri::menu::{Menu, MenuItem, MenuItemKind};
use tauri::{AppHandle, Emitter, Wry};

const ABOUT_ID: &str = "about-dadaduck";

pub fn build(app: &AppHandle<Wry>) -> tauri::Result<Menu<Wry>> {
    let menu = Menu::default(app)?;
    #[cfg(target_os = "macos")]
    let target = menu.items()?.into_iter().next();
    #[cfg(not(target_os = "macos"))]
    let target = menu.get(tauri::menu::HELP_SUBMENU_ID);
    if let Some(MenuItemKind::Submenu(submenu)) = target {
        #[cfg(not(target_os = "macos"))]
        submenu.set_text("帮助")?;
        if !submenu.items()?.is_empty() {
            submenu.remove_at(0)?;
        }
        let about = MenuItem::with_id(app, ABOUT_ID, "关于小达鸭…", true, None::<&str>)?;
        submenu.prepend(&about)?;
    }
    Ok(menu)
}

pub fn handle(app: &AppHandle<Wry>, event: tauri::menu::MenuEvent) {
    if event.id().as_ref() == ABOUT_ID {
        let _ = app.emit_to("main", "open-about", ());
    }
}
