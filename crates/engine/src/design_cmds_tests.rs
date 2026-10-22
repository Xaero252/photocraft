use serde_json::{Value, json};

use crate::Session;

pub(super) fn steps(s: &mut Session) -> Vec<Value> {
    s.execute("design.list", json!({})).unwrap()["steps"].as_array().unwrap().clone()
}

pub(super) fn commands(s: &mut Session) -> Vec<String> {
    steps(s).iter().map(|st| st["command"].as_str().unwrap().to_string()).collect()
}

fn started() -> Session {
    let mut s = Session::new();
    s.execute("file.new", json!({"width": 16, "height": 16})).unwrap();
    s.execute("design.start", json!({})).unwrap();
    s
}

#[test]
fn records_document_changes_only() {
    let mut s = Session::new();
    s.execute("file.new", json!({"width": 16, "height": 16})).unwrap();
    s.execute("layer.new.layer", json!({"name": "before"})).unwrap();
    s.execute("design.start", json!({})).unwrap();
    assert!(steps(&mut s).is_empty(), "steps before the start aren't recorded");
    s.execute("layer.new.layer", json!({"name": "A"})).unwrap();
    let a = s.active().unwrap().active_layer.unwrap();
    s.execute("layer.select", json!({"layer": "before"})).unwrap();
    s.execute("view.zoomIn", json!({})).ok();
    s.execute("layer.setProps", json!({"opacity": 0.5})).unwrap();
    assert_eq!(commands(&mut s), ["layer.new.layer", "layer.setProps"], "navigation isn't a step");
    let st = steps(&mut s);
    assert_eq!(st[0]["created"], json!([a.0]));
    let before = s.active().unwrap().active_layer.unwrap();
    assert_eq!(st[1]["changed"], json!([before.0]));
    assert_ne!(st[0]["step"], st[1]["step"]);
    assert_eq!(photocraft_doc::id_step(a.0), st[0]["step"].as_u64(), "the step's id scope minted its layer");
}

#[test]
fn records_the_targets_and_the_tool_state_before_a_step() {
    let mut s = started();
    s.execute("layer.new.layer", json!({"name": "A"})).unwrap();
    let a = s.active().unwrap().active_layer.unwrap();
    s.execute("tools.setColors", json!({"foreground": [1.0, 0.0, 0.0, 1.0]})).unwrap();
    s.execute("select.all", json!({})).unwrap();
    s.execute("edit.fillForeground", json!({})).unwrap();
    let st = steps(&mut s);
    let fill = st.last().unwrap();
    assert_eq!(fill["command"], "edit.fillForeground");
    assert_eq!(fill["changed"], json!([a.0]));
    let tl = s.active().unwrap().doc.design_timeline.clone().unwrap();
    let rec = tl.steps.last().unwrap();
    assert_eq!(rec.active, Some(a));
    assert_eq!(rec.selected, vec![a]);
    let select = &tl.steps[tl.steps.len() - 2];
    assert_eq!(select.prelude.iter().map(|p| p.command.as_str()).collect::<Vec<_>>(), ["tools.setColors"]);
    assert!(rec.prelude.is_empty(), "a prelude goes with the next step only");
}

#[test]
fn undo_removes_the_step_and_redo_restores_it() {
    let mut s = started();
    s.execute("layer.new.layer", json!({})).unwrap();
    s.execute("layer.setProps", json!({"opacity": 0.25})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(commands(&mut s), ["layer.new.layer"]);
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(commands(&mut s), ["layer.new.layer", "layer.setProps"]);
    s.execute("design.stop", json!({})).unwrap();
    assert!(s.execute("design.list", json!({})).is_err());
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(commands(&mut s).len(), 2, "stopping is undoable");
}

#[test]
fn coalesced_edits_join_the_previous_step() {
    let mut s = started();
    s.execute("layer.new.layer", json!({})).unwrap();
    s.execute("layer.setProps", json!({"opacity": 0.5, "coalesce": "drag"})).unwrap();
    s.execute("layer.setProps", json!({"opacity": 0.4, "coalesce": "drag"})).unwrap();
    let st = steps(&mut s);
    assert_eq!(st.iter().map(|x| x["joinsPrevious"].as_bool().unwrap()).collect::<Vec<_>>(), [false, false, true]);
}

#[test]
fn a_layer_named_in_params_is_recorded_by_id() {
    let mut s = started();
    s.execute("layer.new.layer", json!({"name": "A"})).unwrap();
    let a = s.active().unwrap().active_layer.unwrap();
    s.execute("layer.setProps", json!({"layer": "A", "visible": false})).unwrap();
    assert_eq!(steps(&mut s).last().unwrap()["params"]["layer"], json!(a.0));
}
