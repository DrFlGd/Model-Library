//! Recoverable deletion of complete model folders (including sidecars and previews).
//! Files are renamed into the library's journal on the same filesystem; no data is erased.
use crate::{extract, index::Index, library::Library, model, relayout};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::{path::Path, sync::atomic::{AtomicBool, Ordering}};

fn s<'a>(v: &'a Value, key: &str) -> &'a str { v[key].as_str().unwrap_or("") }
fn stopped(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) { bail!("Stopped."); }
    Ok(())
}

pub fn plan(lib: &Library, ix: &Index, args: &Value) -> Result<Value> {
    let ids = args["ids"].as_array().ok_or_else(|| anyhow!("Select models to delete."))?;
    if ids.is_empty() { bail!("Select models to delete."); }
    let mut seen = std::collections::HashSet::new();
    let mut models = vec![];
    for id in ids {
        let id = id.as_str().ok_or_else(|| anyhow!("Invalid model reference."))?;
        if !seen.insert(id.to_owned()) { continue; }
        let m = ix.get(id).ok_or_else(|| anyhow!("A selected model is missing. Read the library again."))?;
        let dir = extract::checked(lib.root(), m.rel())?;
        if !dir.is_dir() { bail!("The model folder is missing."); }
        let files = model::list_files(&dir);
        let bytes: u64 = files.iter().map(|(_,n)| *n).sum();
        models.push(json!({"id":id,"name":m.v["name"],"rel":m.rel(),"count":files.len(),"bytes":bytes,"manifest":extract::hashes(&dir)?}));
    }
    // Prevent accidental nested model folders in one deletion plan.
    for (i,a) in models.iter().enumerate() {
        for b in models.iter().skip(i+1) {
            let (a,b) = (s(a,"rel"), s(b,"rel"));
            if a.starts_with(&format!("{b}/")) || b.starts_with(&format!("{a}/")) {
                bail!("Nested model folders cannot be deleted together.");
            }
        }
    }
    Ok(json!({"kind":"model_delete","operation_id":model::new_id(),"models":models,
        "count":models.len(),"bytes":models.iter().map(|m|m["bytes"].as_u64().unwrap_or(0)).sum::<u64>(),
        "reversible":true,"cleanup":"The complete model folders are kept in recovery storage with no automatic expiry."}))
}

pub fn execute(lib: &Library, plan: &Value, cancel: &AtomicBool, progress: &dyn Fn(usize,usize,&str)) -> Result<Value> {
    lib.writable()?;
    let items=plan["models"].as_array().ok_or_else(||anyhow!("Invalid delete plan."))?;
    for m in items {
        stopped(cancel)?;
        let source=extract::checked(lib.root(),s(m,"rel"))?;
        if !source.is_dir() || extract::hashes(&source)? != m["manifest"] {
            bail!("{} changed after review. Review the deletion again.",s(m,"name"));
        }
    }
    let jid=relayout::new_id(lib)?;
    let recovery=relayout::kept_dir(lib,&jid).join("models");
    std::fs::create_dir_all(&recovery)?;
    let mut j=plan.clone();
    j["id"]=json!(jid); j["kind"]=json!("model_delete");
    j["label"]=json!(format!("Deleted {} {}",items.len(),if items.len()==1 {"model"} else {"models"}));
    j["created"]=json!(crate::library::now()); j["state"]=json!("running");
    j["moves"]=json!(items.iter().enumerate().map(|(i,m)|json!({"from":m["rel"],"to":format!("_library/journal/{jid}/models/{i}")})).collect::<Vec<_>>());
    j["completed"]=json!([]);
    relayout::write(lib,&j)?;
    let result=(||->Result<()> {
        for (i,m) in items.iter().enumerate() {
            stopped(cancel)?;
            progress(i,items.len(),s(m,"name"));
            let from=extract::checked(lib.root(),s(m,"rel"))?;
            if extract::hashes(&from)? != m["manifest"] { bail!("{} changed during deletion.",s(m,"name")); }
            // Record the intended next move before it happens for interruption recovery.
            j["in_progress"]=json!(i);
            relayout::write(lib,&j)?;
            std::fs::rename(&from,recovery.join(i.to_string()))?;
            j["completed"].as_array_mut().unwrap().push(json!(i));
            j["in_progress"]=Value::Null;
            relayout::write(lib,&j)?;
        }
        Ok(())
    })();
    if let Err(err)=result {
        match restore(lib,&j,false) {
            Ok(()) => { j["state"]=json!("undone"); relayout::write(lib,&j)?; }
            Err(recovery_error) => {
                j["state"]=json!("stopped");
                j["error"]=json!(format!("{err}; recovery: {recovery_error}"));
                relayout::write(lib,&j)?;
                bail!("{err}; recovery requires attention: {recovery_error}");
            }
        }
        return Err(err);
    }
    j["state"]=json!("done"); j["in_progress"]=Value::Null;
    j["finished"]=json!(crate::library::now()); relayout::write(lib,&j)?;
    progress(items.len(),items.len(),"Complete");
    Ok(json!({"journal":jid,"deleted":items.len(),"refresh":items.iter().map(|m|m["id"].clone()).collect::<Vec<_>>()}))
}

