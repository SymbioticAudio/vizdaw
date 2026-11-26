//! The Settings window (Swift `Settings { TabView { … } }`,
//! `VizDAWApp.swift:267-287`): one per app, the nine tabs in Swift's
//! order. It belongs to no document, so [`AppState`] keeps its handle
//! outside `windows`, and it is not one of a document's key contexts:
//! Space in it does not play. A tab whose slice has not come shows
//! "`<Tab>` comes in a later version.".

use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, Bounds, Context, FocusHandle, InteractiveElement, IntoElement,
    ParentElement, Render, StatefulInteractiveElement, Styled, Task, TitlebarOptions, Window,
    WindowBounds, WindowOptions, anchored, deferred, div, point, px, rgb, size,
};
use vizdaw_app::audio_prefs::{
    self, AudioPrefs, BUFFER_SIZES, cache_choices, cache_text, count_text, device_choices, device_tag,
    round_trip_text, trim_text,
};
use vizdaw_app::automation_prefs::{
    AFTER_WRITE_CHOICES, APPLIES_TEXT, AutomationPrefs, MAX_RETURN_MS, PARAMS_ONLY_HELP,
    PARAMS_ONLY_LABEL, RETURN_STEP, THINNING_CHOICES, THINNING_HELP, return_text,
};
use vizdaw_app::record::MIDI_TAKES_HELP;
use vizdaw_app::{DocSession, Prefs, formats};
use vizdaw_ffi::InputInfoRow;
use vizdaw_ffi::host::InputDeviceRow;

use crate::app_state::AppState;
use crate::controls;
use crate::controls::stepper::{Range, stepper_buttons};
use crate::menus::{CloseWindow, SETTINGS_WINDOW_CONTEXT};
use crate::registry::{RegistryScope, SharedRegistry};
use crate::strip_palette::tokens;

/// The window's width (Swift's Form width).
pub const WIDTH: f32 = 500.;
/// Its content height when a tab has no height of its own.
pub const DEFAULT_HEIGHT: f32 = 340.;
/// The Audio tab's content height: four sections, nine rows, six captions.
pub const AUDIO_HEIGHT: f32 = 660.;
/// The Recording tab's content height: three sections, nine rows.
pub const RECORDING_HEIGHT: f32 = 470.;
/// The Automation tab's content height: five rows, three captions.
pub const AUTOMATION_HEIGHT: f32 = 300.;
/// The tab strip's room: two rows, for where the labels wrap (the body
/// takes what one row leaves over).
const STRIP_HEIGHT: f32 = 72.;
/// The round-trip readout's pace while the Audio tab shows (Swift
/// `AudioSettingsView.swift:110-115`).
const POLL_EVERY: Duration = Duration::from_secs(1);
/// How often a waiting read's answer is looked for.
const TAKE_EVERY: Duration = Duration::from_millis(20);

/// The Settings tabs, in Swift's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Appearance,
    Editing,
    Keys,
    Audio,
    Recording,
    Automation,
    Surfaces,
    Plugins,
    CoProducer,
}

impl SettingsTab {
    pub const ALL: [SettingsTab; 9] = [
        SettingsTab::Appearance,
        SettingsTab::Editing,
        SettingsTab::Keys,
        SettingsTab::Audio,
        SettingsTab::Recording,
        SettingsTab::Automation,
        SettingsTab::Surfaces,
        SettingsTab::Plugins,
        SettingsTab::CoProducer,
    ];

    /// The tab's label (`VizDAWApp.swift:269-285`).
    pub fn label(self) -> &'static str {
        match self {
            SettingsTab::Appearance => "Appearance",
            SettingsTab::Editing => "Editing",
            SettingsTab::Keys => "Keys",
            SettingsTab::Audio => "Audio",
            SettingsTab::Recording => "Recording",
            SettingsTab::Automation => "Automation",
            SettingsTab::Surfaces => "Surfaces",
            SettingsTab::Plugins => "Plug-ins",
            SettingsTab::CoProducer => "Co-Producer",
        }
    }

    /// The window's content height with this tab shown (Tasks 5-6 give
    /// their tabs their own).
    pub fn height(self) -> f32 {
        match self {
            SettingsTab::Audio => AUDIO_HEIGHT,
            SettingsTab::Recording => RECORDING_HEIGHT,
            SettingsTab::Automation => AUTOMATION_HEIGHT,
            _ => DEFAULT_HEIGHT,
        }
    }
}

/// The line a tab not built yet shows.
pub fn placeholder(tab: SettingsTab) -> String {
    format!("{} comes in a later version.", tab.label())
}

