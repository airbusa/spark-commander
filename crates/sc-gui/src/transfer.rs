//! F5 and F6: their dialog, the queue's progress strip, the sidebar's
//! Operations, and the questions a running job asks.
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{MAIN_SEPARATOR, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::progress::Progress as Bar;
use gpui_kit::component::radio::RadioGroup;
use gpui_kit::component::{ActiveTheme, IconName, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use sc_core::Panel;
use sc_core::ops::{Answer, JobView, Kind, Operation, Overwrite, Question, Queue, Status, Target, Task as Job};
use sc_core::vfs::{Fs, Local, Meta};

use crate::Commander;
use crate::panel::drag_drop::DragLabel;
use gpui_kit::component::menu::ContextMenuExt;
use sc_core::sidebar::SidebarTab;

const OVERWRITE: [(Overwrite, &str); 5] = [
    (Overwrite::Ask, "Ask"),
    (Overwrite::All, "Overwrite all"),
    (Overwrite::Skip, "Skip all"),
    (Overwrite::Older, "Overwrite older"),
    (Overwrite::Rename, "Rename the copies"),
];

/// The queue, and what the main window last made of it.
#[derive(Default)]
pub struct Transfers {
    pub queue: Queue,
    version: u64,
    /// Finished jobs whose panels were read again.
    seen: HashSet<u64>,
    /// The job whose question is on screen.
    asked: Option<u64>,
    /// The running job, when it started, and its bytes then: for the speed.
    started: Option<(u64, Instant)>,
}

/// Where files go: a panel's folder, or a folder shown in it.
pub(crate) struct Place {
    pub fs: Arc<dyn Fs>,
    pub folder: PathBuf,
    /// A server's address, for the dialog.
    pub server: Option<String>,
    pub in_archive: bool,
    /// Inside an archive: it and its folder, or why 7-Zip cannot change it.
    pub archive: Option<Result<(PathBuf, PathBuf), String>>,
}

impl Place {
    pub fn of(panel: &Panel) -> Self {
        Place {
            fs: panel.fs(),
            folder: panel.folder().to_owned(),
            server: panel.remote().map(|remote| remote.url()),
            in_archive: panel.in_archive(),
            archive: panel.in_archive().then(|| panel.archive_place().map_err(|err| err.to_string())),
        }
    }
}

/// What goes: a panel's entries, or files from another program.
#[derive(Clone)]
pub(crate) struct Batch {
    pub from: Arc<dyn Fs>,
    /// Their folder, the dialog's text being relative to it.
    pub folder: PathBuf,
    pub sources: Vec<PathBuf>,
    /// The one's name, shown name and whether a folder; none for several.
    pub one: Option<(OsString, String, bool)>,
    /// Inside an archive: the panel takes `names` out first.
    pub archived: Option<(Panel, Vec<OsString>)>,
    /// The side whose marks are dropped once queued.
    pub unmark: Option<usize>,
}

impl Batch {
    /// A panel's chosen entries; with `row`, what a drag from there takes.
    pub fn of(panel: &Panel, side: usize, row: Option<usize>) -> Option<Self> {
        let entries = match row {
            Some(row) => panel.dragged(row),
            None => panel.chosen(),
        };
        let folder = panel.folder().to_owned();
        let one = match entries.as_slice() {
            [] => return None,
            [one] => Some((one.name.clone(), one.display_name(), one.is_dir)),
            _ => None,
        };
        let names: Vec<OsString> = entries.iter().map(|e| e.name.clone()).collect();
        Some(Batch {
            from: match panel.in_archive() {
                true => Arc::new(Local),
                false => panel.fs(),
            },
            sources: names.iter().map(|name| folder.join(name)).collect(),
            archived: panel.in_archive().then(|| (panel.clone(), names)),
            unmark: Some(side),
            folder,
            one,
        })
    }

    /// Files of this computer, from another program.
    pub fn paths(paths: Vec<PathBuf>) -> Option<Self> {
        let folder = paths.first()?.parent()?.to_owned();
        let one = match paths.as_slice() {
            [one] => {
                let name = one.file_name()?.to_owned();
                Some((name.clone(), name.display().to_string(), one.is_dir()))
            }
            _ => None,
        };
        Some(Batch {
            from: Arc::new(Local),
            folder,
            sources: paths,
            one,
            archived: None,
            unmark: None,
        })
    }
}

impl Commander {
    /// The chosen entries into the other panel's folder, by way of the queue.
    pub(crate) fn transfer(&mut self, kind: Kind, window: &mut Window, cx: &mut Context<Self>) {
        let side = self.active;
        let Some(batch) = Batch::of(self.panels[side].tabs.current(), side, None) else {
            return;
        };
        let to = Place::of(self.panels[1 - side].tabs.current());
        self.open_transfer(kind, batch, to, window, cx);
    }

    /// F5's or F6's dialog for `batch` into `to`, then the queue.
    pub(crate) fn open_transfer(
        &mut self,
        kind: Kind,
        batch: Batch,
        to: Place,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match &to.archive {
            Some(Ok((archive, dir))) => return self.pack_into(kind, batch, archive.clone(), dir.clone(), window, cx),
            Some(Err(why)) => return window.push_notification(Notification::warning(why.clone()), cx),
            None => {}
        }
        // Moved out: 7-Zip must take them out of the archive after.
        if let (Some((panel, _)), Kind::Move) = (&batch.archived, kind)
            && let Err(err) = panel.archive_place()
        {
            return window.push_notification(Notification::warning(err.to_string()), cx);
        }
        let (what, target) = match &batch.one {
            Some((name, shown, false)) => (
                format!("\"{shown}\""),
                to.folder.join(name).display().to_string(),
            ),
            Some((_, shown, true)) => (format!("\"{shown}\""), folder_text(&to.folder)),
            None => (
                format!("{} items", batch.sources.len()),
                folder_text(&to.folder),
            ),
        };
        let verb = match kind {
            Kind::Copy => "Copy",
            Kind::Move => "Move",
        };
        let label = match &to.server {
            Some(url) => format!("{verb} {what} to {url}:"),
            None => format!("{verb} {what} to:"),
        };
        let commander = cx.entity().downgrade();
        let Place { fs: to, server, .. } = to;
        open_dialog(
            verb,
            label,
            target,
            window,
            cx,
            move |text, overwrite, window, cx| {
                let _ = commander.update(cx, |this, cx| {
                    // The server's own address typed: its path.
                    let text = server
                        .as_deref()
                        .and_then(|url| text.strip_prefix(url))
                        .unwrap_or(&text);
                    let target = Target::parse(text, &batch.folder, batch.sources.len(), &*to);
                    this.enqueue(kind, batch.clone(), to.clone(), target, overwrite, window, cx);
                });
            },
        );
    }

    /// F5 or F6 into an archive: asked, then 7-Zip packs them into its
    /// folder `dir` as a queued task; another archive's are taken out first.
    fn pack_into(&mut self, kind: Kind, batch: Batch, archive: PathBuf, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if batch.from.id() != "local" {
            let text = "Only this computer's files go into an archive: copy them here first.";
            return window.push_notification(Notification::warning(text), cx);
        }
        let what = match &batch.one {
            Some((_, shown, _)) => format!("\"{shown}\""),
            None => format!("{} items", batch.sources.len()),
        };
        let verb = if kind == Kind::Move { "Move" } else { "Copy" };
        let name = archive.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let into = match dir.as_os_str().is_empty() {
            true => name.clone(),
            false => format!("{name}{MAIN_SEPARATOR}{}", dir.display()),
        };
        let (title, text) = (format!("{verb} {what} into {name}"), format!("{verb} {what} into {into}? What has their names there is replaced."));
        let commander = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (commander, batch, archive, dir, title) = (commander.clone(), batch.clone(), archive.clone(), dir.clone(), title.clone());
            alert.title(verb).description(text.clone()).ok_text(verb).show_cancel(true).on_ok(move |_, window, cx| {
                let (batch, archive, dir, title) = (batch.clone(), archive.clone(), dir.clone(), title.clone());
                let _ = commander.update(cx, |this, cx| {
                    let queue = this.transfers.queue.clone();
                    let pack = move |sources: Vec<PathBuf>| {
                        queue.push_task(Job {
                            title,
                            run: Box::new(move |_| {
                                sc_core::archive::add(&archive, &sources, &dir)?;
                                if kind == Kind::Move {
                                    sources.iter().try_for_each(|source| sc_core::vfs::remove_all(&Local, source))?;
                                }
                                Ok(String::new())
                            }),
                        });
                    };
                    match batch.archived {
                        None => pack(batch.sources),
                        Some((panel, names)) => {
                            let taken = cx.background_executor().spawn(async move { panel.extract(&names) });
                            cx.spawn_in(window, async move |_, cx| match taken.await {
                                Ok(sources) => pack(sources),
                                Err(err) => {
                                    let note = Notification::error(format!("Could not take them out: {err}")).autohide(false);
                                    let _ = cx.update(|window, cx| window.push_notification(note, cx));
                                }
                            })
                            .detach();
                        }
                    }
                    if let Some(side) = batch.unmark {
                        this.panels[side].tabs.current_mut().mark_all(false);
                    }
                    cx.notify();
                });
                true
            })
        });
    }

    /// Into the queue; an archive's are taken out first.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn enqueue(
        &mut self,
        kind: Kind,
        batch: Batch,
        to: Arc<dyn Fs>,
        target: Target,
        overwrite: Overwrite,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let queue = self.transfers.queue.clone();
        let from = batch.from.clone();
        // Moved out of an archive: what was moved goes from it, by 7-Zip, once the move is done.
        let leave = match (&batch.archived, kind) {
            (Some((panel, names)), Kind::Move) => panel.archive_place().ok().map(|(archive, dir)| {
                let inside: Vec<PathBuf> = names.iter().map(|name| dir.join(name)).collect();
                let name = archive.file_name().unwrap_or_default().to_string_lossy().into_owned();
                let queue = queue.clone();
                move |taken: Vec<PathBuf>| {
                    queue.push_task(Job {
                        title: format!("Remove the moved from {name}"),
                        run: Box::new(move |_| {
                            // Moved whole is gone from where it was taken out to; skipped, failed, cancelled or half-moved is still there.
                            let gone: Vec<PathBuf> = inside.into_iter().zip(taken).filter(|(_, out)| std::fs::symlink_metadata(out).is_err()).map(|(inside, _)| inside).collect();
                            if !gone.is_empty() {
                                sc_core::archive::delete(&archive, &gone)?;
                            }
                            Ok(String::new())
                        }),
                    });
                }
            }),
            _ => None,
        };
        let push = move |sources: Vec<PathBuf>| {
            queue.push(Operation {
                kind,
                from,
                sources: sources.clone(),
                to,
                target,
                overwrite,
            });
            if let Some(leave) = leave {
                leave(sources);
            }
        };
        match batch.archived {
            None => drop(push(batch.sources)),
            Some((panel, names)) => {
                cx.spawn_in(window, async move |_, cx| {
                    let taken = cx
                        .background_executor()
                        .spawn(async move { panel.extract(&names) })
                        .await;
                    match taken {
                        Ok(sources) => drop(push(sources)),
                        Err(err) => {
                            let text = format!("Could not take them out: {err}");
                            let note = Notification::error(text).autohide(false);
                            let _ = cx.update(|window, cx| window.push_notification(note, cx));
                        }
                    }
                })
                .detach();
            }
        }
        // Queued: the marks have done their work.
        if let Some(side) = batch.unmark {
            self.panels[side].tabs.current_mut().mark_all(false);
        }
        cx.notify();
    }

    /// Checks the queue several times a second while the window lives.
    pub(crate) fn watch_queue(window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(150))
                    .await;
                let watched = this.update_in(cx, |this, window, cx| this.queue_changed(window, cx));
                if watched.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn queue_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.transfers.queue.forget_done(DONE_KEPT);
        let transfers = &mut self.transfers;
        let version = transfers.queue.version();
        if version == transfers.version {
            return;
        }
        transfers.version = version;
        let jobs = transfers.queue.jobs();
        let running = jobs
            .iter()
            .find(|job| job.status == Status::Running)
            .map(|job| job.id);
        if transfers.started.map(|(id, _)| id) != running {
            transfers.started = running.map(|id| (id, Instant::now()));
        }
        let finished: Vec<u64> = jobs
            .iter()
            .filter(|job| job.status.finished())
            .map(|job| job.id)
            .collect();
        let newly: Vec<&JobView> = jobs
            .iter()
            .filter(|job| job.status.finished() && !transfers.seen.contains(&job.id))
            .collect();
        transfers.seen = finished.into_iter().collect();
        // What a task found, or why it failed.
        for job in &newly {
            let note = match (&job.status, &job.note) {
                (Status::Failed, Some(note)) => {
                    let text = format!("{}: {note}", job.title);
                    Notification::error(text).autohide(false)
                }
                (_, Some(note)) => Notification::success(format!("{}: {note}", job.title)),
                _ => continue,
            };
            window.push_notification(note, cx);
        }
        let newly = !newly.is_empty();
        if newly {
            for ix in 0..self.panels.len() {
                // shortcut: a panel whose folder is gone keeps its list until it moves.
                let remote = self.panels[ix].tabs.current().remote().is_some();
                self.navigate_on(ix, remote, Panel::reload, window, cx);
            }
        }
        if self.transfers.asked.is_some_and(|id| {
            !jobs
                .iter()
                .any(|job| job.id == id && job.question.is_some())
        }) {
            self.transfers.asked = None;
        }
        if self.transfers.asked.is_none()
            && let Some(job) = jobs.iter().find(|job| job.question.is_some())
        {
            self.transfers.asked = Some(job.id);
            ask(&self.transfers.queue, job, window, cx);
        }
        cx.notify();
    }

    /// Above the button bar while jobs run or wait.
    pub(crate) fn render_transfers(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        // The sidebar's Operations shows it then.
        if self.sidebar.tab == Some(SidebarTab::Operations) {
            return None;
        }
        let jobs = self.transfers.queue.jobs();
        let waiting = jobs
            .iter()
            .filter(|job| job.status == Status::Waiting)
            .count();
        let job = jobs
            .iter()
            .find(|job| job.status == Status::Running)?
            .clone();
        let theme = cx.theme();
        let p = &job.progress;
        let percent = match p.total_bytes {
            0 => 0.,
            total => p.bytes as f32 * 100. / total as f32,
        };
        let state = match p.scanning {
            true => format!("counting: {} file(s)", p.total_files),
            false => format!("{} / {} file(s), {percent:.0}%", p.files, p.total_files),
        };
        let speed = self.transfers.started.and_then(|(_, at)| {
            let seconds = at.elapsed().as_secs_f64();
            (seconds > 1. && !p.scanning).then(|| {
                format!(
                    "{}/s",
                    sc_core::human_size(((p.bytes - p.resumed) as f64 / seconds) as u64)
                )
            })
        });
        Some(
            h_flex()
                .px_2()
                .py_1()
                .gap_3()
                // Under the frame above, apart from the command line below.
                .border_b_1()
                .border_color(theme.border)
                .text_sm()
                .child(
                    div()
                        .flex_none()
                        .max_w(px(380.))
                        .truncate()
                        .child(job.title),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(80.))
                        .child(Bar::new("transfer-progress").value(percent)),
                )
                .child(div().flex_none().child(state))
                .children(speed)
                .child(
                    Button::new("transfer-queue")
                        .ghost()
                        .small()
                        .label(match waiting {
                            0 => "Queue...".to_string(),
                            n => format!("Queue ({n} waiting)..."),
                        })
                        .on_click(cx.listener(|this, _, window, cx| this.show_operations(window, cx))),
                ),
        )
    }
}

