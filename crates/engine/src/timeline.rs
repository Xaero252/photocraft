//! Recording into a document's design timeline ([`photocraft_doc::design`]): every outermost
//! command that changes a document with a timeline becomes a step, with the targets it acted on
//! and the layers it created and changed.

use std::sync::Arc;

use photocraft_doc::design::{Checkpoint, DesignStep, Prelude, StepState, layer_changes};
use photocraft_doc::{DocId, Document, LayerId};
use serde_json::Value;

use crate::Session;

/// A document with a timeline as it was before a command.
pub(crate) struct Before {
    doc: DocId,
    state: Arc<Document>,
    active: Option<LayerId>,
    selected: Vec<LayerId>,
    history_row: Option<u64>,
    /// The command started in this document (not another one, like Save Contents in the child).
    was_active: bool,
}

/// Commands that change no document but feed the steps after them (tool colours and options).
/// Only these are kept as a step's prelude: anything else that changes no document (saving,
/// undo, navigation) must never run again on replay.
const PRELUDE: &[&str] = &["tools.", "tool.", "brush.", "gradient.", "pattern.", "cloneSource.", "paint.", "type.", "shape.", "swatches.", "color."];

/// Commands that don't replay identically (found by the `design_replay_audit` test): their steps
/// keep the layers they produced and replay those instead of running again. Most read session
/// state a replay session doesn't have (the clipboards, the Fade source, the journal), and copying today's
/// clipboard would paste something other than what was pasted when the step was recorded.
pub const BAKED: &[&str] = &[
    "select.reselect",
    "edit.paste",
    "edit.pasteSpecial.pasteInPlace",
    "edit.pasteSpecial.pasteInto",
    "edit.pasteSpecial.pasteOutside",
    "layer.layerStyle.pasteLayerStyle",
    "path.style.pasteFill",
    "path.style.pasteStroke",
    "edit.fade",
    "filter.lastFilter",
    "edit.transform.again",
    "edit.transform.againCopy",
];

/// Whether a step of `id` run with `params` keeps its result instead of running again on replay.
fn bakes(id: &str, params: &Value) -> bool {
    // A Vanishing Point paste without a `layer` pastes the clipboard.
    let vp_clip = || params.get("paste").and_then(Value::as_array).is_some_and(|ps| ps.iter().any(|p| p.get("layer").is_none()));
    BAKED.contains(&id) || (id == crate::vp_cmds::VP && vp_clip())
}

/// Every document with a timeline, before a command runs (empty when none has one).
pub(crate) fn capture(s: &Session) -> Vec<Before> {
    let active = s.active().map(|st| st.doc.id);
    s.docs
        .iter()
        .filter(|st| st.doc.design_timeline.is_some())
        .map(|st| Before {
            was_active: active == Some(st.doc.id),
            doc: st.doc.id,
            state: st.doc.clone(),
            active: st.active_layer,
            selected: st.selected_layers.clone(),
            history_row: st.history.row_id(st.history.past_len()),
        })
        .collect()
}