/// The Audio tab's pickers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AudioMenu {
    Device,
    Buffer,
    Cache,
    Rate,
    Depth,
    Monitor,
    AfterWrite,
    Thinning,
}

/// What choosing an entry does.
type Pick = Rc<dyn Fn(&mut Window, &mut App)>;

/// One entry of an open picker.
struct Choice {
    title: String,
    checked: bool,
    pick: Pick,
}

/// Runs `work` on its own thread (it may block: a core or HAL read) and
/// hands its answer to `land` on the UI thread, if the window is still
/// there. A panicking `work` lands as `None` (so the read is still over
/// and the take-loop ends).
fn read_off_thread<T: Send + 'static>(
    cx: &mut Context<SettingsWindow>,
    work: impl FnOnce() -> T + Send + 'static,
    land: impl FnOnce(&mut SettingsWindow, Option<T>, &mut Context<SettingsWindow>) + 'static,
) {
    // The thread never wakes a GPUI task (a wake from another thread is
    // not allowed under the test scheduler): the answer waits in a slot,
    // and a short timer on the UI side takes it.
    let slot: Arc<Mutex<Option<Option<T>>>> = Arc::new(Mutex::new(None));
    let put = slot.clone();
    std::thread::spawn(move || {
        let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).ok();
        *put.lock().unwrap() = Some(answer);
    });
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor().timer(TAKE_EVERY).await;
            let answer = slot.lock().unwrap().take();
            if let Some(answer) = answer {
                this.update(cx, |t, cx| land(t, answer, cx)).ok();
                break;
            }
            if this.upgrade().is_none() {
                break;
            }
        }
    })
    .detach();
}

/// The first session whose input is open: the one whose latencies the tab
/// shows (Swift `inputHolder`, `AudioSettingsView.swift:61-63`).
fn input_of_holder(sessions: &[Arc<dyn DocSession>]) -> Option<InputInfoRow> {
    sessions
        .iter()
        .filter_map(|s| s.input_info().ok())
        .find(|i| !i.device.is_empty())
}

/// The Settings window's view.
pub struct SettingsWindow {
    tab: SettingsTab,
    registry: SharedRegistry,
    focus: FocusHandle,
    /// The input devices as the last read found them.
    devices: Vec<InputDeviceRow>,
    device_gen: u64,
    /// The first open input as the last read found it (render shows only
    /// this).
    info: Option<InputInfoRow>,
    /// Bumped when the reads stop, so a late answer is dropped.
    round_gen: u64,
    /// A round-trip read is out (its thread is still running). Apart from
    /// the generation: re-selecting the tab while one hangs starts no
    /// second thread.
    reading: bool,
    /// The 1 Hz poll, alive only while the Audio tab shows.
    poll: Option<Task<()>>,
    menu: Option<AudioMenu>,
}

impl SettingsWindow {
    fn new(tab: SettingsTab, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        // ⌘W reaches the window's root through its focus.
        window.focus(&focus, cx);
        cx.on_release(|_, cx| {
            let app = AppState::global(cx);
            app.update(cx, |s, _| s.settings_window = None);
        })
        .detach();
        // The tab shows the app's audio settings: it follows their changes.
        let app = AppState::global(cx);
        cx.observe(&app, |_, _, cx| cx.notify()).detach();
        let mut this = SettingsWindow {
            tab,
            registry: SharedRegistry::default(),
            focus,
            devices: vec![],
            device_gen: 0,
            info: None,
            round_gen: 0,
            reading: false,
            poll: None,
            menu: None,
        };
        this.shown(cx);
        this
    }

    /// The tab shown.
    pub fn tab(&self) -> SettingsTab {
        self.tab
    }

    /// The elements this window drew last.
    pub fn registry(&self) -> &SharedRegistry {
        &self.registry
    }

    fn select(&mut self, tab: SettingsTab, window: &mut Window, cx: &mut Context<Self>) {
        self.tab = tab;
        self.menu = None;
        window.resize(size(px(WIDTH), px(tab.height() + STRIP_HEIGHT)));
        self.shown(cx);
        cx.notify();
    }

    /// The tab changed (or opened): the Audio tab's reads start, and any
    /// other tab's stop.
    fn shown(&mut self, cx: &mut Context<Self>) {
        self.poll = None;
        self.round_gen += 1;
        if self.tab != SettingsTab::Audio {
            return;
        }
        self.read_devices(cx);
        self.poll = Some(cx.spawn(async move |this, cx| {
            loop {
                if this.update(cx, |t, cx| t.read_round_trip(cx)).is_err() {
                    break;
                }
                cx.background_executor().timer(POLL_EVERY).await;
            }
        }));
    }

