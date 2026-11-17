//! A saved server's details, for the sidebar's Connections section.
//! Passwords go to the system keyring only.

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::{IndexPath, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use sc_core::remote::connections::Connection;
use sc_core::remote::{self, Scheme};
use sc_core::remote::secrets;
use sc_core::t;

type Protocols = SelectState<Vec<&'static str>>;

type Saved = Rc<dyn Fn(Connection, &mut Window, &mut App)>;

/// A saved server's details; the password goes to the keyring.
pub(crate) fn open_edit(
    connection: Connection,
    window: &mut Window,
    cx: &mut App,
    done: impl Fn(Connection, &mut Window, &mut App) + 'static,
) {
    let mut input = |value: Option<String>, placeholder: &str, cx: &mut App| {
        let placeholder = placeholder.to_owned();
        cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .default_value(value.unwrap_or_default())
        })
    };
    let name = input(Some(connection.name.clone()), &t!("connections.placeholder.name"), cx);
    let host = input(
        Some(connection.host.clone()),
        &t!("connections.placeholder.server"),
        cx,
    );
    let port = input(connection.port.map(|port| port.to_string()), &t!("connections.placeholder.port"), cx);
    let user = input(
        connection.user.clone(),
        &t!("connections.placeholder.user"),
        cx,
    );
    let path = input(connection.path.clone(), &t!("connections.placeholder.path"), cx);
    let password = cx.new(|cx| {
        InputState::new(window, cx)
            .masked(true)
            .placeholder(t!("connections.placeholder.password"))
    });
    let at = Scheme::ALL.iter().position(|scheme| *scheme == connection.scheme);
    let labels = Scheme::ALL.map(Scheme::label).to_vec();
    let protocol = cx.new(|cx| SelectState::new(labels, at.map(IndexPath::new), window, cx));
    let view = cx.new(|cx| EditConnection {
        scheme: connection.scheme,
        _picked: cx.subscribe_in(&protocol, window, |this: &mut EditConnection, _, event: &SelectEvent<Vec<&'static str>>, window, cx| {
            let SelectEvent::Confirm(Some(label)) = event else { return };
            if let Some(scheme) = Scheme::ALL.into_iter().find(|scheme| scheme.label() == *label) {
                this.scheme = scheme;
                // The service's own sign-in server, unless one is typed.
                let server = match scheme {
                    Scheme::B2 => "api.backblazeb2.com",
                    Scheme::Swift => "auth.api.rackspacecloud.com",
                    _ => "",
                };
                let host = &this.fields[1];
                if !server.is_empty() && host.read(cx).value().trim().is_empty() {
                    host.update(cx, |input, cx| input.set_value(server, window, cx));
                }
                cx.notify();
            }
        }),
        protocol,
        fields: [name.clone(), host, port, user, path],
        password,
        forget: false,
        folder: connection.folder.clone(),
        done: Rc::new(done),
    });
    window.open_dialog(cx, move |dialog, _, _| {
        let enter = view.clone();
        dialog
            .title(t!("connections.dialog.title"))
            .width(px(520.))
            .overlay_closable(false)
            .child(view.clone())
            .on_ok(move |_, window, cx| {
                enter.update(cx, |this, cx| this.finish(window, cx));
                false
            })
    });
    crate::dialogs::pack_dialog::focus_all(&name, window, cx);
}

struct EditConnection {
    scheme: Scheme,
    protocol: Entity<Protocols>,
    _picked: Subscription,
    /// Name, host, port, user, path.
    fields: [Entity<InputState>; 5],
    password: Entity<InputState>,
    forget: bool,
    /// The list's folder it is in, kept.
    folder: Option<String>,
    done: Saved,
}

impl EditConnection {
    fn finish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let [name, host, port, user, path] = self
            .fields
            .each_ref()
            .map(|input| input.read(cx).value().trim().to_string());
        if host.is_empty() {
            return window.push_notification(Notification::error(t!("connections.error.no_server")), cx);
        }
        let port = match port.as_str() {
            "" => None,
            text => match text.parse() {
                Ok(port) => Some(port),
                Err(_) => {
                    return window
                        .push_notification(Notification::error(t!("connections.error.port", text = text)), cx);
                }
            },
        };
        let some = |text: String| (!text.is_empty()).then_some(text);
        let connection = Connection {
            name,
            scheme: self.scheme,
            host,
            port,
            user: some(user),
            path: some(path),
            folder: self.folder.clone(),
        };
        let password = self.password.read(cx).value().to_string();
        if self.scheme.logs_in() && (!password.is_empty() || self.forget) {
            let key = remote::keyring_key(&connection.address());
            let forget = self.forget;
            std::thread::spawn(move || match forget {
                true => secrets::forget(&key),
                false => secrets::remember(&key, &password),
            });
        }
        (self.done)(connection, window, cx);
        window.close_dialog(cx);
    }
}

impl Render for EditConnection {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (labels, secret) = match self.scheme {
            Scheme::S3 => (
                [t!("connections.field.name"), t!("connections.field.endpoint"), t!("connections.field.port"), t!("connections.field.access_key"), t!("connections.field.bucket")],
                t!("connections.field.secret_key"),
            ),
            Scheme::B2 => (
                [t!("connections.field.name"), t!("connections.field.server"), t!("connections.field.port"), t!("connections.field.key_id"), t!("connections.field.bucket")],
                t!("connections.field.application_key"),
            ),
            Scheme::Swift => (
                [t!("connections.field.name"), t!("connections.field.server"), t!("connections.field.port"), t!("connections.field.user"), t!("connections.field.container")],
                t!("connections.field.api_key"),
            ),
            _ => (
                [t!("connections.field.name"), t!("connections.field.server"), t!("connections.field.port"), t!("connections.field.user"), t!("connections.field.start_folder")],
                t!("connections.field.password"),
            ),
        };
        let field = |label: String, input: &Entity<InputState>| {
            h_flex()
                .gap_3()
                .child(div().w(px(96.)).flex_none().child(label))
                .child(div().flex_1().child(Input::new(input)))
        };
        let [name, rest @ ..] = &self.fields;
        let [first, others @ ..] = labels;
        v_flex()
            .w_full()
            .gap_2()
            .text_sm()
            .child(field(first, name))
            .child(
                h_flex()
                    .gap_3()
                    .child(div().w(px(96.)).flex_none().child(t!("connections.field.protocol")))
                    .child(div().flex_1().child(Select::new(&self.protocol))),
            )
            .children(
                others
                    .into_iter()
                    .zip(rest)
                    .map(|(label, input)| field(label, input)),
            )
            .when(self.scheme.logs_in(), |el| {
                el.child(field(secret, &self.password)).child(
                    h_flex().gap_3().child(div().w(px(96.)).flex_none()).child(
                        Checkbox::new("connection-forget")
                            .label(t!("connections.forget_password"))
                            .checked(self.forget)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.forget = *checked;
                                cx.notify();
                            })),
                    ),
                )
            })
            .child(
                h_flex()
                    .pt_2()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("connection-ok")
                            .primary()
                            .label(t!("remote.dialog.ok"))
                            .min_w(px(80.))
                            .on_click(cx.listener(|this, _, window, cx| this.finish(window, cx))),
                    )
                    .child(
                        Button::new("connection-cancel")
                            .outline()
                            .label(t!("remote.dialog.cancel"))
                            .min_w(px(80.))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
    }
}
