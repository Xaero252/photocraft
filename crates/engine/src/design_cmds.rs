//! Design timeline commands: turn a document's timeline on or off and list its steps. Each
//! change is an ordinary undoable history step.

use std::sync::Arc;

use photocraft_doc::design::DesignTimeline;
use serde_json::{Value, json};

use crate::commands::CommandSpec;
use crate::{EngineError, Result, Session};

fn timeline_of(s: &Session) -> std::result::Result<&Arc<DesignTimeline>, String> {
    s.active().ok_or("no document open")?.doc.design_timeline.as_ref().ok_or_else(|| "the design timeline is off".into())
}

fn can_start(s: &Session) -> std::result::Result<(), String> {
    match s.active().ok_or("no document open")?.doc.design_timeline {
        Some(_) => Err("the design timeline is already on".into()),
        None => Ok(()),
    }
}

fn start(s: &mut Session, _: &Value) -> Result<Value> {
    can_start(s).map_err(EngineError::Other)?;
    s.edit("Start Design Timeline", |doc, _| {
        doc.design_timeline = Some(Arc::new(DesignTimeline::new(Arc::new(doc.clone()))));
        Ok(())
    })?;
    Ok(json!({"started": true}))
}

fn stop(s: &mut Session, _: &Value) -> Result<Value> {
    timeline_of(s).map_err(EngineError::Other)?;
    s.edit("Stop Design Timeline", |doc, _| {
        doc.design_timeline = None;
        Ok(())
    })?;
    Ok(json!({"stopped": true}))
}

fn list(s: &mut Session, _: &Value) -> Result<Value> {
    let tl = timeline_of(s).map_err(EngineError::Other)?;
    let steps: Vec<Value> = tl
        .steps
        .iter()
        .map(|st| {
            json!({
                "step": st.step, "command": st.command, "label": st.label, "params": st.params,
                "suppressed": st.suppressed, "joinsPrevious": st.joins_previous, "baked": st.baked.is_some(),
                "created": st.created, "changed": st.changed, "state": st.state,
            })
        })
        .collect();
    Ok(json!({"steps": steps, "marker": tl.marker, "inEffect": tl.in_effect()}))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "design.start",
            label: "Start Design Timeline",
            menu: &[],
            shortcut: None,
            params: "{}",
            enabled: can_start,
            run: start,
            journal: true,
        },
        CommandSpec {
            id: "design.stop",
            label: "Stop Design Timeline",
            menu: &[],
            shortcut: None,
            params: "{}",
            enabled: |s| timeline_of(s).map(|_| ()),
            run: stop,
            journal: true,
        },
        CommandSpec {
            id: "design.list",
            label: "List Design Timeline Steps",
            menu: &[],
            shortcut: None,
            params: "{} → {steps:[{step,command,label,params,suppressed,joinsPrevious,baked,created,changed,state}],marker,inEffect}",
            enabled: |s| timeline_of(s).map(|_| ()),
            run: list,
            journal: false,
        },
    ]
}

#[cfg(test)]
#[path = "design_cmds_tests.rs"]
mod tests;