    /// Reads the device list off the UI thread (the tab opens, the picker
    /// opens); only the newest read lands.
    fn read_devices(&mut self, cx: &mut Context<Self>) {
        self.device_gen += 1;
        let generation = self.device_gen;
        let core = AppState::global(cx).read(cx).store.core();
        read_off_thread(
            cx,
            move || core.audio_input_device_list(),
            move |t, devices, cx| {
                let Some(devices) = devices else { return };
                if t.device_gen == generation && t.devices != devices {
                    t.devices = devices;
                    cx.notify();
                }
            },
        );
    }

    /// Reads the first open input off the UI thread; a read still out
    /// holds off the next.
    fn read_round_trip(&mut self, cx: &mut Context<Self>) {
        if self.reading {
            return;
        }
        self.reading = true;
        let generation = self.round_gen;
        let sessions = AppState::global(cx).read(cx).store.sessions();
        read_off_thread(
            cx,
            move || input_of_holder(&sessions),
            move |t, info, cx| {
                // Whatever the generation, the read is over.
                t.reading = false;
                if t.round_gen != generation {
                    return;
                }
                // A read that panicked leaves what was shown.
                let Some(info) = info else { return };
                if t.info != info {
                    t.info = info;
                    cx.notify();
                }
            },
        );
    }

    /// Changes an audio setting (core first, saved after); a refusal is
    /// the tab's `problem` line.
    fn change(
        cx: &mut App,
        f: impl FnOnce(&mut AudioPrefs, &mut Prefs, &dyn vizdaw_app::CoreCalls) -> Result<(), String>,
    ) {
        let app = AppState::global(cx);
        app.update(cx, |s, cx| {
            let core = s.store.core();
            let _ = f(&mut s.audio, &mut s.prefs, core.as_ref());
            // New documents start from the (changed) record defaults.
            s.store.set_record_defaults(s.audio.record_defaults());
            cx.notify();
        });
    }

    /// Changes an automation setting (core first, saved after); a refusal
    /// is the tab's `problem` line.
    fn change_automation(
        cx: &mut App,
        f: impl FnOnce(
            &mut AutomationPrefs,
            &mut Prefs,
            &dyn vizdaw_app::CoreCalls,
        ) -> Result<(), String>,
    ) {
        let app = AppState::global(cx);
        app.update(cx, |s, cx| {
            let core = s.store.core();
            let _ = f(&mut s.automation, &mut s.prefs, core.as_ref());
            cx.notify();
        });
    }

    // One body per tab; Tasks 5-6 fill Audio, Recording and Automation.

    fn appearance_tab(&mut self, scope: &mut RegistryScope) -> AnyElement {
        placeholder_body(SettingsTab::Appearance, scope)
    }

    fn editing_tab(&mut self, scope: &mut RegistryScope) -> AnyElement {
        placeholder_body(SettingsTab::Editing, scope)
    }

    fn keys_tab(&mut self, scope: &mut RegistryScope) -> AnyElement {
        placeholder_body(SettingsTab::Keys, scope)
    }

