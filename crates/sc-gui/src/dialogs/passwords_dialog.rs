//! The saved passwords' master password (sc_core's secrets): set or changed
//! in Settings › Passwords, typed to unlock them on another computer.

use std::rc::Rc;
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, WindowExt, h_flex, v_flex};
use gpui_kit::*;
use sc_core::remote::secrets;

/// What the master password is asked for.
#[derive(Clone, Copy, PartialEq)]
pub enum Ask {
    /// To open the saved passwords here.
    Unlock,
    /// A new one, set or changed.
    Set,
}

type Done = Rc<dyn Fn(&mut Window, &mut App)>;

/// The note that the saved passwords are locked, gone once they are open.
struct Locked;

/// Soon after the start: saved passwords locked here (the settings came from
/// another computer) are offered to unlock.
pub fn init(cx: &mut App) {
    cx.spawn(async move |cx| {
        cx.background_executor().timer(Duration::from_secs(2)).await;
        // The keyring may wait for its wallet to open.
        if cx.background_spawn(async { secrets::locked() }).await {
            cx.update(offer_unlock);
        }
    })
    .detach();
}

fn offer_unlock(cx: &mut App) {
    let note = Notification::info("The saved passwords are locked on this computer: the master password opens them.").id::<Locked>().autohide(false).action(|_, _, _| {
        Button::new("unlock-passwords").primary().label("Unlock…").on_click(|_, window, cx| ask(Ask::Unlock, window, cx, |_, _| {}))
    });
    crate::notify_main(note, cx);
}

/// Asks for the master password (a new one twice) and unlocks or sets it in
/// the background; `done` hears once that has worked.
pub fn ask(what: Ask, window: &mut Window, cx: &mut App, done: impl Fn(&mut Window, &mut App) + 'static) {
    let mut masked = |cx: &mut App| cx.new(|cx| InputState::new(window, cx).masked(true));
    let first = masked(cx);
    let again = (what == Ask::Set).then(|| masked(cx));
    let view = cx.new(|_| MasterDialog { what, first: first.clone(), again, problem: None, working: false, done: Rc::new(done) });
    let title = match what {
        Ask::Unlock => "Unlock Saved Passwords",
        Ask::Set => "Master Password",
    };
    window.open_dialog(cx, move |dialog, _, _| {
        let ok = view.clone();
        dialog
            .title(title)
            .width(px(480.))
            .overlay_closable(false)
            .child(view.clone())
            .on_ok(move |_, window, cx| {
                ok.update(cx, |this, cx| this.finish(window, cx));
                false
            })
    });
    first.update(cx, |input, cx| input.focus(window, cx));
}

struct MasterDialog {
    what: Ask,
    first: Entity<InputState>,
    /// A new one's repeat.
    again: Option<Entity<InputState>>,
    problem: Option<String>,
    working: bool,
    done: Done,
}

impl MasterDialog {
    fn finish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.working {
            return;
        }
        let master = self.first.read(cx).value().to_string();
        let again = self.again.as_ref().map(|again| again.read(cx).value().to_string());
        self.problem = match () {
            _ if master.is_empty() => Some("Type the master password.".into()),
            _ if again.is_some_and(|again| again != master) => Some("The two are not the same.".into()),
            _ => None,
        };
        if self.problem.is_some() {
            return cx.notify();
        }
        self.working = true;
        cx.notify();
        let what = self.what;
        // Slow on purpose (PBKDF2), and the keyring may wait.
        let work = cx.background_spawn(async move {
            match what {
                Ask::Unlock => secrets::unlock(&master),
                Ask::Set => secrets::set_master(Some(&master)),
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let worked = work.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.working = false;
                match worked {
                    Ok(()) => {
                        window.close_dialog(cx);
                        let text = match this.what {
                            Ask::Unlock => "The saved passwords are open.",
                            Ask::Set => "The master password is set.",
                        };
                        window.remove_notification::<Locked>(cx);
                        window.push_notification(Notification::success(text), cx);
                        (this.done)(window, cx);
                    }
                    Err(err) => {
                        let text = err.to_string();
                        let mut chars = text.trim_end_matches('.').chars();
                        this.problem = chars.next().map(|first| format!("{}{}.", first.to_uppercase(), chars.as_str()));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }
}

impl Render for MasterDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let about = match self.what {
            Ask::Unlock => "The settings came from another computer, or this one keeps no keyring: the master password opens the saved passwords here.",
            Ask::Set => {
                "With a master password, the saved passwords open on another computer once it is typed there, the settings folder copied. \
                 Forgotten, only they are lost: they are typed again."
            }
        };
        let field = |label: &'static str, input: &Entity<InputState>| h_flex().gap_3().child(div().w(px(130.)).flex_none().child(label)).child(div().flex_1().child(Input::new(input)));
        let theme = cx.theme();
        v_flex()
            .w_full()
            .gap_3()
            .text_sm()
            .child(div().whitespace_normal().text_color(theme.muted_foreground).child(about))
            .child(field("Master password", &self.first))
            .children(self.again.as_ref().map(|again| field("Again", again)))
            .children(self.problem.clone().map(|problem| div().whitespace_normal().text_color(theme.danger).child(problem)))
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("master-ok")
                            .primary()
                            .label(if self.working { "Working…" } else { "OK" })
                            .min_w(px(80.))
                            .on_click(cx.listener(|this, _, window, cx| this.finish(window, cx))),
                    )
                    .child(Button::new("master-cancel").outline().label("Cancel").min_w(px(80.)).on_click(|_, window, cx| window.close_dialog(cx))),
            )
    }
}