fn folder_text(folder: &std::path::Path) -> String {
    let mut text = folder.display().to_string();
    if !text.ends_with(MAIN_SEPARATOR) {
        text.push(MAIN_SEPARATOR);
    }
    text
}

type Done = Rc<dyn Fn(String, Overwrite, &mut Window, &mut App)>;

/// "Copy 3 items to:", the target, and what to do with files there already.
fn open_dialog(
    title: &'static str,
    label: String,
    target: String,
    window: &mut Window,
    cx: &mut App,
    done: impl Fn(String, Overwrite, &mut Window, &mut App) + 'static,
) {
    let input = cx.new(|cx| InputState::new(window, cx).default_value(target));
    let view = cx.new(|_| TransferDialog {
        label,
        input: input.clone(),
        overwrite: Overwrite::Ask,
        done: Rc::new(done),
    });
    window.open_dialog(cx, move |dialog, _, _| {
        let ok = view.clone();
        dialog
            .title(title)
            .width(px(560.))
            .overlay_closable(false)
            .child(view.clone())
            .on_ok(move |_, window, cx| {
                ok.update(cx, |this, cx| this.finish(window, cx));
                true
            })
    });
    input.update(cx, |input, cx| {
        input.focus(window, cx);
        input.select_all(window, cx);
    });
}