    fn audio_tab(
        &mut self,
        scope: &mut RegistryScope,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (st, problem) = {
            let a = AppState::global(cx).read(cx);
            (
                a.audio.settings().clone(),
                a.audio.problem().map(str::to_owned),
            )
        };
        let t = tokens();

        // The Input section.
        let choices = device_choices(&st, &self.devices);
        let current = device_tag(&st, &self.devices);
        let face = choices
            .iter()
            .find(|c| c.tag().map(str::to_owned) == current)
            .map_or("Same as output", |c| c.title.as_str())
            .to_owned();
        let device_items = choices
            .into_iter()
            .map(|c| Choice {
                checked: c.tag().map(str::to_owned) == current,
                title: c.title.clone(),
                pick: Rc::new(move |_, cx| {
                    let row = c.name.clone().map(|name| InputDeviceRow {
                        name,
                        uid: c.uid.clone().unwrap_or_default(),
                    });
                    Self::change(cx, move |a, p, core| a.set_input_device(row, p, core));
                }),
            })
            .collect();
        let device = self.picker(
            "Input device",
            &face,
            AudioMenu::Device,
            device_items,
            scope,
            window,
            cx,
        );

        let st_on = st.software_monitoring;
        let monitoring = switch(
            "settings-software-monitoring",
            "Software Monitoring",
            st.software_monitoring,
            true,
            scope,
            move |_, cx| {
                Self::change(cx, move |a, p, core| {
                    a.set_software_monitoring(!st_on, p, core)
                });
                // Every document's meters follow at once.
                AppState::global(cx).update(cx, |s, _| s.store.refresh_input_monitoring());
            },
        );

        // The Latency section.
        let buffer_items = BUFFER_SIZES
            .iter()
            .map(|&n| Choice {
                title: format!("{n} frames"),
                checked: n == st.buffer_frames,
                pick: Rc::new(move |_, cx| {
                    Self::change(cx, move |a, p, core| a.set_buffer_frames(n, p, core));
                }),
            })
            .collect();
        let buffer = self.picker(
            "I/O Buffer Size",
            &format!("{} frames", st.buffer_frames),
            AudioMenu::Buffer,
            buffer_items,
            scope,
            window,
            cx,
        );
        let round_trip = round_trip_text(self.info.as_ref(), st.buffer_frames);
        let rate = self.info.as_ref().map_or(0.0, |i| i.sample_rate);
        let trim = i64::from(st.latency_trim);
        let trim_range = Range {
            min: -i64::from(audio_prefs::MAX_TRIM),
            max: i64::from(audio_prefs::MAX_TRIM),
            step: 1,
        };
        let trim_buttons = stepper_buttons("Latency trim", trim_range, trim, scope, |v, _, cx| {
            Self::change(cx, move |a, p, core| a.set_latency_trim(v as i32, p, core));
        });
        let reset = {
            let enabled = st.latency_trim != 0;
            let mut face = div()
                .id("settings-reset-trim")
                .px_2()
                .h(px(controls::stepper::BUTTON))
                .flex()
                .items_center()
                .rounded(px(t.radius))
                .border_1()
                .border_color(rgb(t.surface_separator))
                .bg(rgb(t.surface_inset))
                .opacity(if enabled { 1. } else { 0.45 })
                .child("Reset Trim");
            if enabled {
                face = face.cursor_pointer().on_click(|_, _, cx| {
                    Self::change(cx, |a, p, core| a.set_latency_trim(0, p, core));
                });
            }
            div()
                .relative()
                .flex_none()
                .child(face)
                .child(scope.control("button", "Reset Trim", enabled))
                .into_any_element()
        };

        // The Bounce section.
        let tail = i64::from(st.bounce_tail_seconds);
        let tail_buttons = stepper_buttons(
            "Bounce tail limit",
            Range {
                min: 0,
                max: 30,
                step: 1,
            },
            tail,
            scope,
            |v, _, cx| {
                Self::change(cx, move |a, p, core| {
                    a.set_bounce_tail_seconds(v as u32, p, core)
                });
            },
        );

        // The Streaming section.
        let cache_items = cache_choices(st.cache_mb)
            .into_iter()
            .map(|mb| Choice {
                title: cache_text(mb),
                checked: mb == st.cache_mb,
                pick: Rc::new(move |_, cx| {
                    Self::change(cx, move |a, p, core| a.set_cache_mb(mb, p, core));
                }),
            })
            .collect();
        let cache = self.picker(
            "RAM cache",
            &cache_text(st.cache_mb),
            AudioMenu::Cache,
            cache_items,
            scope,
            window,
            cx,
        );
        let ahead = i64::from(st.read_ahead_seconds);
        let ahead_buttons = stepper_buttons(
            "Disk read-ahead",
            Range {
                min: 2,
                max: 8,
                step: 1,
            },
            ahead,
            scope,
            |v, _, cx| {
                Self::change(cx, move |a, p, core| {
                    a.set_read_ahead_seconds(v as u32, p, core)
                });
            },
        );

        let mut body = div().flex().flex_col().gap(px(6.)).p_4();
        body = body
            .child(section("Input", scope))
            .child(form_row("Input device", device, scope))
            .child(caption(
                "A new device takes effect at once; while recording or measuring, once that stops.",
                scope,
            ))
            .child(monitoring)
            .child(caption(
                "Off: VizDAW never plays an input back; listen through your interface's direct monitoring. Recording and MIDI thru are unaffected.",
                scope,
            ))
            .child(section("Latency", scope))
            .child(form_row("I/O Buffer Size", buffer, scope))
            .child(caption(&round_trip, scope))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(div().flex_1().child(text("Latency trim", scope)))
                    .child(text(&trim_text(st.latency_trim, rate), scope))
                    .child(trim_buttons),
            )
            .child(div().flex().flex_row().child(reset))
            .child(section("Bounce", scope))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(div().flex_1().child(text(
                        &format!("Bounce tail limit: {} s", st.bounce_tail_seconds),
                        scope,
                    )))
                    .child(tail_buttons),
            )
            .child(caption(
                "A bounce renders on after the range until it is quiet, at most this long.",
                scope,
            ))
            .child(section("Streaming", scope))
            .child(form_row("RAM cache", cache, scope))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(div().flex_1().child(text(
                        &format!("Disk read-ahead: {} s", st.read_ahead_seconds),
                        scope,
                    )))
                    .child(ahead_buttons),
            )
            .child(caption("Cache and read-ahead apply from the next playback.", scope));
        if let Some(p) = Self::problem_line(problem, scope) {
            body = body.child(p);
        }
        body.into_any_element()
    }

    /// A picker: its face, and its list under it while it is open.
    #[allow(clippy::too_many_arguments)]
    fn picker(
        &mut self,
        name: &'static str,
        face: &str,
        which: AudioMenu,
        items: Vec<Choice>,
        scope: &mut RegistryScope,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let me = cx.entity().downgrade();
        let toggler = me.clone();
        let face_el = controls::menu_face(name, face, None, true, scope, move |_, cx| {
            toggler.update(cx, |t, cx| t.toggle_menu(which, cx)).ok();
        });
        let mut holder = div()
            .relative()
            .w(px(240.))
            .h(px(controls::menu_face::HEIGHT as f32))
            .child(face_el);
        if self.menu == Some(which) {
            let k = tokens();
            let size = window.viewport_size();
            let closing = me.clone();
            let backdrop = deferred(
                anchored().position(point(px(0.), px(0.))).child(
                    div()
                        .id("settings-menu-backdrop")
                        .w(size.width)
                        .h(size.height)
                        .occlude()
                        .on_any_mouse_down(move |_, _, cx| {
                            closing.update(cx, |t, cx| t.close_menu(cx)).ok();
                        }),
                ),
            )
            .with_priority(1);
            let mut list = crate::arrangement::menu_list("settings-menu");
            let mut inner = scope.container();
            for (n, c) in items.into_iter().enumerate() {
                let (closing, pick) = (me.clone(), c.pick.clone());
                list = list.child(
                    div()
                        .id(("settings-menu-item", n))
                        .relative()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.))
                        .px_3()
                        .py_1()
                        .text_xs()
                        .whitespace_nowrap()
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(k.surface_inset)))
                        .child(
                            div()
                                .w(px(10.))
                                .flex_none()
                                .when(c.checked, |d| d.child("\u{2713}")),
                        )
                        .child(c.title.clone())
                        .child(inner.control_selected("menuitem", &c.title, true, None, c.checked))
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            closing.update(cx, |t, cx| t.close_menu(cx)).ok();
                            pick(window, cx);
                        }),
                );
            }
            holder = holder.child(backdrop).child(
                div()
                    .absolute()
                    .top_full()
                    .left_0()
                    .child(deferred(anchored().snap_to_window().child(list)).with_priority(2)),
            );
        }
        holder.into_any_element()
    }

    fn toggle_menu(&mut self, which: AudioMenu, cx: &mut Context<Self>) {
        if self.menu == Some(which) {
            self.menu = None;
        } else {
            self.menu = Some(which);
            // The picker opens: the device list is read again.
            if which == AudioMenu::Device {
                self.read_devices(cx);
            }
        }
        cx.notify();
    }

    fn close_menu(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        cx.notify();
    }

    /// A stepper row: its label with the value, and the buttons.
    fn stepper_row(label: &str, buttons: AnyElement, scope: &mut RegistryScope) -> AnyElement {
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .child(div().flex_1().child(text(label, scope)))
            .child(buttons)
            .into_any_element()
    }

    /// The red line at a tab's bottom.
    fn problem_line(problem: Option<String>, scope: &mut RegistryScope) -> Option<AnyElement> {
        problem.map(|p| {
            div()
                .relative()
                .text_color(rgb(tokens().state_error))
                .child(p.clone())
                .child(scope.control_with("text", &p, true, Some("problem"), false))
                .into_any_element()
        })
    }

    fn recording_tab(
        &mut self,
        scope: &mut RegistryScope,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (st, problem, format, rates) = {
            let a = AppState::global(cx).read(cx);
            (
                a.audio.settings().clone(),
                a.audio.problem().map(str::to_owned),
                a.store.default_format(),
                a.store.core().project_rates(),
            )
        };

        // New Projects: the core's rates, and the current one if it is not
        // among them.
        let mut rate_list = rates;
        if !rate_list.contains(&format.sample_rate) {
            rate_list.push(format.sample_rate);
            rate_list.sort_unstable();
        }
        let rate_items = rate_list
            .into_iter()
            .map(|r| Choice {
                title: formats::rate_title(r),
                checked: r == format.sample_rate,
                pick: Rc::new(move |_, cx| set_format(cx, Some(r), None)),
            })
            .collect();
        let rate = self.picker(
            "Sample rate",
            &formats::rate_title(format.sample_rate),
            AudioMenu::Rate,
            rate_items,
            scope,
            window,
            cx,
        );
        let depth_items = formats::DEPTHS
            .iter()
            .map(|&(d, title)| Choice {
                title: title.to_owned(),
                checked: d == format.depth,
                pick: Rc::new(move |_, cx| set_format(cx, None, Some(d))),
            })
            .collect();
        let depth = self.picker(
            "Bit depth",
            formats::depth_title(format.depth),
            AudioMenu::Depth,
            depth_items,
            scope,
            window,
            cx,
        );

        // New Documents.
        let count_in = stepper_buttons(
            "Count-in",
            Range {
                min: 0,
                max: 4,
                step: 1,
            },
            i64::from(st.count_in_bars),
            scope,
            |v, _, cx| {
                Self::change(cx, move |a, p, core| a.set_count_in_bars(v as u8, p, core));
            },
        );
        let pre_roll = stepper_buttons(
            "Pre-roll",
            Range {
                min: 0,
                max: 8,
                step: 1,
            },
            i64::from(st.pre_roll_bars),
            scope,
            |v, _, cx| {
                Self::change(cx, move |a, p, core| a.set_pre_roll_bars(v as u8, p, core));
            },
        );
        let takes_on = st.midi_passes_as_takes;
        let takes = switch(
            "settings-midi-takes",
            "MIDI passes as takes",
            takes_on,
            true,
            scope,
            move |_, cx| {
                Self::change(cx, move |a, p, core| {
                    a.set_midi_passes_as_takes(!takes_on, p, core)
                });
            },
        );
        let monitor_items = [("auto", "Auto"), ("input", "Input"), ("off", "Off")]
            .into_iter()
            .map(|(id, title)| Choice {
                title: title.to_owned(),
                checked: id == st.default_monitor,
                pick: Rc::new(move |_, cx| {
                    Self::change(cx, move |a, p, core| a.set_default_monitor(id, p, core));
                }),
            })
            .collect();
        let face = match st.default_monitor.as_str() {
            "input" => "Input",
            "off" => "Off",
            _ => "Auto",
        };
        let monitor = self.picker(
            "Monitoring for new tracks",
            face,
            AudioMenu::Monitor,
            monitor_items,
            scope,
            window,
            cx,
        );

        let mut body = div().flex().flex_col().gap(px(6.)).p_4();
        body = body
            .child(section("New Projects", scope))
            .child(form_row("Sample rate", rate, scope))
            .child(form_row("Bit depth", depth, scope))
            .child(section("New Documents", scope))
            .child(Self::stepper_row(
                &format!("Count-in: {}", count_text(st.count_in_bars)),
                count_in,
                scope,
            ))
            .child(Self::stepper_row(
                &format!("Pre-roll: {}", count_text(st.pre_roll_bars)),
                pre_roll,
                scope,
            ))
            .child(takes)
            .child(caption(MIDI_TAKES_HELP, scope))
            .child(form_row("Monitoring for new tracks", monitor, scope))
            .child(caption(
                "Open documents keep their own options: change those in the transport bar's record popover.",
                scope,
            ))
            .child(section("Disk", scope))
            .child(caption(audio_prefs::DISK_TEXT, scope));
        if let Some(p) = Self::problem_line(problem, scope) {
            body = body.child(p);
        }
        body.into_any_element()
    }

    fn automation_tab(
        &mut self,
        scope: &mut RegistryScope,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (st, problem) = {
            let a = AppState::global(cx).read(cx);
            (
                a.automation.settings().clone(),
                a.automation.problem().map(str::to_owned),
            )
        };
        let title_of = |choices: &[(&str, &str)], id: &str| {
            choices
                .iter()
                .find(|(i, _)| *i == id)
                .map_or("", |(_, t)| *t)
                .to_owned()
        };
        let after_items = AFTER_WRITE_CHOICES
            .iter()
            .map(|&(id, title)| Choice {
                title: title.to_owned(),
                checked: id == st.after_write,
                pick: Rc::new(move |_, cx| {
                    Self::change_automation(cx, move |a, p, core| a.set_after_write(id, p, core));
                }),
            })
            .collect();
        let after = self.picker(
            "After Write",
            &title_of(&AFTER_WRITE_CHOICES, &st.after_write),
            AudioMenu::AfterWrite,
            after_items,
            scope,
            window,
            cx,
        );
        let ret = stepper_buttons(
            "Return time",
            Range {
                min: 0,
                max: i64::from(MAX_RETURN_MS),
                step: i64::from(RETURN_STEP),
            },
            i64::from(st.return_ms),
            scope,
            |v, _, cx| {
                Self::change_automation(cx, move |a, p, core| a.set_return_ms(v as u32, p, core));
            },
        );
        let thin_items = THINNING_CHOICES
            .iter()
            .map(|&(id, title)| Choice {
                title: title.to_owned(),
                checked: id == st.thinning,
                pick: Rc::new(move |_, cx| {
                    Self::change_automation(cx, move |a, p, core| a.set_thinning(id, p, core));
                }),
            })
            .collect();
        let thinning = self.picker(
            "Thinning",
            &title_of(&THINNING_CHOICES, &st.thinning),
            AudioMenu::Thinning,
            thin_items,
            scope,
            window,
            cx,
        );
        // Always on, so it cannot be turned off.
        let always = switch(
            "settings-params-only",
            PARAMS_ONLY_LABEL,
            true,
            false,
            scope,
            |_, _| {},
        );

        let mut body = div().flex().flex_col().gap(px(6.)).p_4();
        body = body
            .child(form_row("After Write", after, scope))
            .child(Self::stepper_row(&return_text(st.return_ms), ret, scope))
            .child(form_row("Thinning", thinning, scope))
            .child(caption(THINNING_HELP, scope))
            .child(always)
            .child(caption(PARAMS_ONLY_HELP, scope))
            .child(caption(APPLIES_TEXT, scope));
        if let Some(p) = Self::problem_line(problem, scope) {
            body = body.child(p);
        }
        body.into_any_element()
    }

    fn surfaces_tab(&mut self, scope: &mut RegistryScope) -> AnyElement {
        placeholder_body(SettingsTab::Surfaces, scope)
    }

    fn plugins_tab(&mut self, scope: &mut RegistryScope) -> AnyElement {
        placeholder_body(SettingsTab::Plugins, scope)
    }

    fn co_producer_tab(&mut self, scope: &mut RegistryScope) -> AnyElement {
        placeholder_body(SettingsTab::CoProducer, scope)
    }

    fn body(
        &mut self,
        scope: &mut RegistryScope,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match self.tab {
            SettingsTab::Appearance => self.appearance_tab(scope),
            SettingsTab::Editing => self.editing_tab(scope),
            SettingsTab::Keys => self.keys_tab(scope),
            SettingsTab::Audio => self.audio_tab(scope, window, cx),
            SettingsTab::Recording => self.recording_tab(scope, window, cx),
            SettingsTab::Automation => self.automation_tab(scope, window, cx),
            SettingsTab::Surfaces => self.surfaces_tab(scope),
            SettingsTab::Plugins => self.plugins_tab(scope),
            SettingsTab::CoProducer => self.co_producer_tab(scope),
        }
    }
}