fn restore(lib: &Library, j: &Value, strict: bool) -> Result<()> {
    let items=j["models"].as_array().ok_or_else(||anyhow!("Invalid recovery record."))?;
    let kept=relayout::kept_dir(lib,s(j,"id")).join("models");
    // Preflight all recoveries: never start a multi-model restore with a known conflict.
    for (i,m) in items.iter().enumerate() {
        let source=extract::checked(lib.root(),s(m,"rel"))?;
        let saved=kept.join(i.to_string());
        if saved.exists() {
            if source.exists() { bail!("{} has been reused; nothing was overwritten.",s(m,"rel")); }
            if strict && extract::hashes(&saved)? != m["manifest"] {
                bail!("The recovery copy of {} was changed; resolve it before restoring.",s(m,"name"));
            }
        } else if !source.is_dir() || (strict && extract::hashes(&source)? != m["manifest"]) {
            bail!("The recovery copy of {} is missing or changed.",s(m,"name"));
        }
    }
    for (i,m) in items.iter().enumerate().rev() {
        let source=extract::checked(lib.root(),s(m,"rel"))?;
        let saved=kept.join(i.to_string());
        if saved.is_dir() {
            std::fs::create_dir_all(source.parent().unwrap_or(Path::new(".")))?;
            std::fs::rename(saved,source)?;
        }
    }
    Ok(())
}
pub fn undo(lib:&Library,mut j:Value)->Result<Value> {
    match restore(lib,&j,true) {
        Ok(()) => {
            j["state"]=json!("undone");
            relayout::write(lib,&j)?;
            Ok(json!({"journal":j["id"],"state":"undone","failed":[]}))
        }
        Err(e) => {
            j["state"]=json!("stopped"); j["error"]=json!(e.to_string());
            relayout::write(lib,&j)?; Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::tests::temp_dir;
    #[test]
    fn deletion_remains_restorable_after_more_than_twenty_changes() {
        let root = temp_dir("model-delete-retention");
        let lib = Library::open(&root).unwrap();
        let dir = root.join("Unsorted/Recover Me");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("unique.part"), b"the only copy").unwrap();
        std::fs::write(
            dir.join("model.json"),
            r#"{"name":"Recover Me","id":"recover-me"}"#,
        ).unwrap();
        let ix = Index::build(&lib, None, true);
        let model_id = ix.models[0].id();
        let reviewed = plan(&lib, &ix, &json!({"ids": [model_id]})).unwrap();
        let result = execute(&lib, &reviewed, &AtomicBool::new(false), &|_, _, _| {}).unwrap();
        let jid = s(&result, "journal").to_owned();
        let saved = relayout::kept_dir(&lib, &jid).join("models/0/unique.part");
        assert!(!dir.exists());
        assert_eq!(std::fs::read(&saved).unwrap().as_slice(), b"the only copy");

        // Ordinary later changes used to evict the journal and recursively
        // erase the only copy. Unrelated newer changes must not block restore.
        for n in 0..26 {
            let next = relayout::new_id(&lib).unwrap();
            relayout::record(
                &lib,
                &next,
                &json!({"kind":"move","label":format!("unrelated change {n}"),"moves":[]}),
            ).unwrap();
        }
        assert_eq!(relayout::read(&lib, &jid).unwrap()["state"], "done");
        assert_eq!(std::fs::read(&saved).unwrap().as_slice(), b"the only copy");
        let brief = relayout::briefs(&lib)
            .into_iter()
            .find(|j| j["id"] == jid)
            .expect("delete must stay in recoverable history");
        assert_eq!(brief["undo"], true, "restore was blocked: {brief}");

        relayout::undo(&lib, &jid, &AtomicBool::new(false), &|_, _, _| {}).unwrap();
        assert_eq!(std::fs::read(dir.join("unique.part")).unwrap().as_slice(), b"the only copy");
        assert_eq!(Index::build(&lib, None, true).models.len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn delete_and_restore_two_models_without_deleting_category() {
        let root=temp_dir("model-delete");
        let lib=Library::open(&root).unwrap();
        for n in ["One","Two"] {
            let dir=root.join("Unsorted").join(n);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("part.xyz"),n).unwrap();
            std::fs::write(dir.join("model.json"),format!(r#"{{"name":"{n}","id":"{n}"}}"#)).unwrap();
        }
        let ix=Index::build(&lib,None,true);
        let ids:Vec<_>=ix.models.iter().map(|m|json!(m.id())).collect();
        let p=plan(&lib,&ix,&json!({"ids":ids})).unwrap();
        let result=execute(&lib,&p,&AtomicBool::new(false),&|_,_,_|{}).unwrap();
        assert_eq!(Index::build(&lib,None,true).models.len(),0);
        assert!(root.join("Unsorted").is_dir());
        let mut j=relayout::read(&lib,s(&result,"journal")).unwrap();
        let recovered=relayout::kept_dir(&lib,s(&result,"journal")).join("models/0/part.xyz");
        std::fs::write(&recovered,"tampered").unwrap();
        assert!(undo(&lib,j.clone()).is_err());
        std::fs::write(&recovered, if s(&p["models"][0],"name")=="One" {"One"} else {"Two"}).unwrap();
        j=relayout::read(&lib,s(&result,"journal")).unwrap();
        undo(&lib,j).unwrap();
        assert_eq!(Index::build(&lib,None,true).models.len(),2);
    }
}