/// Record command `id` (run under id-scope `step`) into each document of `before` it changed.
pub(crate) fn record(s: &mut Session, before: Vec<Before>, step: u64, id: &str, params: &Value) {
    // Queries and view-only commands (video frame navigation) aren't steps.
    if before.is_empty() || id.starts_with("design.") || !crate::commands::find(id).is_some_and(|c| c.journal) {
        return;
    }
    let mut recorded = false;
    for b in before {
        let Some(st) = s.docs.iter_mut().find(|st| st.doc.id == b.doc) else { continue };
        if Arc::ptr_eq(&st.doc, &b.state) {
            continue;
        }
        // Undo, redo and revert swap in another state with its own timeline: not a new step.
        let same_timeline = matches!((&st.doc.design_timeline, &b.state.design_timeline), (Some(a), Some(b)) if Arc::ptr_eq(a, b));
        if !same_timeline {
            continue;
        }
        let (created, changed) = layer_changes(&b.state, &st.doc);
        // A command run from another document (Save Contents, closing an Edit Contents document)
        // can't run again here: keep what it did.
        let baked = (bakes(id, params) || !b.was_active).then(|| Arc::new(photocraft_doc::design::Baked::capture(&b.state, &st.doc, &created, &changed)));
        let entry = DesignStep {
            step,
            command: id.to_string(),
            params: resolve_names(&b.state, params),
            label: st.history.undo_label().unwrap_or(id).to_string(),
            prelude: s.timeline_prelude.clone(),
            active: b.active,
            selected: b.selected,
            created,
            changed,
            suppressed: false,
            joins_previous: st.history.row_id(st.history.past_len()) == b.history_row,
            baked,
            rerecord: false,
            state: StepState::Ok,
            // Without its timeline: that would hold every earlier checkpoint out of the budget's reach.
            checkpoint: Checkpoint(Some(Arc::new(Document { design_timeline: None, ..(*st.doc).clone() }))),
        };
        let current = st.doc.clone();
        let doc = Arc::make_mut(&mut st.doc);
        let Some(tl) = doc.design_timeline.as_mut() else { continue };
        let tl = Arc::make_mut(tl);
        let at = tl.in_effect();
        tl.steps.insert(at, entry);
        if tl.marker.is_some() {
            tl.marker = Some(at + 1);
            // Steps after an inserted one replay from it when rolled forward.
            for later in tl.steps.iter_mut().skip(at + 1) {
                later.checkpoint = Checkpoint(None);
            }
        }
        budget_checkpoints(&st.history, &current, tl);
        recorded = true;
    }
    if recorded {
        s.timeline_prelude.clear();
    } else if PRELUDE.iter().any(|p| id.starts_with(p)) {
        s.timeline_prelude.push(Prelude { command: id.to_string(), params: params.clone() });
    }
}

/// `params` with a `"layer"` given by name replaced by that layer's id, so replay doesn't pick
/// another layer that later took the name.
fn resolve_names(doc: &Document, params: &Value) -> Value {
    let mut p = params.clone();
    if let Some(slot) = p.get_mut("layer")
        && let Some(name) = slot.as_str()
        && let Some((_, _, l)) = doc.walk().into_iter().find(|(_, _, l)| l.name == name)
    {
        *slot = Value::from(l.id.0);
    }
    p
}

/// A timeline replayed from some step on.
pub(crate) struct Replayed {
    pub doc: Document,
    pub timeline: photocraft_doc::design::DesignTimeline,
    pub active: Option<LayerId>,
    pub selected: Vec<LayerId>,
}

/// A session to replay in: `doc` alone, with this session's tool, preset and preference state,
/// so the live session's history, jobs and journal are untouched.
fn scratch(s: &Session, doc: &Document) -> Session {
    let mut r = Session::new();
    r.tools = s.tools.clone();
    r.prefs = s.prefs.clone();
    r.patterns = s.patterns.clone();
    r.presets = s.presets.clone();
    r.type_defaults = s.type_defaults.clone();
    r.quick_mask_options = s.quick_mask_options;
    r.authorize = s.authorize;
    let mut doc = doc.clone();
    doc.design_timeline = None;
    r.add_document(doc, None);
    r
}

/// The document `tl` produces from the first step at or before `from` with a checkpoint to the
/// marker, with every replayed step's outcome and checkpoint updated.
/// Baked steps after `changed` (the step the change was made at) are marked stale.
pub(crate) fn replay(s: &Session, tl: &photocraft_doc::design::DesignTimeline, from: usize, changed: Option<usize>) -> crate::Result<Replayed> {
    let mut tl = tl.clone();
    let end = tl.in_effect();
    let mut start = from.min(end);
    while start > 0 && tl.steps.get(start - 1).is_none_or(|st| st.checkpoint.0.is_none()) {
        start -= 1;
    }
    let resume = match start {
        0 => tl.base.clone(),
        k => tl
            .steps
            .get(k - 1)
            .and_then(|st| st.checkpoint.0.clone())
            .ok_or_else(|| crate::EngineError::Other("internal error: no checkpoint to replay from".into()))?,
    };
    let mut r = scratch(s, &resume);
    for (i, step) in tl.steps.iter_mut().enumerate().take(end).skip(start) {
        let stale = step.baked.is_some() && (step.state == StepState::Stale || changed.is_some_and(|c| i > c));
        run_step(&mut r, step);
        if stale && !step.state.is_failure() {
            step.state = StepState::Stale;
        }
    }
    // Steps past the marker resume from it when rolled forward.
    for later in tl.steps.iter_mut().skip(end) {
        later.checkpoint = Checkpoint(None);
    }
    let st = r.active().ok_or(crate::EngineError::NoDocument)?;
    Ok(Replayed { doc: (*st.doc).clone(), active: st.active_layer, selected: st.selected_layers.clone(), timeline: tl })
}

