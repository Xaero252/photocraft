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
}

/// Commands that change no document but feed the steps after them (tool colours and options).
/// Only these are kept as a step's prelude: anything else that changes no document (saving,
/// undo, navigation) must never run again on replay.
const PRELUDE: &[&str] = &["tools.", "tool.", "brush.", "gradient.", "pattern.", "cloneSource.", "paint.", "type.", "shape.", "swatches.", "color."];

/// Every document with a timeline, before a command runs (empty when none has one).
pub(crate) fn capture(s: &Session) -> Vec<Before> {
    s.docs
        .iter()
        .filter(|st| st.doc.design_timeline.is_some())
        .map(|st| Before {
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
            baked: None,
            state: StepState::Ok,
            checkpoint: Checkpoint(Some(st.doc.clone())),
        };
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