/// File › New's default format from the Recording tab: `rate` and/or
/// `depth` replace the current ones.
fn set_format(cx: &mut App, rate: Option<u32>, depth: Option<vizdaw_ffi::BitDepthRow>) {
    AppState::global(cx).update(cx, |s, cx| {
        let cur = s.store.default_format();
        s.remember_default_format(vizdaw_ffi::ProjectFormatRow {
            sample_rate: rate.unwrap_or(cur.sample_rate),
            depth: depth.unwrap_or(cur.depth),
        });
        cx.notify();
    });
}

/// A switch row (Swift `Toggle`): the label, then the switch at the right;
/// `checkbox` in the tree, its value `on` or `off`. A disabled one takes
/// no clicks.
fn switch(
    id: &'static str,
    label: &'static str,
    on: bool,
    enabled: bool,
    scope: &mut RegistryScope,
    press: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let t = tokens();
    let mut row = div()
        .id(id)
        .relative()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.))
        .opacity(if enabled { 1. } else { 0.6 });
    if enabled {
        row = row
            .cursor_pointer()
            .on_click(move |_, window, cx| press(window, cx));
    }
    row.child(div().flex_1().child(label))
        .child(
            div()
                .w(px(34.))
                .h(px(18.))
                .rounded_full()
                .bg(rgb(if on { t.accent } else { t.surface_inset }))
                .border_1()
                .border_color(rgb(t.surface_separator))
                .flex()
                .flex_row()
                .when(on, |d| d.justify_end())
                .child(div().size(px(16.)).rounded_full().bg(rgb(t.surface_raised))),
        )
        .child(scope.control_with(
            "checkbox",
            label,
            enabled,
            Some(if on { "on" } else { "off" }),
            false,
        ))
        .into_any_element()
}

