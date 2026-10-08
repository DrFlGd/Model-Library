//! Extract selected model files transactionally. Originals survive staging and cancellation.
use crate::{archive, config, import, index::Index, library::Library, model, relayout, thumb};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::{Path, PathBuf}, sync::atomic::{AtomicBool, Ordering}};

fn s<'a>(v: &'a Value, k: &str) -> &'a str { v[k].as_str().unwrap_or("") }
fn safe(rel: &str) -> Result<PathBuf> {
    if rel.is_empty() || rel.contains('\\') || rel.contains(':') || rel.split('/').any(|x| x.is_empty() || x == "." || x == "..") { bail!("Choose a path inside the model."); }
    let p = PathBuf::from(rel);
    if p.is_absolute() { bail!("Choose a path inside the model."); }
    Ok(p)
}
fn checked(root: &Path, rel: &str) -> Result<PathBuf> {
    let p = safe(rel)?;
    let mut at = root.to_path_buf();
    for part in p.components() { at.push(part); if std::fs::symlink_metadata(&at).is_ok_and(|m| m.file_type().is_symlink()) { bail!("Links cannot be extracted."); } }
    Ok(at)
}
fn walk(root: &Path, rel: &str, out: &mut BTreeMap<String, Option<String>>) -> Result<()> {
    let p = checked(root, rel)?;
    if p.is_dir() {
        for e in std::fs::read_dir(p)? { let e = e?; walk(root, &format!("{rel}/{}", e.file_name().to_string_lossy()), out)?; }
    } else if p.is_file() {
        if rel == model::SIDECAR || rel.starts_with("_thumbs/") { bail!("The model's details and preview stay with it."); }
        out.insert(rel.to_string(), None);
    } else { bail!("{rel} is no longer there."); }
    Ok(())
}

pub fn plan(lib: &Library, ix: &Index, args: &Value) -> Result<Value> {
    let m = ix.get(s(args,"id")).ok_or_else(|| anyhow!("Read the library again: that model is gone."))?;
    let root = checked(lib.root(), m.rel())?;
    let mut selected = BTreeMap::new();
    let mut anchors = vec![];
    for f in args["files"].as_array().into_iter().flatten() {
        let f = f.as_str().ok_or_else(|| anyhow!("Choose files inside the model."))?;
        anchors.push(safe(f)?.parent().unwrap_or(Path::new("")).to_path_buf());
        walk(&root, f, &mut selected)?;
    }
    for e in args["entries"].as_array().into_iter().flatten() {
        let (file, entry) = (s(e,"file"), s(e,"entry"));
        let zip = checked(&root, file)?;
        let ep=safe(entry.trim_end_matches('/'))?;
        anchors.push(ep.parent().unwrap_or(Path::new("")).to_path_buf());
        let available = archive::list(&zip)?;
        let mut found = false;
        for item in available { let n = s(&item,"name"); if n == entry || n.starts_with(&format!("{}/",entry.trim_end_matches('/'))) {
            safe(n)?;
            selected.insert(format!("{file}!{n}"), Some(n.to_string()));
            found = true;
        }}
        if !found { bail!("{entry} is no longer in the archive."); }
    }
    if selected.is_empty() { bail!("Choose files to make a new model."); }
    let all = model::list_files(&root);
    if !all.is_empty() && all.iter().filter(|(f,_)| !f.starts_with("_thumbs/")).all(|(f,_)| selected.contains_key(f)) { bail!("That's the whole model: use Move to category instead."); }
    let mut base = anchors.first().cloned().unwrap_or_default();
    for p in &anchors { while !p.starts_with(&base) { if !base.pop() { break; } } }
    let name = s(args,"name").trim();
    if name.is_empty() { bail!("Give the new model a name."); }
    let sid = args.get("schema").unwrap_or(&m.v["schema"]);
    let schema = match sid.as_str().filter(|x| !x.is_empty()) { Some(id) => Some(ix.schema(id).ok_or_else(|| anyhow!("Choose an existing category."))?), None => None };
    let values: Vec<String> = args.get("values").unwrap_or(&m.v["path"]).as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
    let folder = import::folder_name(schema.map_or("{name}", |x| &x.model_folder), name, m.v["authors"][0].as_str().unwrap_or(""));
    let dest = import::destination(lib, schema, &values, &folder, None, &Default::default())?;
    // Reject symlink destinations, including ancestors that already exist.
    checked(lib.root(), &lib.relative(&dest).unwrap_or_default())?;
    let mut used = std::collections::HashSet::new();
    let mut files = vec![];
    for (from, entry) in selected {
        let logical = entry.as_deref().unwrap_or(&from);
        let to = Path::new(logical).strip_prefix(&base)?.to_string_lossy().replace('\\',"/");
        if to == model::SIDECAR || to.starts_with("_thumbs/") || !used.insert(to.to_lowercase()) { bail!("Selected files have conflicting names in the new model."); }
        let file = if entry.is_some() { from.strip_suffix(&format!("!{}",entry.as_ref().unwrap())).unwrap().to_string() } else { from.clone() };
        files.push(json!({"from":from,"to":to,"file":file,"entry":entry}));
    }
    Ok(json!({"dest":lib.relative(&dest),"files":files,"source":m.rel(),"source_id":m.id(),"name":name,"metadata":m.v,"schema":sid,"values":values}))
}
fn stop(cancel: &AtomicBool) -> Result<()> { if cancel.load(Ordering::Relaxed) { bail!("Stopped."); } Ok(()) }
fn snapshot(root: &Path, keep: &Path, file: &str) -> Result<bool> {
    let src = root.join(file);
    if !src.is_file() { return Ok(false); }
    let dst = keep.join(file); std::fs::create_dir_all(dst.parent().unwrap())?; std::fs::copy(src,dst)?; Ok(true)
}
fn restore(root: &Path, keep: &Path, file: &str, had: bool) -> Result<()> {
    let dst = root.join(file);
    if had { std::fs::create_dir_all(dst.parent().unwrap())?; std::fs::copy(keep.join(file),dst)?; }
    else if dst.exists() { std::fs::remove_file(dst)?; }
    Ok(())
}