struct TransferDialog {
    label: String,
    input: Entity<InputState>,
    overwrite: Overwrite,
    done: Done,
}

impl TransferDialog {
    fn finish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.input.read(cx).value().to_string();
        (self.done)(target, self.overwrite, window, cx);
    }
}

impl Render for TransferDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = OVERWRITE
            .iter()
            .position(|(mode, _)| *mode == self.overwrite);
        v_flex()
            .w_full()
            .gap_3()
            .text_sm()
            .child(
                v_flex()
                    .gap_1()
                    .child(self.label.clone())
                    .child(Input::new(&self.input)),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child("Where the target has a file of the same name:")
                    .child(
                        RadioGroup::vertical("transfer-overwrite")
                            .children(OVERWRITE.map(|(_, label)| label))
                            .selected_index(selected)
                            .on_click(cx.listener(|this, ix: &usize, _, cx| {
                                this.overwrite = OVERWRITE[*ix].0;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("transfer-ok")
                            .primary()
                            .label("OK")
                            .min_w(px(80.))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.finish(window, cx);
                                window.close_dialog(cx);
                            })),
                    )
                    .child(
                        Button::new("transfer-cancel")
                            .outline()
                            .label("Cancel")
                            .min_w(px(80.))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
    }
}

/// A running job's question; it waits until a button answers.
fn ask(queue: &Queue, job: &JobView, window: &mut Window, cx: &mut App) {
    let Some(question) = job.question.clone() else {
        return;
    };
    let (title, text, answers): (&str, String, &[(Answer, &str)]) = match &question {
        Question::Exists {
            target,
            source,
            existing,
        } => (
            "File exists",
            format!(
                "{}\n\nThere already: {}\nThe new one: {}",
                target.display(),
                describe(existing),
                describe(source)
            ),
            &[
                (Answer::Overwrite, "Overwrite"),
                (Answer::OverwriteAll, "Overwrite all"),
                (Answer::Skip, "Skip"),
                (Answer::SkipAll, "Skip all"),
                (Answer::Rename, "Rename"),
                (Answer::Cancel, "Cancel"),
            ],
        ),
        // Permission denied: an administrator may, as Total Commander offers.
        Question::Failed { path, error, elevatable: true } => (
            "Error",
            format!("{}\n\n{error}\n\nAs administrator: this and what else is refused, once the rest is done, asking for the password once.", path.display()),
            &[
                (Answer::Elevate, "As administrator"),
                (Answer::Retry, "Retry"),
                (Answer::Skip, "Skip"),
                (Answer::SkipAll, "Skip all"),
                (Answer::Cancel, "Cancel"),
            ],
        ),
        Question::Failed { path, error, .. } => (
            "Error",
            format!("{}\n\n{error}", path.display()),
            &[
                (Answer::Retry, "Retry"),
                (Answer::Skip, "Skip"),
                (Answer::SkipAll, "Skip all"),
                (Answer::Cancel, "Cancel"),
            ],
        ),
    };
    let (queue, id) = (queue.clone(), job.id);
    let job_title = job.title.clone();
    window.open_dialog(cx, move |dialog, _, _| {
        let buttons = answers.iter().map(|&(answer, label)| {
            let queue = queue.clone();
            let button = Button::new(label)
                .label(label)
                .on_click(move |_, window, cx| {
                    queue.answer(id, answer);
                    window.close_dialog(cx);
                });
            match answer {
                Answer::Overwrite | Answer::Retry => button.primary(),
                Answer::Cancel => button.with_variant(ButtonVariant::Danger),
                _ => button.outline(),
            }
        });
        dialog
            .title(title)
            .width(px(560.))
            .keyboard(false)
            .close_button(false)
            .overlay_closable(false)
            .child(
                v_flex()
                    .gap_3()
                    .text_sm()
                    .child(div().text_xs().child(job_title.clone()))
                    .child(div().whitespace_normal().child(text.clone()))
                    .child(h_flex().gap_2().flex_wrap().justify_end().children(buttons)),
            )
    });
}