/// A line of text, in the tree as a `text`.
fn text(line: &str, scope: &mut RegistryScope) -> AnyElement {
    div()
        .relative()
        .child(line.to_owned())
        .child(scope.control("text", line, true))
        .into_any_element()
}

/// A section's title.
fn section(title: &str, scope: &mut RegistryScope) -> AnyElement {
    div()
        .mt_2()
        .font_weight(gpui::FontWeight::BOLD)
        .child(text(title, scope))
        .into_any_element()
}

/// A caption under a row, in the secondary colour.
fn caption(line: &str, scope: &mut RegistryScope) -> AnyElement {
    div()
        .text_size(px(11.))
        .text_color(rgb(tokens().text_secondary))
        .child(text(line, scope))
        .into_any_element()
}

/// A label, then its control at the right.
fn form_row(label: &str, control: AnyElement, scope: &mut RegistryScope) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.))
        .child(div().flex_1().child(text(label, scope)))
        .child(control)
        .into_any_element()
}

fn placeholder_body(tab: SettingsTab, scope: &mut RegistryScope) -> AnyElement {
    let t = tokens();
    let line = placeholder(tab);
    div()
        .relative()
        .p_4()
        .text_color(rgb(t.text_secondary))
        .child(line.clone())
        .child(scope.control("text", &line, true))
        .into_any_element()
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens();
        let mut scope = RegistryScope::root(&self.registry);
        let mut strip = scope.container();
        let mut tabs = div()
            .flex()
            .flex_row()
            // The nine labels wrap to a second row where a face is wide.
            .flex_wrap()
            .items_center()
            .gap_1()
            .p_1()
            .bg(rgb(t.surface_panel))
            .border_b_1()
            .border_color(rgb(t.surface_separator));
        for tab in SettingsTab::ALL {
            let on = tab == self.tab;
            tabs = tabs.child(
                div()
                    .id(tab.label())
                    .relative()
                    .px_2()
                    .py_1()
                    .rounded(px(t.radius))
                    .when_selected(on, t.accent, t.text_inverse, t.text_primary)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select(tab, window, cx);
                    }))
                    .child(tab.label())
                    .child(strip.control_selected("tab", tab.label(), true, None, on)),
            );
        }
        let body = self.body(&mut scope, window, cx);
        div()
            .id("settings-window")
            .track_focus(&self.focus)
            .key_context(SETTINGS_WINDOW_CONTEXT)
            .on_action(|_: &CloseWindow, window, _| window.remove_window())
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(t.surface_raised))
            .text_color(rgb(t.text_primary))
            .text_size(px(13.))
            .child(tabs)
            .child(body)
    }
}

