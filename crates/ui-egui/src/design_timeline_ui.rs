//! Window › Design Timeline: a strip of the active document's design timeline steps (engine
//! `design.*`). Click a step to roll the marker to just after it; right-click to suppress or delete
//! it; drag the marker slider to scrub. Failed steps are red, steps placed elsewhere or baked steps
//! gone stale are yellow, suppressed steps are struck through. The panel only reads state: every
//! action runs a `design.*` command, so the control channel and MCP drive it the same way.

use egui::{RichText, vec2};
use photocraft_doc::design::StepState;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::PhotocraftApp;
use crate::theme::Tokens;

pub const ID: &str = "window.panel.designTimeline";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DesignTimelineUi {
    pub open: bool,
}

pub fn checked(app: &PhotocraftApp, id: &str) -> Option<bool> {
    (id == ID).then_some(app.ui.design_timeline.open)
}

pub fn handles(id: &str) -> bool {
    id == ID
}

pub fn menu(app: &mut PhotocraftApp, id: &str, _params: &Value) -> Option<Result<Value, String>> {
    (id == ID).then(|| {
        app.ui.design_timeline.open = !app.ui.design_timeline.open;
        Ok(json!({ "open": app.ui.design_timeline.open }))
    })
}

/// Hover text for a step's state, and whether it is a failure (red) or a warning (yellow).
fn describe(state: &StepState) -> (Option<String>, Option<bool>) {
    match state {
        StepState::Ok => (None, None),
        StepState::Error { message } => (Some(message.clone()), Some(true)),
        StepState::MissingReference { .. } => (Some(tl!("A layer this step acted on was removed.").into()), Some(true)),
        StepState::Retargeted { .. } => (Some(tl!("This step would now change a layer it didn't change before, so it was skipped.").into()), Some(true)),
        StepState::PlacedElsewhere => (Some(tl!("The layer this step placed its new layers beside was removed; they went elsewhere.").into()), Some(false)),
        StepState::Stale => (Some(tl!("An earlier step changed, so this step's stored result may be out of date.").into()), Some(false)),
    }
}