pub fn execute(lib: &Library, plan: &Value, mode: &str, cancel: &AtomicBool, progress: &dyn Fn(usize,usize,&str)) -> Result<Value> {
    lib.writable()?;
    if !matches!(mode,"move"|"copy") { bail!("Choose Move or Copy."); }
    stop(cancel)?;
    let (src,dest) = (checked(lib.root(),s(plan,"source"))?,checked(lib.root(),s(plan,"dest"))?);
    if dest.exists() { bail!("The destination is now taken. Preview it again."); }
    let id = relayout::new_id(lib)?;
    let keep = relayout::kept_dir(lib,&id);
    std::fs::create_dir_all(&keep)?;
    let mut j = plan.clone();
    j["kind"]=json!("extract"); j["mode"]=json!(mode); j["id"]=json!(id); j["label"]=json!(format!("Made {} a new model",s(plan,"name")));
    j["moves"]=json!([{"from":plan["source"],"to":plan["dest"]}]);
    j["sidecar_before"]=model::read_sidecar(&src);
    j["source_dirs"]=json!(directories(&src)?);
    j["had_sidecar"]=json!(snapshot(&src,&keep,model::SIDECAR)?);
    j["had_thumb"]=json!(snapshot(&src,&keep,thumb::THUMB)?);
    j["created"]=json!(crate::library::now()); j["state"]=json!("running");
    let stage = dest.parent().unwrap().join(format!(".extract-{id}"));
    j["stage"]=json!(lib.relative(&stage));
    relayout::write(lib,&j)?;
    std::fs::create_dir_all(stage.parent().unwrap())?;
    std::fs::create_dir(&stage)?;
    let files = plan["files"].as_array().unwrap();
    let result = (|| -> Result<Value> {
        for (i,f) in files.iter().enumerate() {
            progress(i,files.len(),s(f,"from")); stop(cancel)?;
            let to = stage.join(safe(s(f,"to"))?);
            std::fs::create_dir_all(to.parent().unwrap())?;
            let from = checked(&src,s(f,"file"))?;
            if let Some(entry) = f["entry"].as_str() {
                use std::io::{Read,Write};
                let mut z = zip::ZipArchive::new(std::fs::File::open(from)?)?;
                let mut e = z.by_name(entry)?;
                let mut out = std::fs::File::create(to)?;
                let mut buf = vec![0;1<<20];
                loop { stop(cancel)?; let n=e.read(&mut buf)?; if n==0 {break;} out.write_all(&buf[..n])?; }
                out.sync_all()?;
            } else {
                import::transfer(&from,&to,false,true,&import::Progress{cancel,on_bytes:&|_|{}})?;
            }
        }
        stop(cancel)?;
        let mut side = json!({"format":1,"id":model::new_id(),"name":plan["name"],"split_from":plan["source_id"],"added":crate::library::now()});
        for k in ["authors","tags","source","released"] { if !plan["metadata"][k].is_null() { side[k]=plan["metadata"][k].clone(); } }
        if !plan["schema"].is_null() { side["schema"]=plan["schema"].clone(); side["path"]=plan["values"].clone(); }
        config::write_json(&stage.join(model::SIDECAR),&side)?;
        j["new_sidecar_after"]=side.clone(); j["original_hashes"]=hashes(&stage)?; relayout::write(lib,&j)?;
        std::fs::create_dir_all(dest.parent().unwrap())?;
        // Stage beside the destination, including when that category is on another drive.
        std::fs::rename(&stage,&dest)?;
        if mode=="move" {
            // Check every original before removing any: files may change while staging.
            for f in files { if f["entry"].is_null() && hash(&checked(&src,s(f,"file"))?)? != j["original_hashes"][s(f,"to")].as_str().unwrap_or("") { bail!("A selected file changed while it was copied. Try again."); } }
            j["source_touched"]=json!(true); relayout::write(lib,&j)?;
            for f in files { stop(cancel)?; if f["entry"].is_null() { std::fs::remove_file(checked(&src,s(f,"file"))?)?; } }
            let before=model::read_sidecar(&src);
            let cover=s(&before,"cover").to_string();
            model::update(&src,&json!({}),&plan["metadata"])?;
            if !cover.is_empty() && !src.join(&cover).is_file() { model::update(&src,&json!({"cover":null}),&plan["metadata"])?; }
            for f in files { let path=src.join(s(f,"file")); let mut p=path.parent(); while let Some(d)=p { if d==src || std::fs::remove_dir(d).is_err() {break;} p=d.parent(); } }
            let _=std::fs::remove_file(src.join(thumb::THUMB)); let _=thumb::make(&src);
        }
        let _=thumb::make(&dest);
        j["sidecar_after"]=model::read_sidecar(&src); j["dest_hashes"]=hashes(&dest)?; j["source_sidecar_hash"]=json!(hash(&src.join(model::SIDECAR)).ok()); j["state"]=json!("done"); j["finished"]=json!(crate::library::now()); relayout::write(lib,&j)?;
        Ok(json!({"id":side["id"],"rel":plan["dest"],"journal":id,"refresh":[plan["source_id"]]}))
    })();
    if let Err(error)=result {
        // Ignore cancellation during rollback: restoring the complete source is mandatory.
        match rollback(lib,&j,false) { Ok(()) => { j["state"]=json!("undone"); }, Err(e) => { j["state"]=json!("stopped"); j["error"]=json!(format!("{error}; restore failed: {e}")); relayout::write(lib,&j)?; return Err(anyhow!("{error}; restore failed: {e}. Use Undo in Recent changes.")); } }
        let _=std::fs::remove_dir_all(stage); relayout::write(lib,&j)?; return Err(error);
    }
    result
}
fn directories(root:&Path)->Result<Vec<String>> {
    fn visit(root:&Path,at:&Path,out:&mut Vec<String>)->Result<()> {
        for e in std::fs::read_dir(at)? { let e=e?; if e.file_type()?.is_dir() { let p=e.path(); out.push(p.strip_prefix(root)?.to_string_lossy().replace('\\',"/")); visit(root,&p,out)?; } } Ok(())
    }
    let mut out=vec![]; visit(root,root,&mut out)?; Ok(out)
}
fn hash(path:&Path)->Result<String> {
    use sha2::{Digest,Sha256}; use std::io::Read;
    let mut f=std::fs::File::open(path)?; let mut h=Sha256::new(); let mut b=vec![0;1<<20];
    loop { let n=f.read(&mut b)?; if n==0 {break;} h.update(&b[..n]); }
    Ok(hex::encode(h.finalize()))
}
fn hashes(root:&Path)->Result<Value> {
    fn collect(root:&Path,dir:&Path,out:&mut Value)->Result<()> {
        for e in std::fs::read_dir(dir)? { let e=e?; let p=e.path(); if e.file_type()?.is_symlink() {bail!("A link was added to the model.");} if p.is_dir() {collect(root,&p,out)?;} else {out[p.strip_prefix(root)?.to_string_lossy().replace('\\',"/")]=json!(hash(&p)?);} } Ok(())
    }
    let mut out=json!({}); collect(root,root,&mut out)?; Ok(out)
}
fn rollback(lib:&Library,j:&Value, strict:bool)->Result<()> {
    let src=checked(lib.root(),s(j,"source"))?; let dest=checked(lib.root(),s(j,"dest"))?;
    let keep=relayout::kept_dir(lib,s(j,"id"));
    let files=j["files"].as_array().unwrap();
    let owns_dest=dest.is_dir() && !j["new_sidecar_after"]["id"].is_null() && model::read_sidecar(&dest)["id"]==j["new_sidecar_after"]["id"];
    if dest.exists() && !owns_dest { bail!("The destination belongs to another model. Nothing was removed."); }
    if strict {
        if !owns_dest || hashes(&dest)? != j["dest_hashes"] { bail!("The new model changed outside the app. Restore it before undoing."); }
        if json!(hash(&src.join(model::SIDECAR)).ok()) != j["source_sidecar_hash"] { bail!("The source model's details changed. Restore them before undoing."); }
    }
    if j["mode"]=="move" {
        if strict { for f in files { if f["entry"].is_null() && checked(&src,s(f,"file"))?.exists() && hash(&checked(&src,s(f,"file"))?)? != j["original_hashes"][s(f,"to")].as_str().unwrap_or("") { bail!("A file is back at {}. Move it aside before undoing.",s(f,"file")); } } }
        for f in files { if f["entry"].is_null() {
            let old=checked(&src,s(f,"file"))?;
            if !old.exists() { if !owns_dest { bail!("The extracted file is missing; cannot restore it."); } let new=checked(&dest,s(f,"to"))?; import::transfer(&new,&old,false,true,&import::Progress{cancel:&AtomicBool::new(false),on_bytes:&|_|{}})?; }
        }}
    }
    if j["source_touched"]==true {
        restore(&src,&keep,model::SIDECAR,j["had_sidecar"]==true)?;
        restore(&src,&keep,thumb::THUMB,j["had_thumb"]==true)?;
    }
    if j["source_touched"]==true { for d in j["source_dirs"].as_array().into_iter().flatten().filter_map(Value::as_str) {std::fs::create_dir_all(checked(&src,d)?)?;} }
    if owns_dest { std::fs::remove_dir_all(dest)?; }
    if let Some(stage)=j["stage"].as_str() { let p=checked(lib.root(),stage)?; if p.exists() { std::fs::remove_dir_all(p)?; } }
    Ok(())
}
pub fn undo(lib:&Library,mut j:Value)->Result<Value> {
    match rollback(lib,&j,j["finished"].is_string()) {
        Ok(()) => { j["state"]=json!("undone"); relayout::write(lib,&j)?; Ok(json!({"journal":j["id"],"state":"undone","failed":[],"refresh":[j["source_id"]]})) }
        Err(e) => { j["state"]=json!("stopped"); j["error"]=json!(e.to_string()); relayout::write(lib,&j)?; Err(e) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::tests::temp_dir;
    fn setup(label:&str)->(Library,Index,String) {
        let root=temp_dir(label); let lib=Library::open(&root).unwrap();
        let dir=root.join("Unsorted/Kit"); std::fs::create_dir_all(dir.join("Arms")).unwrap();
        for f in ["Arms/left.stl","Arms/right.stl","body.stl","cover.png"] {std::fs::write(dir.join(f),f).unwrap();}
        std::fs::write(dir.join(model::SIDECAR),b"{ \"id\": \"source-kit\", \"name\": \"Kit\", \"authors\": [\"Maker\"], \"cover\": \"cover.png\", \"custom\": 42 }\n").unwrap();
        std::fs::create_dir_all(dir.join("_thumbs")).unwrap(); std::fs::write(dir.join(thumb::THUMB),b"original preview").unwrap();
        let ix=Index::build(&lib,None,true); let id=ix.models[0].id().to_string(); (lib,ix,id)
    }
    #[test]
    fn force_copy_move_and_exact_undo() {
        let (lib,ix,id)=setup("extract-move"); let src=lib.root().join("Unsorted/Kit");
        let original=hashes(&src).unwrap();
        let p=plan(&lib,&ix,&json!({"id":id,"files":["Arms/left.stl","Arms/right.stl","cover.png"],"name":"Arms","force_copy":true})).unwrap();
        let r=execute(&lib,&p,"move",&AtomicBool::new(false),&|_,_,_|{}).unwrap();
        let dest=lib.resolve(s(&r,"rel")).unwrap();
        assert!(dest.join("Arms/left.stl").is_file()); assert!(!src.join("Arms/left.stl").exists());
        assert_eq!(model::read_sidecar(&dest)["split_from"],id); assert_eq!(model::read_sidecar(&dest)["authors"],json!(["Maker"]));
        assert!(model::read_sidecar(&src)["cover"].is_null());
        relayout::undo(&lib,s(&r,"journal"),&AtomicBool::new(false),&|_,_,_|{}).unwrap();
        assert_eq!(hashes(&src).unwrap(),original); assert!(!dest.exists());
    }
    #[test]
    fn cancelled_multi_file_stage_leaves_source_whole() {
        let (lib,ix,id)=setup("extract-stop"); let src=lib.root().join("Unsorted/Kit"); let before=hashes(&src).unwrap();
        let p=plan(&lib,&ix,&json!({"id":id,"files":["Arms"],"name":"Arms","force_copy":true})).unwrap();
        let cancel=AtomicBool::new(false);
        assert!(execute(&lib,&p,"move",&cancel,&|i,_,_|{if i==1 {cancel.store(true,Ordering::Relaxed);}}).is_err());
        assert_eq!(hashes(&src).unwrap(),before); assert!(!lib.resolve(s(&p,"dest")).unwrap().exists());
    }
    #[test]
    fn zip_entries_copy_out_and_undo_preserves_zip() {
        use std::io::Write;
        let (lib,_,_)=setup("extract-zip"); let src=lib.root().join("Unsorted/Kit");
        let mut z=zip::ZipWriter::new(std::fs::File::create(src.join("parts.zip")).unwrap());
        z.start_file("Parts/hand.stl",zip::write::SimpleFileOptions::default()).unwrap(); z.write_all(b"solid hand").unwrap(); z.finish().unwrap();
        let ix=Index::build(&lib,None,true); let id=ix.models[0].id(); let before=hashes(&src).unwrap();
        let p=plan(&lib,&ix,&json!({"id":id,"entries":[{"file":"parts.zip","entry":"Parts/hand.stl"}],"name":"Hand"})).unwrap();
        let r=execute(&lib,&p,"move",&AtomicBool::new(false),&|_,_,_|{}).unwrap();
        assert_eq!(std::fs::read(lib.resolve(s(&r,"rel")).unwrap().join("hand.stl")).unwrap(),b"solid hand");
        relayout::undo(&lib,s(&r,"journal"),&AtomicBool::new(false),&|_,_,_|{}).unwrap(); assert_eq!(hashes(&src).unwrap(),before);
    }
    #[test]
    fn copy_undo_refuses_external_changes_and_newer_changes() {
        let (lib,ix,id)=setup("extract-conflict");
        let p=plan(&lib,&ix,&json!({"id":id,"files":["Arms/left.stl"],"name":"Left"})).unwrap();
        let r=execute(&lib,&p,"copy",&AtomicBool::new(false),&|_,_,_|{}).unwrap();
        let newer=relayout::new_id(&lib).unwrap(); relayout::record(&lib,&newer,&json!({"kind":"move","label":"newer","moves":[]})).unwrap();
        assert!(relayout::undo(&lib,s(&r,"journal"),&AtomicBool::new(false),&|_,_,_|{}).is_err());
        let mut n=relayout::read(&lib,&newer).unwrap(); n["state"]=json!("undone"); relayout::write(&lib,&n).unwrap();
        let added=lib.resolve(s(&r,"rel")).unwrap().join("new.txt"); std::fs::write(&added,"keep me").unwrap();
        assert!(relayout::undo(&lib,s(&r,"journal"),&AtomicBool::new(false),&|_,_,_|{}).is_err()); assert!(added.is_file());
    }
    #[test]
    fn rejects_unsafe_paths_and_whole_model() {
        let (lib,ix,id)=setup("extract-safe");
        for f in ["../body.stl","/body.stl","C:/body.stl","model.json",""] { assert!(plan(&lib,&ix,&json!({"id":id,"files":[f],"name":"Bad"})).is_err(),"{f}"); }
        assert!(plan(&lib,&ix,&json!({"id":id,"files":["Arms","body.stl","cover.png"],"name":"All"})).is_err());
    }
}