trait Selected: Styled + Sized {
    /// The selected tab's fill and text, else the plain text colour.
    fn when_selected(self, on: bool, fill: u32, on_text: u32, text: u32) -> Self {
        if on {
            self.bg(rgb(fill)).text_color(rgb(on_text))
        } else {
            self.text_color(rgb(text))
        }
    }
}

impl<T: Styled + Sized> Selected for T {}

/// Opens the Settings window on `tab` (`None`: the tab it was on, else
/// Appearance), or brings the open one to the front. Call it outside
/// `AppState::update`.
pub fn open_settings(tab: Option<SettingsTab>, cx: &mut App) {
    let app = AppState::global(cx);
    if let Some(h) = app.read(cx).settings_window {
        let front = h.update(cx, |this, window, cx| {
            if let Some(tab) = tab {
                this.select(tab, window, cx);
            }
            window.activate_window();
        });
        if front.is_ok() {
            return;
        }
        // The window is gone and its handle stale: forget it, open anew.
        app.update(cx, |s, _| s.settings_window = None);
    }
    let tab = tab.unwrap_or(SettingsTab::Appearance);
    let bounds = Bounds::centered(None, size(px(WIDTH), px(tab.height() + STRIP_HEIGHT)), cx);
    let opened = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Settings".into()),
                ..Default::default()
            }),
            is_resizable: false,
            ..Default::default()
        },
        move |window, cx| cx.new(|cx| SettingsWindow::new(tab, window, cx)),
    );
    match opened {
        Ok(h) => {
            app.update(cx, |s, cx| {
                s.settings_window = Some(h);
                cx.notify();
            });
        }
        Err(e) => {
            app.update(cx, |s, cx| {
                s.store.post_alert(vizdaw_app::alerts::AlertItem::new(
                    "Window Failed",
                    e.to_string(),
                ));
                cx.notify();
            });
            crate::alerts::present(cx);
        }
    }
}