fn describe(meta: &Meta) -> String {
    let time = meta
        .modified
        .map(sc_core::dates::date_time_seconds);
    format!(
        "{} bytes, {}",
        sc_core::grouped(meta.size),
        time.unwrap_or_default()
    )
}

/// A waiting job dragged in the Operations list, to wait elsewhere.
#[derive(Clone, Copy)]
struct JobDrag(u64);

/// A done job stays listed this long; failed and cancelled ones until cleared.
const DONE_KEPT: Duration = Duration::from_secs(300);

/// How far a job is, 0 to 1, by its bytes, else its files; none while counting.
fn done_part(progress: &sc_core::ops::Progress) -> Option<f32> {
    let part = match (progress.total_bytes, progress.total_files) {
        _ if progress.scanning => return None,
        (0, 0) => return None,
        (0, files) => progress.files as f32 / files as f32,
        (bytes, _) => progress.bytes as f32 / bytes as f32,
    };
    Some(part.clamp(0., 1.))
}

/// `1:05:00`, `4:10`, `12 s`.
fn duration_text(seconds: u64) -> String {
    match seconds {
        0..60 => format!("{seconds} s"),
        60..3600 => format!("{}:{:02}", seconds / 60, seconds % 60),
        _ => format!("{}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60),
    }
}

/// A running job's files, bytes, speed and time left, as far as known.
fn running_text(job: &JobView) -> String {
    let p = &job.progress;
    let mut parts = Vec::new();
    if p.total_files > 1 {
        parts.push(format!("{} / {} files", p.files, p.total_files));
    }
    if p.total_bytes > 0 {
        parts.push(format!("{} of {}", sc_core::human_size(p.bytes), sc_core::human_size(p.total_bytes)));
    }
    let seconds = job.started.map_or(0., |at| at.elapsed().as_secs_f64());
    let speed = (seconds > 1.).then(|| (p.bytes - p.resumed) as f64 / seconds).filter(|speed| *speed > 0.);
    if let Some(speed) = speed {
        parts.push(format!("{}/s", sc_core::human_size(speed as u64)));
        if p.total_bytes > p.bytes {
            parts.push(format!("{} left", duration_text(((p.total_bytes - p.bytes) as f64 / speed) as u64)));
        }
    }
    match parts.is_empty() {
        true => "Running".into(),
        false => parts.join(" · "),
    }
}

impl Commander {
    /// Commands › Operation Queue: the sidebar's Operations, shown.
    pub(crate) fn show_operations(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sidebar.tab != Some(SidebarTab::Operations) {
            self.toggle_sidebar(SidebarTab::Operations, window, cx);
        }
    }

    /// The Operations header's summary while jobs run or wait (or failed),
    /// and how far the running ones are, 0 to 1.
    pub(crate) fn operations_summary(&self) -> Option<(String, Option<f32>)> {
        let jobs = self.transfers.queue.jobs();
        let count = |status: Status| jobs.iter().filter(|job| job.status == status).count();
        let (running, waiting, failed) = (count(Status::Running), count(Status::Waiting), count(Status::Failed));
        let parts: Vec<f32> = jobs.iter().filter(|job| job.status == Status::Running).filter_map(|job| done_part(&job.progress)).collect();
        let done = (!parts.is_empty()).then(|| parts.iter().sum::<f32>() / parts.len() as f32);
        let mut text = Vec::new();
        match done {
            Some(done) => text.push(format!("{:.0}%", done * 100.)),
            None if running > 0 => text.push(format!("{running} running")),
            None => {}
        }
        if waiting > 0 {
            text.push(format!("{waiting} waiting"));
        }
        if failed > 0 {
            text.push(format!("{failed} failed"));
        }
        (!text.is_empty()).then(|| (text.join(" · "), done))
    }

    /// The jobs: running and waiting ones in the queue's order (the waiting
    /// dragged into another), then the finished ones, the newest first.
    pub(crate) fn render_operations(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let jobs = self.transfers.queue.jobs();
        let waiting: Rc<Vec<u64>> = Rc::new(jobs.iter().filter(|job| job.status == Status::Waiting).map(|job| job.id).collect());
        let (unfinished, finished): (Vec<&JobView>, Vec<&JobView>) = jobs.iter().partition(|job| !job.status.finished());
        let rows: Vec<AnyElement> = unfinished.into_iter().chain(finished.iter().rev().copied()).map(|job| self.render_job(job, waiting.clone(), cx).into_any_element()).collect();
        let theme = cx.theme();
        let queue = self.transfers.queue.clone();
        let list = v_flex().id("operations").flex_1().min_h_0().overflow_y_scroll().px_1().gap_0p5().children(rows);
        let list = match jobs.is_empty() {
            true => list.child(div().p_2().text_color(theme.muted_foreground).child("No operations.")),
            false => list,
        };
        v_flex().size_full().child(list).when(!finished.is_empty(), |el| {
            el.child(
                h_flex().px_1().pt_1().justify_end().child(Button::new("operations-clear").ghost().small().label("Clear finished").on_click(move |_, _, _| queue.clear_finished())),
            )
        })
    }

    fn render_job(&self, job: &JobView, waiting: Rc<Vec<u64>>, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let (id, queue) = (job.id, self.transfers.queue.clone());
        let p = &job.progress;
        let (detail, color) = match &job.status {
            Status::Waiting => ("Waiting".to_owned(), theme.muted_foreground),
            Status::Running if job.question.is_some() => ("Waiting for your answer".into(), theme.warning),
            Status::Running if p.scanning => (format!("Counting: {} files", p.total_files), theme.muted_foreground),
            Status::Running => (running_text(job), theme.muted_foreground),
            Status::Done if p.skipped > 0 => (format!("Done, {} skipped", p.skipped), theme.muted_foreground),
            Status::Done => (job.note.clone().unwrap_or_else(|| "Done".into()), theme.muted_foreground),
            Status::Cancelled => ("Cancelled".into(), theme.muted_foreground),
            Status::Failed => (job.note.clone().unwrap_or_else(|| "Failed".into()), theme.danger),
        };
        let percent = done_part(p).map(|done| done * 100.);
        let running = job.status == Status::Running;
        let is_waiting = job.status == Status::Waiting;
        let cancel = (!job.status.finished()).then(|| {
            let queue = queue.clone();
            Button::new(("job-cancel", id)).ghost().xsmall().icon(IconName::Close).tooltip("Cancel").on_click(move |_, _, _| queue.cancel(id))
        });
        let title = job.title.clone();
        let target = theme.drop_target;
        let row = v_flex()
            .id(("job", id))
            .px_1p5()
            .py_1()
            .gap_1()
            .rounded(theme.radius)
            .hover(|style| style.bg(theme.foreground.opacity(0.06)))
            .when(job.status.finished(), |el| el.opacity(0.75))
            .child(
                h_flex()
                    .gap_1()
                    .child(div().flex_1().min_w_0().truncate().child(title.clone()))
                    .children(cancel),
            )
            .when(running, |el| el.child(Bar::new(("job-bar", id)).value(percent.unwrap_or(0.))))
            .child(div().text_xs().text_color(color).whitespace_normal().child(detail));
        // Waiting ones go where they are dropped, or a place up or down.
        let dropped = self.transfers.queue.clone();
        let row = row.when(is_waiting, move |row| {
            let text: SharedString = title.into();
            row.on_drag(JobDrag(id), move |_, grab, _, cx| cx.new(|_| DragLabel { text: text.clone(), grab }))
                .drag_over::<JobDrag>(move |style, _, _, _| style.bg(target))
                .on_drop(move |JobDrag(moved): &JobDrag, _, _| {
                    let at = |job: u64| waiting.iter().position(|&id| id == job);
                    if let (Some(from), Some(to)) = (at(*moved), at(id)) {
                        dropped.reorder(*moved, to as isize - from as isize);
                    }
                })
        });
        let step = move |by: isize, label: &'static str| {
            let queue = queue.clone();
            gpui_kit::component::menu::PopupMenuItem::new(label).on_click(move |_, _, _| queue.reorder(id, by))
        };
        let (menu_queue, going) = (self.transfers.queue.clone(), !job.status.finished());
        row.context_menu(move |menu, _, _| {
            let queue = menu_queue.clone();
            let menu = match is_waiting {
                true => menu.item(step(-1, "Move Up")).item(step(1, "Move Down")).separator(),
                false => menu,
            };
            match going {
                true => menu.item(gpui_kit::component::menu::PopupMenuItem::new("Cancel").on_click(move |_, _, _| queue.cancel(id))),
                false => menu.item(gpui_kit::component::menu::PopupMenuItem::new("Clear Finished").on_click(move |_, _, _| queue.clear_finished())),
            }
        })
    }
}