/// Replay one step in `r`, setting its outcome and checkpoint.
fn run_step(r: &mut Session, step: &mut DesignStep) {
    let Some(pre) = r.active().map(|st| st.doc.clone()) else { return };
    let finish = |r: &mut Session, step: &mut DesignStep, state: StepState| {
        step.state = state;
        step.checkpoint = Checkpoint(r.active().map(|st| st.doc.clone()));
    };
    if step.suppressed {
        return finish(r, step, StepState::Ok);
    }
    let missing: Vec<LayerId> = step.changed.iter().copied().filter(|id| pre.layer(*id).is_none()).collect();
    if !missing.is_empty() {
        return finish(r, step, StepState::MissingReference { layers: missing });
    }
    let placed_elsewhere = step.active.is_some_and(|a| pre.layer(a).is_none()) && !step.created.is_empty();
    for p in &step.prelude {
        // A tool setting that no longer applies leaves the tool as it is.
        let _ = r.execute(&p.command, p.params.clone());
    }
    if let Some(st) = r.active_mut() {
        st.active_layer = step.active.filter(|a| pre.layer(*a).is_some());
        st.selected_layers = step.selected.iter().copied().filter(|id| pre.layer(*id).is_some()).collect();
        st.layer_anchor = st.active_layer;
    }
    let ran = match step.baked.clone().filter(|_| !step.rerecord) {
        Some(baked) => apply_baked(r, step, &baked),
        None => {
            let _scope = photocraft_doc::IdScope::new(step.step).enter();
            r.execute(&step.command, step.params.clone()).map(|_| ())
        }
    };
    let restore = |r: &mut Session| {
        if let Some(st) = r.active_mut() {
            st.doc = pre.clone();
        }
    };
    if let Err(e) = ran {
        restore(r);
        return finish(r, step, StepState::Error { message: e.to_string() });
    }
    let Some(post) = r.active().map(|st| st.doc.clone()) else { return };
    let (created, changed) = layer_changes(&pre, &post);
    if step.rerecord {
        if step.baked.is_some() {
            step.baked = Some(Arc::new(photocraft_doc::design::Baked::capture(&pre, &post, &created, &changed)));
        }
        (step.created, step.changed, step.rerecord) = (created, changed, false);
        return finish(r, step, StepState::Ok);
    }
    let foreign: Vec<LayerId> = changed.into_iter().filter(|id| !step.changed.contains(id)).collect();
    if !foreign.is_empty() {
        restore(r);
        return finish(r, step, StepState::Retargeted { layers: foreign });
    }
    finish(r, step, if placed_elsewhere { StepState::PlacedElsewhere } else { StepState::Ok });
}

fn apply_baked(r: &mut Session, step: &DesignStep, baked: &photocraft_doc::design::Baked) -> crate::Result<()> {
    let st = r.active_mut().ok_or(crate::EngineError::NoDocument)?;
    let mut doc = (*st.doc).clone();
    baked.apply(&mut doc, &step.created, &step.changed);
    st.doc = Arc::new(doc);
    Ok(())
}

/// Drop the oldest checkpoints while the pixels only they hold exceed a quarter of the history
/// budget. The base and the newest checkpoint are kept.
pub(crate) fn budget_checkpoints(history: &photocraft_ops::History, current: &Document, tl: &mut photocraft_doc::design::DesignTimeline) {
    let cap = history.max_bytes / 4;
    loop {
        let held: Vec<&Document> = tl.steps.iter().filter_map(|st| st.checkpoint.0.as_deref()).collect();
        if held.len() <= 1 || history.extra_bytes(current, &held) <= cap {
            return;
        }
        if let Some(oldest) = tl.steps.iter_mut().find(|st| st.checkpoint.0.is_some()) {
            oldest.checkpoint = Checkpoint(None);
        }
    }
}