pub fn windows(app: &mut PhotocraftApp, ctx: &egui::Context) {
    if !app.ui.design_timeline.open {
        return;
    }
    let have_doc = app.session.active().is_some();
    let tl = app.session.active().and_then(|d| d.doc.design_timeline.clone());
    let mut act: Option<(&str, Value)> = None;
    let mut close = false;
    let t = Tokens::get(ctx);

    crate::analysis_ui::panel_window(app, ctx, "design_timeline", tl!("Design Timeline"), vec2(0.0, 600.0), 720.0, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(tl!("Design Timeline")).strong().color(t.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::icons::button(ui, "x", 20.0, false, tl!("Close")).clicked() {
                    close = true;
                }
                if tl.is_some() && crate::icons::button(ui, "trash", 20.0, false, tl!("Stop Design Timeline")).clicked() {
                    act = Some(("design.stop", json!({})));
                }
            });
        });
        ui.separator();
        if !have_doc {
            ui.weak(tl!("Open a document to use the design timeline."));
            return;
        }
        let Some(tl) = &tl else {
            ui.add_space(8.0);
            ui.vertical_centered(|ui| {
                if crate::widgets::primary_button(ui, tl!("Start Design Timeline"), 220.0).clicked() {
                    act = Some(("design.start", json!({})));
                }
                ui.add_space(4.0);
                ui.weak(tl!("Records each edit from now on, so you can suppress, delete or edit any of them later."));
            });
            return;
        };
        if tl.steps.is_empty() {
            ui.weak(tl!("No steps yet: edits to this document appear here."));
            return;
        }
        let in_effect = tl.in_effect();
        egui::ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal(|ui| {
                for (i, st) in tl.steps.iter().enumerate() {
                    if i == in_effect {
                        ui.separator();
                    }
                    let (why, failure) = describe(&st.state);
                    let mut text = RichText::new(&st.label);
                    text = match failure {
                        Some(true) => text.color(t.danger),
                        Some(false) => text.color(t.warning),
                        None if i >= in_effect => text.color(t.text_faint),
                        None => text.color(t.text),
                    };
                    if st.suppressed {
                        text = text.strikethrough();
                    }
                    let mut hover = st.label.clone();
                    if let Some(why) = &why {
                        hover = format!("{hover}\n{why}");
                    }
                    let r = ui.add(egui::Button::new(text).selected(i + 1 == in_effect)).on_hover_text(hover);
                    if r.clicked() {
                        act = Some(("design.rollTo", json!({ "at": i + 1 })));
                    }
                    r.context_menu(|ui| {
                        let (cmd, label) = if st.suppressed { ("design.unsuppress", tl!("Unsuppress")) } else { ("design.suppress", tl!("Suppress")) };
                        if ui.button(label).clicked() {
                            act = Some((cmd, json!({ "step": st.step })));
                            ui.close();
                        }
                        if ui.button(tl!("Delete")).clicked() {
                            act = Some(("design.delete", json!({ "step": st.step })));
                            ui.close();
                        }
                        if ui.button(tl!("Roll Back to Here")).clicked() {
                            act = Some(("design.rollTo", json!({ "at": i + 1 })));
                            ui.close();
                        }
                    });
                }
            });
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let mut at = in_effect as f64;
            let n = tl.steps.len();
            let r = ui.add(egui::Slider::new(&mut at, 0.0..=n as f64).integer().show_value(false).text(tl!("Marker")));
            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                let at = at.round() as usize;
                act = Some(("design.rollTo", json!({ "at": if at >= n { Value::Null } else { json!(at) } })));
            }
            if in_effect < n && ui.button(tl!("Roll to End")).clicked() {
                act = Some(("design.rollTo", json!({ "at": null })));
            }
        });
    });

    if close {
        app.ui.design_timeline.open = false;
    }
    if let Some((cmd, p)) = act
        && let Err(e) = app.run(cmd, p)
    {
        app.ui.status = e;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};

    fn app() -> PhotocraftApp {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"width": 64, "height": 48})).unwrap();
        app.ui.design_timeline.open = true;
        app.last_canvas_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 800.0));
        app
    }

    fn harness(mut app: PhotocraftApp) -> Harness<'static, PhotocraftApp> {
        // The Start button uses the app's own font families, which apply from the next frame:
        // open the panel only once they're in.
        app.ui.design_timeline.open = false;
        let mut h = Harness::builder().with_size(vec2(1000.0, 800.0)).build_ui_state(|ui, app| windows(app, ui.ctx()), app);
        crate::theme::install_fonts(&h.ctx);
        h.run_steps(1);
        h.state_mut().ui.design_timeline.open = true;
        h.run_steps(2);
        h
    }

    fn timeline(h: &Harness<'_, PhotocraftApp>) -> Option<std::sync::Arc<photocraft_doc::design::DesignTimeline>> {
        h.state().session.active().and_then(|d| d.doc.design_timeline.clone())
    }

    #[test]
    fn start_record_roll_back_and_forward() {
        let mut h = harness(app());
        h.get_by_label("Start Design Timeline").click();
        h.run_steps(2);
        assert!(timeline(&h).is_some());
        h.state_mut().run("layer.new.layer", json!({"name": "A"})).unwrap();
        h.state_mut().run("layer.new.layer", json!({"name": "B"})).unwrap();
        h.run_steps(2);
        let labels: Vec<String> = timeline(&h).unwrap().steps.iter().map(|s| s.label.clone()).collect();
        assert_eq!(labels.len(), 2);
        // Clicking the first step rolls the marker to just after it.
        h.get_all_by_label(&labels[0]).next().unwrap().click();
        h.run_steps(2);
        assert_eq!(timeline(&h).unwrap().in_effect(), 1);
        h.get_by_label("Roll to End").click();
        h.run_steps(2);
        assert_eq!(timeline(&h).unwrap().in_effect(), 2);
    }

    #[test]
    fn a_failed_step_shows_why() {
        let mut a = app();
        a.run("design.start", json!({})).unwrap();
        a.run("layer.new.layer", json!({"name": "A"})).unwrap();
        a.run("layer.setProps", json!({"opacity": 0.5})).unwrap();
        let first = a.session.active().unwrap().doc.design_timeline.as_ref().unwrap().steps[0].step;
        a.run("design.suppress", json!({"step": first})).unwrap();
        let tl = a.session.active().unwrap().doc.design_timeline.clone().unwrap();
        assert!(tl.steps[1].state.is_failure());
        let (why, failure) = describe(&tl.steps[1].state);
        assert_eq!(failure, Some(true));
        assert!(why.is_some());
        let mut h = harness(a);
        h.run_steps(1);
        assert!(h.query_by_label("Roll to End").is_none(), "nothing is rolled back");
    }
}
