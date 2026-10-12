//! OS tray / menu-bar rendering. Runs on the main thread (required on macOS); the Windows
//! message pump is tao's event loop. All logic lives in [`crate::menu_model`]; this module only
//! turns a [`MenuModel`] into native menus and forwards clicks.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use muda::{IsMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::client::{self, ClientConfig, Command};
use crate::menu_model::{self, Action, Item, MenuModel};
use crate::state::AppState;

enum UserEvent {
    State(Box<AppState>),
    Menu(MenuEvent),
}

/// Draw the tray icon: a ring, filled in the middle while a session is inbound.
pub fn icon_rgba(size: u32, active: bool, rgb: [u8; 3]) -> Vec<u8> {
    let mut px = vec![0u8; (size * size * 4) as usize];
    let c = (size as f32 - 1.0) / 2.0;
    let outer = size as f32 * 0.46;
    let inner = size as f32 * 0.32;
    let dot = size as f32 * 0.18;
    for y in 0..size {
        for x in 0..size {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
            let on = (d <= outer && d >= inner) || (active && d <= dot);
            if on {
                let i = ((y * size + x) * 4) as usize;
                px[i..i + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
        }
    }
    px
}

fn make_icon(active: bool) -> Option<Icon> {
    // macOS: black template image (the system tints it). Windows: a visible accent colour.
    let rgb = if cfg!(target_os = "macos") {
        [0, 0, 0]
    } else if active {
        [0x2e, 0xa0, 0x43]
    } else {
        [0x3b, 0x82, 0xf6]
    };
    let size = 32;
    Icon::from_rgba(icon_rgba(size, active, rgb), size, size).ok()
}

fn render_items(
    items: &[Item],
    actions: &mut HashMap<MenuId, Action>,
    next: &mut u32,
) -> Vec<Box<dyn IsMenuItem>> {
    let mut out: Vec<Box<dyn IsMenuItem>> = Vec::new();
    for item in items {
        match item {
            Item::Label(text) => out.push(Box::new(MenuItem::new(text, false, None))),
            Item::Separator => out.push(Box::new(PredefinedMenuItem::separator())),
            Item::Action {
                text,
                action,
                enabled,
            } => {
                let id = MenuId::new(format!("rs-{next}"));
                *next += 1;
                actions.insert(id.clone(), action.clone());
                out.push(Box::new(MenuItem::with_id(id, text, *enabled, None)));
            }
            Item::Submenu { text, items } => {
                let sub = Submenu::new(text, true);
                for child in render_items(items, actions, next) {
                    let _ = sub.append(child.as_ref());
                }
                out.push(Box::new(sub));
            }
        }
    }
    out
}

fn render(model: &MenuModel) -> (Menu, HashMap<MenuId, Action>) {
    let menu = Menu::new();
    let mut actions = HashMap::new();
    let mut next = 0;
    for item in render_items(&model.items, &mut actions, &mut next) {
        let _ = menu.append(item.as_ref());
    }
    (menu, actions)
}

/// Run the tray until Quit. Never returns.
pub fn run(cfg: ClientConfig) -> ! {
    #[allow(unused_mut)]
    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    #[cfg(target_os = "macos")]
    {
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
    }

    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |e| {
        let _ = proxy.send_event(UserEvent::Menu(e));
    }));
    let proxy = event_loop.create_proxy();
    let (cmds, ipc_thread) = client::spawn(cfg, move |s: &AppState| {
        let _ = proxy.send_event(UserEvent::State(Box::new(s.clone())));
    });
    let mut ipc_thread = Some(ipc_thread);

    let mut tray: Option<TrayIcon> = None;
    let mut state = AppState::default();
    let mut model: Option<MenuModel> = None;
    let mut actions: HashMap<MenuId, Action> = HashMap::new();
    let refresh_every = Duration::from_secs(30);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + refresh_every);
        let mut rebuild = false;
        match event {
            Event::NewEvents(StartCause::Init) => {
                // macOS requires the tray to be created once the event loop is running.
                let (m, a) = render(&menu_model::build(&state, client::unix_now()));
                actions = a;
                let mut b = TrayIconBuilder::new()
                    .with_menu(Box::new(m))
                    .with_tooltip("RemoteSmith");
                if let Some(icon) = make_icon(false) {
                    #[cfg(target_os = "macos")]
                    {
                        b = b.with_icon_templated(icon);
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        b = b.with_icon(icon);
                    }
                }
                match b.build() {
                    Ok(t) => tray = Some(t),
                    Err(e) => log::error!("cannot create tray icon: {e}"),
                }
                rebuild = true;
            }
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => rebuild = true,
            Event::UserEvent(UserEvent::State(s)) => {
                state = *s;
                rebuild = true;
            }
            Event::UserEvent(UserEvent::Menu(e)) => match actions.get(e.id()).cloned() {
                Some(Action::Connect(peer)) => {
                    log::info!("connect requested from tray: {peer}");
                    let _ = cmds.send(Command::Connect(peer));
                }
                Some(Action::OpenApp) => {
                    if let Err(e) = crate::platform::open_app() {
                        log::warn!("cannot open RemoteSmith: {e}");
                    }
                }
                Some(Action::Quit) => {
                    log::info!("quit from tray");
                    let _ = cmds.send(Command::Shutdown);
                    if let Some(h) = ipc_thread.take() {
                        let _ = h.join();
                    }
                    tray.take();
                    *control_flow = ControlFlow::Exit;
                    return;
                }
                None => {}
            },
            _ => {}
        }
        if rebuild {
            let new_model = menu_model::build(&state, client::unix_now());
            if model.as_ref() != Some(&new_model) {
                if let Some(t) = &tray {
                    let (m, a) = render(&new_model);
                    t.set_menu(Some(Box::new(m)));
                    actions = a;
                    let _ = t.set_tooltip(Some(&new_model.tooltip));
                    if model.as_ref().map(|m| m.active) != Some(new_model.active) {
                        #[cfg(target_os = "macos")]
                        let _ = t.set_icon_templated(make_icon(new_model.active));
                        #[cfg(not(target_os = "macos"))]
                        let _ = t.set_icon(make_icon(new_model.active));
                    }
                    #[cfg(target_os = "macos")]
                    t.set_title(new_model.badge.as_deref());
                }
                model = Some(new_model);
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_shapes() {
        let idle = icon_rgba(32, false, [0, 0, 0]);
        let active = icon_rgba(32, true, [0, 0, 0]);
        assert_eq!(idle.len(), 32 * 32 * 4);
        let alpha = |v: &[u8], x: u32, y: u32| v[((y * 32 + x) * 4 + 3) as usize];
        assert_eq!(alpha(&idle, 16, 16), 0, "idle centre is hollow");
        assert_eq!(alpha(&active, 16, 16), 255, "active centre is filled");
        assert_eq!(alpha(&idle, 0, 0), 0);
        assert_eq!(alpha(&idle, 16, 3), 255, "ring present");
    }
}
