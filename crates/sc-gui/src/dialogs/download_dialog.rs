//! Files › Download from Link: an HTTP(S) or FTP(S) link's file into the
//! active panel's folder, this computer's only.

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, WindowExt, h_flex, v_flex};
use gpui_kit::*;
use sc_core::ops::download;

use crate::Commander;

type Done = Rc<dyn Fn(String, Option<String>, &mut Window, &mut App)>;

impl Commander {
    /// Asks for a link, then downloads it into the active panel's folder.
    pub(crate) fn download_link(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let panel = self.panels[self.active].tabs.current();
        if panel.remote().is_some() || panel.in_archive() {
            let text = "Downloads go into this computer's folders: open one in the panel first.";
            return window.push_notification(Notification::warning(text), cx);
        }
        let dir = panel.folder().to_owned();
        let this = cx.entity().downgrade();
        let into = dir.clone();
        open(dir, window, cx, move |link, name, window, cx| {
            let _ = this.update(cx, |this, cx| this.start_download(link, name, into.clone(), window, cx));
        });
    }

    fn start_download(&mut self, link: String, name: Option<String>, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        // A server's password, or its unknown key, is asked in a dialog.
        let (ask, _) = self.dialog_ask(window, cx);
        let limit = self.settings.queue.download_limit_kb.checked_mul(1024);
        self.transfers.queue.run_now(download::download(link.trim().to_owned(), dir, name, limit, ask));
        cx.notify();
    }
}

/// The link (the clipboard's, if it holds one) and the name to save as;
/// `done` gets them once they will do.
fn open(dir: PathBuf, window: &mut Window, cx: &mut App, done: impl Fn(String, Option<String>, &mut Window, &mut App) + 'static) {
    let clipboard = cx.read_from_clipboard().and_then(|item| item.text()).filter(|text| download::is_link(text));
    let link = cx.new(|cx| InputState::new(window, cx).placeholder("https://, http://, ftp:// or ftps://").default_value(clipboard.unwrap_or_default()));
    let name = cx.new(|cx| InputState::new(window, cx).placeholder("as the server or the link names it"));
    let view = cx.new(|cx| {
        // A changed name is asked about again.
        let changed = |cx: &mut Context<DownloadDialog>, input: &Entity<InputState>| {
            cx.subscribe(input, |this: &mut DownloadDialog, _, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    (this.problem, this.replace) = (None, None);
                    cx.notify();
                }
            })
        };
        DownloadDialog {
            _changes: [changed(cx, &link), changed(cx, &name)],
            link: link.clone(),
            name,
            dir,
            problem: None,
            replace: None,
            done: Rc::new(done),
        }
    });
    window.open_dialog(cx, move |dialog, _, _| {
        let ok = view.clone();
        dialog
            .title("Download from Link")
            .width(px(560.))
            .overlay_closable(false)
            .child(view.clone())
            .on_ok(move |_, window, cx| {
                ok.update(cx, |this, cx| this.finish(window, cx));
                false
            })
    });
    crate::dialogs::pack_dialog::focus_all(&link, window, cx);
}

struct DownloadDialog {
    _changes: [Subscription; 2],
    link: Entity<InputState>,
    name: Entity<InputState>,
    dir: PathBuf,
    problem: Option<String>,
    /// The name the user was told is taken: OK again replaces it.
    replace: Option<String>,
    done: Done,
}

impl DownloadDialog {
    fn finish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let link = self.link.read(cx).value().trim().to_string();
        let typed = self.name.read(cx).value().trim().to_string();
        let name = download::safe_name(&typed);
        self.problem = match () {
            _ if link.is_empty() => Some("Paste or type a link.".into()),
            _ if !download::is_link(&link) => Some("Not a link to download: it starts with https://, http://, ftp:// or ftps://.".into()),
            _ if !typed.is_empty() && name.as_deref() != Some(typed.as_str()) => Some("A file's name, without a folder in it.".into()),
            _ if name.as_ref().is_some_and(|name| self.dir.join(name).exists() && self.replace.as_ref() != Some(name)) => {
                self.replace = name.clone();
                Some(format!("There is a {typed} here already: Replace downloads over it."))
            }
            _ => None,
        };
        if self.problem.is_some() {
            return cx.notify();
        }
        (self.done)(link, name, window, cx);
        window.close_dialog(cx);
    }
}

impl Render for DownloadDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let field = |label: &'static str, input: &Entity<InputState>| v_flex().gap_1().child(label).child(Input::new(input));
        let ok = if self.replace.is_some() && self.problem.is_some() { "Replace" } else { "Download" };
        v_flex()
            .w_full()
            .gap_3()
            .text_sm()
            .child(field("Link", &self.link))
            .child(field("Save as", &self.name))
            .child(div().whitespace_normal().text_color(theme.muted_foreground).child(format!("Into {}", self.dir.display())))
            .children(self.problem.clone().map(|problem| div().whitespace_normal().text_color(theme.danger).child(problem)))
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(Button::new("download-ok").primary().label(ok).min_w(px(80.)).on_click(cx.listener(|this, _, window, cx| this.finish(window, cx))))
                    .child(Button::new("download-cancel").outline().label("Cancel").min_w(px(80.)).on_click(|_, window, cx| window.close_dialog(cx))),
            )
    }
}
