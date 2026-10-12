//! OS tray / menu-bar rendering. Runs on the main thread (required on macOS); the Windows
//! message pump is tao's event loop. All logic lives in [`crate::menu_model`]; this module only
//! turns a [`MenuModel`] into native menus and forwards clicks.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use muda::{IsMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::client::{self, ClientConfig, Command};
use crate::menu_model::{self, Action, Item, MenuModel, NodeKey, Op};
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

/// A rendered row, kept so later models can be applied in place.
enum Node {
    Item(MenuItem),
    Separator(PredefinedMenuItem),
    Submenu(Submenu, Vec<Node>),
}

impl Node {
    fn as_item(&self) -> &dyn IsMenuItem {
        match self {
            Node::Item(i) => i,
            Node::Separator(s) => s,
            Node::Submenu(s, _) => s,
        }
    }
}

/// The two kinds of container muda rows live in.
enum Container<'a> {
    Root(&'a Menu),
    Sub(&'a Submenu),
}

impl Container<'_> {
    fn insert(&self, item: &dyn IsMenuItem, at: usize) {
        let _ = match self {
            Container::Root(m) => m.insert(item, at),
            Container::Sub(s) => s.insert(item, at),
        };
    }
    fn remove_at(&self, at: usize) {
        match self {
            Container::Root(m) => drop(m.remove_at(at)),
            Container::Sub(s) => drop(s.remove_at(at)),
        }
    }
}

fn create(item: &Item, used: &mut HashSet<String>) -> Node {
    match item {
        Item::Label(text) => Node::Item(MenuItem::new(text, false, None)),
        Item::Separator => Node::Separator(PredefinedMenuItem::separator()),
        Item::Action {
            text,
            action,
            enabled,
        } => {
            let base = menu_model::action_id(action);
            let mut id = base.clone();
            let mut n = 2;
            while used.contains(&id) {
                id = format!("{base}#{n}");
                n += 1;
            }
            used.insert(id.clone());
            Node::Item(MenuItem::with_id(MenuId::new(id), text, *enabled, None))
        }
        Item::Submenu { key, text, items } => {
            let sub = Submenu::with_id(MenuId::new(format!("sub:{key}")), text, true);
            let mut children = Vec::new();
            sync(&Container::Sub(&sub), &mut children, items, used);
            Node::Submenu(sub, children)
        }
    }
}

fn update(node: &mut Node, item: &Item, used: &mut HashSet<String>) {
    match (node, item) {
        (Node::Item(m), Item::Label(text)) => {
            m.set_text(text);
            m.set_enabled(false);
        }
        (Node::Item(m), Item::Action { text, enabled, .. }) => {
            used.insert(m.id().0.clone());
            m.set_text(text);
            m.set_enabled(*enabled);
        }
        (Node::Submenu(sub, children), Item::Submenu { text, items, .. }) => {
            sub.set_text(text);
            sync(&Container::Sub(sub), children, items, used);
        }
        _ => {} // separators have nothing to update; keys guarantee kinds match
    }
}

/// Make `nodes` (rendered into `container`) match `items`, mutating rows in place where their
/// identity is unchanged. The root menu is never replaced, so an open macOS menu stays open
/// (muda cancels menu tracking when a menu is dropped).
fn sync(container: &Container, nodes: &mut Vec<Node>, items: &[Item], used: &mut HashSet<String>) {
    let old: Vec<NodeKey> = nodes.iter().map(node_key).collect();
    let new: Vec<NodeKey> = items.iter().map(menu_model::key_of).collect();
    for op in menu_model::plan(&old, &new) {
        match op {
            Op::Update { at } => update(&mut nodes[at], &items[at], used),
            Op::Insert { at } => {
                let node = create(&items[at], used);
                container.insert(node.as_item(), at);
                nodes.insert(at, node);
            }
            Op::Remove { at } => {
                container.remove_at(at);
                nodes.remove(at);
            }
        }
    }
}

fn node_key(n: &Node) -> NodeKey {
    match n {
        Node::Separator(_) => NodeKey::Separator,
        Node::Item(m) => {
            let id = &m.id().0;
            match menu_model::action_from_id(id) {
                Some(a) => NodeKey::Action(menu_model::action_id(&a)),
                None => NodeKey::Label,
            }
        }
        Node::Submenu(s, _) => NodeKey::Submenu(submenu_key(s)),
    }
}

/// Submenus are created with their model key as the muda id.
fn submenu_key(s: &Submenu) -> &'static str {
    match s.id().0.as_str() {
        "sub:readiness" => "readiness",
        "sub:this_machine" => "this_machine",
        _ => "",
    }
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
    let (cmds, _ipc_thread) = client::spawn(cfg, move |s: &AppState| {
        let _ = proxy.send_event(UserEvent::State(Box::new(s.clone())));
    });

    let mut tray: Option<TrayIcon> = None;
    let mut state = AppState::default();
    let mut model: Option<MenuModel> = None;
    // The one root menu, kept for the process lifetime and updated in place.
    let root = Menu::new();
    let mut nodes: Vec<Node> = Vec::new();
    let refresh_every = Duration::from_secs(30);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + refresh_every);
        let mut rebuild = false;
        match event {
            Event::NewEvents(StartCause::Init) => {
                // macOS requires the tray to be created once the event loop is running.
                let first = menu_model::build(&state, client::unix_now());
                sync(
                    &Container::Root(&root),
                    &mut nodes,
                    &first.items,
                    &mut HashSet::new(),
                );
                let mut b = TrayIconBuilder::new()
                    .with_menu(Box::new(root.clone()))
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
            Event::UserEvent(UserEvent::Menu(e)) => match menu_model::action_from_id(&e.id().0) {
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
                    // Don't join the IPC thread here: it may sit in a bounded connect/hello/write
                    // wait, and the menu bar must not freeze. It holds no state worth flushing;
                    // process exit ends it.
                    let _ = cmds.send(Command::Shutdown);
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
                    sync(
                        &Container::Root(&root),
                        &mut nodes,
                        &new_model.items,
                        &mut HashSet::new(),
                    );
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
